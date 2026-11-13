// siafudb-core/src/sync/capture.rs
//
// Capture of local writes from the engine's CDC log, and apply of remote
// changes. All of it runs under the mutex `SiafuDB` keeps this state in.

use super::{ChangeValue, GID_PROPERTY, Gid, GraphChange, gid_from_hex, gid_to_hex};
use crate::error::SiafuError;
use grafeo::cdc::{ChangeEvent, ChangeKind, EntityId};
use grafeo::store::{Direction, EdgeId, EpochId, NodeId};
use std::collections::HashMap;

fn engine_err(e: grafeo::Error) -> SiafuError {
    SiafuError::EngineError(e.to_string())
}

fn reserved(key: &str) -> bool {
    key == GID_PROPERTY
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Entity {
    Node(u64),
    Edge(u64),
}

/// What an apply did to the id maps, to take back if it fails.
enum Undo {
    Bound(Entity),
    Unbound(Entity, Gid),
}

/// The sync state of one database: the gid of every entity that was ever
/// captured or applied, the position in the engine's CDC log, and the
/// changes captured but not yet taken.
pub(crate) struct SyncState {
    nodes: HashMap<u64, Gid>,
    gid_nodes: HashMap<Gid, u64>,
    edges: HashMap<u64, Gid>,
    gid_edges: HashMap<Gid, u64>,
    /// The last commit epoch whose events were captured. Commit epochs are
    /// unique and a commit's events are recorded whole, in epoch order.
    cursor: u64,
    pending: Vec<GraphChange>,
    /// Entities bound to a gid in memory whose `_gid` property is still to
    /// be written.
    unstamped: Vec<Entity>,
    /// A captured value that could not be reported (the property was left
    /// out); returned by the next `take`.
    error: Option<SiafuError>,
}

impl SyncState {
    /// Build the state of a database: index every entity that already has a
    /// gid (one scan), and start capturing after the current epoch.
    pub(crate) fn load(
        engine: &grafeo::GrafeoDB,
        pending: Vec<GraphChange>,
    ) -> Result<Self, SiafuError> {
        let mut state = Self {
            nodes: HashMap::new(),
            gid_nodes: HashMap::new(),
            edges: HashMap::new(),
            gid_edges: HashMap::new(),
            cursor: 0,
            pending,
            unstamped: Vec::new(),
            error: None,
        };
        let session = engine.session_with_cdc(false);
        let scan = |query: &str| -> Result<Vec<(u64, Gid)>, SiafuError> {
            let result = session.execute(query).map_err(engine_err)?;
            let mut found: Vec<(u64, Gid)> = result
                .rows()
                .iter()
                .filter_map(|row| match (row.first(), row.get(1)) {
                    (Some(grafeo::Value::Int64(id)), Some(grafeo::Value::String(hex))) => {
                        Some((u64::try_from(*id).ok()?, gid_from_hex(hex)?))
                    }
                    _ => None,
                })
                .collect();
            // Should two entities hold one gid (a user copied the property),
            // the older keeps it and the other gets its own when next touched.
            found.sort_unstable();
            Ok(found)
        };
        for (id, gid) in scan(&format!(
            "MATCH (n) WHERE n.{GID_PROPERTY} IS NOT NULL RETURN id(n), n.{GID_PROPERTY}"
        ))? {
            if !state.gid_nodes.contains_key(&gid) {
                state.bind(Entity::Node(id), gid);
            }
        }
        for (id, gid) in scan(&format!(
            "MATCH ()-[r]->() WHERE r.{GID_PROPERTY} IS NOT NULL RETURN id(r), r.{GID_PROPERTY}"
        ))? {
            if !state.gid_edges.contains_key(&gid) {
                state.bind(Entity::Edge(id), gid);
            }
        }
        Ok(state)
    }

    pub(crate) fn pending(&self) -> &[GraphChange] {
        &self.pending
    }

    /// Whether the engine's log holds events not captured yet.
    pub(crate) fn behind(&self, engine: &grafeo::GrafeoDB) -> bool {
        !engine
            .cdc_log()
            .changes_between(EpochId(self.cursor + 1), EpochId(u64::MAX))
            .is_empty()
    }

    /// Hand over everything captured so far.
    pub(crate) fn take(&mut self) -> Result<Vec<GraphChange>, SiafuError> {
        match self.error.take() {
            Some(error) => Err(error),
            None => Ok(std::mem::take(&mut self.pending)),
        }
    }

    fn bind(&mut self, entity: Entity, gid: Gid) {
        match entity {
            Entity::Node(id) => {
                self.nodes.insert(id, gid);
                self.gid_nodes.insert(gid, id);
            }
            Entity::Edge(id) => {
                self.edges.insert(id, gid);
                self.gid_edges.insert(gid, id);
            }
        }
    }

    fn unbind(&mut self, entity: Entity) -> Option<Gid> {
        match entity {
            Entity::Node(id) => {
                let gid = self.nodes.remove(&id)?;
                self.gid_nodes.remove(&gid);
                Some(gid)
            }
            Entity::Edge(id) => {
                let gid = self.edges.remove(&id)?;
                self.gid_edges.remove(&gid);
                Some(gid)
            }
        }
    }

    /// Bind a locally written entity to a fresh gid.
    fn mint(&mut self, entity: Entity) -> Gid {
        let gid = *uuid::Uuid::new_v4().as_bytes();
        self.bind(entity, gid);
        self.unstamped.push(entity);
        gid
    }

    // ---------------------------------------------------------------------
    // Capture
    // ---------------------------------------------------------------------

    /// Move the engine's new CDC events into `pending` as changes, and give
    /// every entity they introduce its gid.
    ///
    /// On an error the captured changes are kept (nothing is lost or
    /// reported twice) and the gids still to be written are retried by the
    /// next call.
    pub(crate) fn drain(&mut self, engine: &grafeo::GrafeoDB) -> Result<(), SiafuError> {
        let log = engine.cdc_log();
        let events = log.changes_between(EpochId(self.cursor + 1), EpochId(u64::MAX));
        if events.is_empty() && self.unstamped.is_empty() {
            return Ok(());
        }
        let mut session = engine.session_with_cdc(false);
        for event in &events {
            // Named graphs and RDF triples are not synced.
            if event.graph.is_none() {
                self.capture(&session, event);
            }
            self.cursor = self.cursor.max(event.epoch.0);
        }
        // Everything up to the cursor is captured: the engine need not keep it.
        log.prune_before(EpochId(self.cursor + 1));
        self.stamp(&mut session)
    }

    /// Record `error` for the next `take`, keeping the first.
    fn fail(&mut self, error: SiafuError) {
        self.error.get_or_insert(error);
    }

    /// The reportable properties of an event image, sorted by key. A
    /// property whose value cannot be reported is left out and the error
    /// kept for the next `take`.
    fn properties(
        &mut self,
        image: Option<&HashMap<String, grafeo::Value>>,
    ) -> Vec<(String, ChangeValue)> {
        let mut out = Vec::new();
        for (key, value) in image.into_iter().flatten() {
            if reserved(key) {
                continue;
            }
            match ChangeValue::from_engine(value) {
                Ok(value) => out.push((key.clone(), value)),
                Err(SiafuError::UnsupportedValue(why)) => {
                    self.fail(SiafuError::UnsupportedValue(format!(
                        "property {key:?} was not captured: {why}"
                    )));
                }
                Err(other) => self.fail(other),
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// The gid of a node an event names. A node that was never captured
    /// (written while capture was off) is reported whole first, as it is
    /// now, so the receiver never sees an entity it was not told about.
    fn node_gid(&mut self, session: &grafeo::session::Session, id: u64) -> Gid {
        if let Some(gid) = self.nodes.get(&id) {
            return *gid;
        }
        let gid = self.mint(Entity::Node(id));
        // A node deleted since (its Delete event follows) is still declared.
        let node = session.get_node(NodeId(id)).ok().flatten();
        let (labels, image) = match node {
            Some(node) => (
                node.labels.iter().map(ToString::to_string).collect(),
                node.properties
                    .iter()
                    .map(|(k, v)| (k.as_str().to_string(), v.clone()))
                    .collect::<HashMap<_, _>>(),
            ),
            None => (Vec::new(), HashMap::new()),
        };
        let set = self.properties(Some(&image));
        self.pending.push(GraphChange::NodePut {
            gid,
            labels_add: labels,
            labels_remove: Vec::new(),
            set,
            unset: Vec::new(),
        });
        gid
    }

    /// Replace the queue with the whole default graph as puts (every node,
    /// then every edge), giving a gid to each entity that has none, and hand
    /// it over like [`take`](Self::take).
    ///
    /// Changes captured before describe writes the graph already shows, so
    /// they are dropped; writes after this call are captured as usual.
    pub(crate) fn take_graph(
        &mut self,
        engine: &grafeo::GrafeoDB,
    ) -> Result<Vec<GraphChange>, SiafuError> {
        self.drain(engine)?;
        let mut session = engine.session_with_cdc(false);
        let ids = |query: &str| -> Result<Vec<u64>, SiafuError> {
            let result = session.execute(query).map_err(engine_err)?;
            let mut ids: Vec<u64> = result
                .rows()
                .iter()
                .filter_map(|row| match row.first() {
                    Some(grafeo::Value::Int64(id)) => u64::try_from(*id).ok(),
                    _ => None,
                })
                .collect();
            ids.sort_unstable();
            Ok(ids)
        };
        let nodes = ids("MATCH (n) RETURN id(n)")?;
        let edges = ids("MATCH ()-[r]->() RETURN id(r)")?;
        self.pending.clear();
        self.error = None;
        for id in nodes {
            match self.nodes.get(&id).copied() {
                Some(gid) => {
                    if let Some(put) = self.node_whole(&session, id, gid) {
                        self.pending.push(put);
                    }
                }
                // Reports the node whole as it binds it.
                None => {
                    self.node_gid(&session, id);
                }
            }
        }
        for id in edges {
            let Some(edge) = session.get_edge(EdgeId(id)).ok().flatten() else {
                continue;
            };
            let src = self.node_gid(&session, edge.src.0);
            let dst = self.node_gid(&session, edge.dst.0);
            let gid = match self.edges.get(&id) {
                Some(gid) => *gid,
                None => self.mint(Entity::Edge(id)),
            };
            let image: HashMap<String, grafeo::Value> = edge
                .properties
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.clone()))
                .collect();
            let set = self.properties(Some(&image));
            self.pending.push(GraphChange::EdgePut {
                gid,
                src,
                dst,
                edge_type: edge.edge_type.to_string(),
                set,
                unset: Vec::new(),
            });
        }
        self.stamp(&mut session)?;
        self.take()
    }

    /// A node as it is now, as one put. `None` if it is gone.
    fn node_whole(
        &mut self,
        session: &grafeo::session::Session,
        id: u64,
        gid: Gid,
    ) -> Option<GraphChange> {
        let node = session.get_node(NodeId(id)).ok().flatten()?;
        let image: HashMap<String, grafeo::Value> = node
            .properties
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.clone()))
            .collect();
        let set = self.properties(Some(&image));
        Some(GraphChange::NodePut {
            gid,
            labels_add: node.labels.iter().map(ToString::to_string).collect(),
            labels_remove: Vec::new(),
            set,
            unset: Vec::new(),
        })
    }

    /// The property delta of an Update event: `(set, unset)`.
    fn delta(&mut self, event: &ChangeEvent) -> (Vec<(String, ChangeValue)>, Vec<String>) {
        let set = self.properties(event.after.as_ref());
        let mut unset: Vec<String> = match (&event.before, &event.after) {
            (Some(before), None) => before.keys().filter(|k| !reserved(k)).cloned().collect(),
            _ => Vec::new(),
        };
        unset.sort();
        (set, unset)
    }

    fn capture(&mut self, session: &grafeo::session::Session, event: &ChangeEvent) {
        match (event.entity_id, &event.kind) {
            (EntityId::Node(id), ChangeKind::Create) => {
                let id = id.0;
                // An id met again names a new entity: forget the old one.
                self.unbind(Entity::Node(id));
                let gid = self.mint(Entity::Node(id));
                let set = self.properties(event.after.as_ref());
                self.pending.push(GraphChange::NodePut {
                    gid,
                    labels_add: event.labels.clone().unwrap_or_default(),
                    labels_remove: Vec::new(),
                    set,
                    unset: Vec::new(),
                });
            }
            (EntityId::Node(id), ChangeKind::Update) => {
                let id = id.0;
                let gid = self.node_gid(session, id);
                let (set, unset) = self.delta(event);
                let (mut labels_add, mut labels_remove) = (Vec::new(), Vec::new());
                if let (Some(before), Some(after)) = (&event.before_labels, &event.labels) {
                    labels_add.extend(after.iter().filter(|l| !before.contains(l)).cloned());
                    labels_remove.extend(before.iter().filter(|l| !after.contains(l)).cloned());
                }
                if touches_reserved(event) {
                    // A user write to `_gid`: put ours back.
                    self.unstamped.push(Entity::Node(id));
                }
                if labels_add.is_empty()
                    && labels_remove.is_empty()
                    && set.is_empty()
                    && unset.is_empty()
                {
                    return;
                }
                self.pending.push(GraphChange::NodePut {
                    gid,
                    labels_add,
                    labels_remove,
                    set,
                    unset,
                });
            }
            (EntityId::Node(id), ChangeKind::Delete) => {
                // A node that was never reported needs no delete.
                if let Some(gid) = self.unbind(Entity::Node(id.0)) {
                    self.pending.push(GraphChange::NodeDel { gid });
                }
            }
            (EntityId::Edge(id), ChangeKind::Create) => {
                let id = id.0;
                let (Some(src), Some(dst), Some(edge_type)) =
                    (event.src_id, event.dst_id, &event.edge_type)
                else {
                    return;
                };
                let src = self.node_gid(session, src);
                let dst = self.node_gid(session, dst);
                self.unbind(Entity::Edge(id));
                let gid = self.mint(Entity::Edge(id));
                let set = self.properties(event.after.as_ref());
                self.pending.push(GraphChange::EdgePut {
                    gid,
                    src,
                    dst,
                    edge_type: edge_type.clone(),
                    set,
                    unset: Vec::new(),
                });
            }
            (EntityId::Edge(id), ChangeKind::Update) => {
                let id = id.0;
                // An update event has no endpoints: read them. An edge
                // deleted since is skipped, its Delete event follows.
                let Some(edge) = session.get_edge(EdgeId(id)).ok().flatten() else {
                    return;
                };
                let src = self.node_gid(session, edge.src.0);
                let dst = self.node_gid(session, edge.dst.0);
                let (gid, (set, unset)) = match self.edges.get(&id) {
                    Some(gid) => (*gid, self.delta(event)),
                    // Never captured: reported whole, as it is now.
                    None => {
                        let image: HashMap<String, grafeo::Value> = edge
                            .properties
                            .iter()
                            .map(|(k, v)| (k.as_str().to_string(), v.clone()))
                            .collect();
                        let gid = self.mint(Entity::Edge(id));
                        (gid, (self.properties(Some(&image)), Vec::new()))
                    }
                };
                if touches_reserved(event) {
                    self.unstamped.push(Entity::Edge(id));
                }
                self.pending.push(GraphChange::EdgePut {
                    gid,
                    src,
                    dst,
                    edge_type: edge.edge_type.to_string(),
                    set,
                    unset,
                });
            }
            (EntityId::Edge(id), ChangeKind::Delete) => {
                if let Some(gid) = self.unbind(Entity::Edge(id.0)) {
                    self.pending.push(GraphChange::EdgeDel { gid });
                }
            }
            _ => {}
        }
    }

    /// Write the `_gid` property of the entities bound since the last call,
    /// in one transaction that the CDC log does not see.
    fn stamp(&mut self, session: &mut grafeo::session::Session) -> Result<(), SiafuError> {
        if self.unstamped.is_empty() {
            return Ok(());
        }
        session.begin_transaction().map_err(engine_err)?;
        let mut written = Ok(());
        for entity in &self.unstamped {
            // Entities deleted in the meantime are unbound or gone.
            written =
                match *entity {
                    Entity::Node(id) => match self.nodes.get(&id) {
                        Some(gid) if session.node_exists(NodeId(id)).unwrap_or(false) => session
                            .set_node_property(NodeId(id), GID_PROPERTY, gid_to_hex(gid).into()),
                        _ => Ok(()),
                    },
                    Entity::Edge(id) => match self.edges.get(&id) {
                        Some(gid) if session.edge_exists(EdgeId(id)).unwrap_or(false) => session
                            .set_edge_property(EdgeId(id), GID_PROPERTY, gid_to_hex(gid).into()),
                        _ => Ok(()),
                    },
                };
            if written.is_err() {
                break;
            }
        }
        match written.and_then(|()| session.commit()) {
            Ok(()) => {
                self.unstamped.clear();
                Ok(())
            }
            Err(e) => {
                let _ = session.rollback();
                Err(SiafuError::SyncError(format!(
                    "could not store global ids (will retry): {e}"
                )))
            }
        }
    }

    // ---------------------------------------------------------------------
    // Apply
    // ---------------------------------------------------------------------

    /// Apply remote changes in one transaction that the CDC log does not
    /// see. On an error nothing is applied.
    pub(crate) fn apply(
        &mut self,
        engine: &grafeo::GrafeoDB,
        changes: &[GraphChange],
    ) -> Result<(), SiafuError> {
        // Local writes first, so the id maps are current.
        self.drain(engine)?;
        let mut session = engine.session_with_cdc(false);
        session.begin_transaction().map_err(engine_err)?;
        let mut undo = Vec::new();
        let applied = changes
            .iter()
            .try_for_each(|change| self.apply_one(&session, change, &mut undo))
            .and_then(|()| session.commit().map_err(engine_err));
        if applied.is_err() {
            let _ = session.rollback();
            for step in undo.into_iter().rev() {
                match step {
                    Undo::Bound(entity) => {
                        self.unbind(entity);
                    }
                    Undo::Unbound(entity, gid) => self.bind(entity, gid),
                }
            }
        }
        applied
    }

    fn apply_one(
        &mut self,
        session: &grafeo::session::Session,
        change: &GraphChange,
        undo: &mut Vec<Undo>,
    ) -> Result<(), SiafuError> {
        match change {
            GraphChange::NodePut {
                gid,
                labels_add,
                labels_remove,
                set,
                unset,
            } => {
                check_keys(set, unset)?;
                let Some(&id) = self.gid_nodes.get(gid) else {
                    let labels: Vec<&str> = labels_add.iter().map(String::as_str).collect();
                    let id = session
                        .create_node_with_props(&labels, new_properties(gid, set)?)
                        .map_err(engine_err)?;
                    self.bind(Entity::Node(id.0), *gid);
                    undo.push(Undo::Bound(Entity::Node(id.0)));
                    return Ok(());
                };
                let id = NodeId(id);
                for label in labels_add {
                    session.add_node_label(id, label).map_err(engine_err)?;
                }
                for label in labels_remove {
                    session.remove_node_label(id, label).map_err(engine_err)?;
                }
                for (key, value) in set {
                    match value {
                        ChangeValue::Null => {
                            session.remove_node_property(id, key).map_err(engine_err)?;
                        }
                        value => session
                            .set_node_property(id, key, value.to_engine()?)
                            .map_err(engine_err)?,
                    }
                }
                for key in unset {
                    session.remove_node_property(id, key).map_err(engine_err)?;
                }
            }
            GraphChange::NodeDel { gid } => {
                let Some(&id) = self.gid_nodes.get(gid) else {
                    return Ok(());
                };
                let edges = session
                    .get_node_edges(NodeId(id), Direction::Both, &[])
                    .map_err(engine_err)?;
                for (_, edge) in edges {
                    if let Some(gid) = self.unbind(Entity::Edge(edge.0)) {
                        undo.push(Undo::Unbound(Entity::Edge(edge.0), gid));
                    }
                }
                session.detach_delete_node(NodeId(id)).map_err(engine_err)?;
                self.unbind(Entity::Node(id));
                undo.push(Undo::Unbound(Entity::Node(id), *gid));
            }
            GraphChange::EdgePut {
                gid,
                src,
                dst,
                edge_type,
                set,
                unset,
            } => {
                check_keys(set, unset)?;
                let endpoint = |end: &Gid| {
                    self.gid_nodes
                        .get(end)
                        .copied()
                        .ok_or(SiafuError::UnknownEndpoint {
                            edge: *gid,
                            endpoint: *end,
                        })
                };
                let (src, dst) = (endpoint(src)?, endpoint(dst)?);
                let Some(&id) = self.gid_edges.get(gid) else {
                    let id = session
                        .create_edge_with_props(
                            NodeId(src),
                            NodeId(dst),
                            edge_type,
                            new_properties(gid, set)?,
                        )
                        .map_err(engine_err)?;
                    self.bind(Entity::Edge(id.0), *gid);
                    undo.push(Undo::Bound(Entity::Edge(id.0)));
                    return Ok(());
                };
                let id = EdgeId(id);
                let edge = session.get_edge(id).map_err(engine_err)?.ok_or_else(|| {
                    SiafuError::SyncError(format!(
                        "edge {} is indexed but missing",
                        gid_to_hex(gid)
                    ))
                })?;
                if edge.src.0 != src || edge.dst.0 != dst || edge.edge_type.as_str() != edge_type {
                    return Err(SiafuError::InvalidChange(format!(
                        "edge {} exists with other endpoints or another type",
                        gid_to_hex(gid)
                    )));
                }
                for (key, value) in set {
                    match value {
                        ChangeValue::Null => {
                            session.remove_edge_property(id, key).map_err(engine_err)?;
                        }
                        value => session
                            .set_edge_property(id, key, value.to_engine()?)
                            .map_err(engine_err)?,
                    }
                }
                for key in unset {
                    session.remove_edge_property(id, key).map_err(engine_err)?;
                }
            }
            GraphChange::EdgeDel { gid } => {
                let Some(&id) = self.gid_edges.get(gid) else {
                    return Ok(());
                };
                session.delete_edge(EdgeId(id)).map_err(engine_err)?;
                self.unbind(Entity::Edge(id));
                undo.push(Undo::Unbound(Entity::Edge(id), *gid));
            }
        }
        Ok(())
    }
}

/// Whether an Update event wrote or removed a reserved property.
fn touches_reserved(event: &ChangeEvent) -> bool {
    [&event.before, &event.after]
        .into_iter()
        .flatten()
        .any(|image| image.keys().any(|k| reserved(k)))
}

fn check_keys(set: &[(String, ChangeValue)], unset: &[String]) -> Result<(), SiafuError> {
    match set
        .iter()
        .map(|(k, _)| k)
        .chain(unset)
        .find(|k| reserved(k))
    {
        Some(key) => Err(SiafuError::InvalidChange(format!(
            "property {key:?} is reserved"
        ))),
        None => Ok(()),
    }
}

/// The properties a new entity is created with: its gid and the non-null
/// values of `set`.
fn new_properties(
    gid: &Gid,
    set: &[(String, ChangeValue)],
) -> Result<Vec<(grafeo::store::PropertyKey, grafeo::Value)>, SiafuError> {
    let mut properties = vec![(GID_PROPERTY.into(), gid_to_hex(gid).into())];
    for (key, value) in set {
        if *value != ChangeValue::Null {
            properties.push((key.as_str().into(), value.to_engine()?));
        }
    }
    Ok(properties)
}
