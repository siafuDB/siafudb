# Graph-First Architectural Amendment

### The Paradigm Shift — JanusGraph as Primary Data Engine, Supabase as ACID Ledger

**Date:** April 2026
**Status:** DRAFT — Pending Bryan's review
**Scope:** Affects all three canonical documents — The Mukoko Order v4, Mukoko Architecture v4, Mukoko Manifesto v4
**Companion to:** Layer 3 Amendment (April 2026), Layers 3–4 Revision (March 2026), Layer 4 Orchestration Revision (April 2026), MIT Ancestral Lifecycle Protocol
**Decision Owner:** Bryan Fawcett, Founder & CEO

---

## The Decision

The Mukoko platform adopts a **graph-first** data paradigm. JanusGraph on ScyllaDB/Cassandra becomes the **primary data engine** for all entities, relationships, profiles, preferences, and contextual data across the platform. Supabase/PostgreSQL is reduced to the **ACID ledger** — the narrow set of operations that require serializable multi-row atomic transactions: financial ledgers, the service bus, platform configuration, and RBAC enforcement.

This is not a migration within the existing paradigm. This is a paradigm shift. The relational database was the only tool available when the digital world began — a ledger designed for accounting and structured business transactions in 1970. The graph database is the data structure that mirrors how the human brain works, how AI systems reason, and how the real world is actually connected. Mukoko is frontier infrastructure for a continent of one billion people. It does not inherit the constraints of the previous era. It builds on the paradigm that will define the next.

The three sources of truth remain three. The boundaries shift:

**Source of Truth 1 — The Web3 Pod (personal, sovereign, graph-structured).** Unchanged. The personal source of truth. Graph-structured for native AI reasoning.

**Source of Truth 2 — JanusGraph/ScyllaDB/Cassandra (platform non-relational AND platform entity/relationship graph).** Expanded. Was: content bodies, message streams, time-series data. Now also: all entity vertices (people, organisations, places, communities, content, interest categories), all relationship edges (membership, employment, authorship, location, interest, family, credentials, follows, transactions), all profile and preference data, all contextual data that Shamwari and the Digital Twin reason over. JanusGraph is the query engine. ScyllaDB is the hot storage tier. Cassandra is the cold/heritage tier and sovereignty guarantee.

**Source of Truth 3 — Supabase/PostgreSQL (platform ACID ledger).** Reduced. Retains only: financial transactions (wallet schema), the service bus (transactional event delivery), platform configuration tables (verification tiers, permission definitions, themes, interest category definitions), RBAC enforcement tables, and authentication stubs (minimal identity records for JWT minting). PostgreSQL does what it was born to do — precise, transactional, ACID-guaranteed ledger operations — and nothing else.

Everything else remains unchanged in this document: CouchDB is the sync protocol (Layer 4), Durable Objects and KV are edge caches (Layer 5), the device layer holds local data (Layer 6), Doris is the open data commons (Layer 7). The graph-first transformation of Layers 1, 5, and 6 — replacing SQLite with SiafuDB (the embedded graph engine) across device, edge, and pod — is specified in the Graph Fabric Addendum to this amendment.

---

## The Reasoning

### The Cognitive Argument

Edgar Codd published the relational model in 1970 at IBM. It was designed to represent structured business transactions — rows, columns, foreign keys, ACID guarantees. It is a ledger. It thinks in tables because the problems it solved were tabular: inventory counts, payroll records, bank balances.

The human brain has approximately 86 billion neurons, each connected to roughly 7,000 others, forming approximately 100 trillion synaptic connections. There is no schema. There is no foreign key constraint between a memory of a mother's voice and the smell of rain on dry earth. Those connections exist because experience created them, and they strengthen or weaken based on use. The brain is a weighted, directed, dynamically evolving graph — and the relationships carry as much meaning as the nodes.

Every platform built in the last twenty years — Facebook, Google, Amazon, Netflix — started on relational databases and spent billions of dollars building graph layers, recommendation engines, and knowledge systems on top of them because the relational model could not represent what they needed. They are all playing catch-up against their own foundational decisions.

Mukoko does not carry that technical debt. The foundational decision is being made now, in 2026, with full knowledge of where computing is heading. AI systems — current LLMs, future AGI, and whatever comes after — reason over relationships. They traverse connections. They build context by following edges from node to node. A graph database is the native data structure for machine reasoning, just as it is the native data structure for human cognition.

The question is not whether graph databases represent the future. The question is whether Mukoko builds on that future now or inherits the relational compromise and spends years migrating away from it. Frontier infrastructure does not play catch-up.

### The Technical Argument

JanusGraph on ScyllaDB supports transactional operations with tunable consistency. ScyllaDB and Cassandra allow per-operation consistency levels — `LOCAL_QUORUM` provides strong consistency within a datacenter, `ALL` provides the strongest guarantee, `ONE` provides fastest reads with eventual consistency. JanusGraph inherits these guarantees. JanusGraph also supports lightweight locking for write conflict resolution, enabling safe concurrent graph updates.

The operations that genuinely require PostgreSQL-style serializable isolation with multi-row atomic writes are narrow: financial transfers (wallet A to wallet B where double-spending must be impossible), service bus event delivery (where exactly-once semantics depend on transactional inserts), and RBAC permission checks (where stale reads could grant unauthorized access). These are inherently tabular, inherently transactional, and inherently small. They are what the relational database was designed for.

Everything else — identity profiles, organizational data, credentials, memberships, interests, preferences, family relationships, place associations, content metadata — is relationship data being forced into tables. A person's occupation is not a column. It is an edge to an organisation. A person's interests are not a join table. They are weighted edges to interest category nodes. A business membership is not a row with foreign keys. It is an edge between a person vertex and an organisation vertex with role, permissions, and timestamps as edge properties.

JanusGraph on ScyllaDB handles these operations at sub-millisecond latency for direct lookups (partition key reads via CQL) and millisecond latency for multi-hop traversals (Gremlin queries). A Shamwari context-building query that would require six nested JOINs in PostgreSQL becomes a single Gremlin traversal starting at the person vertex and walking 3-4 hops outward. The performance advantage compounds as the graph grows — graph traversals are O(local neighbourhood), not O(table size).

### The Sovereignty Argument

The Layer 3 Amendment established that ScyllaDB's speed is rented and Cassandra's sovereignty is owned. The same principle applies to the entity graph. Every vertex, every edge, every property stored in JanusGraph on ScyllaDB can be migrated to JanusGraph on Cassandra with zero code changes — because JanusGraph's CQL storage backend is identical across both databases. The graph-first paradigm inherits the sovereignty guarantee from the Layer 3 Amendment without any additional architectural work.

PostgreSQL (Supabase) is itself a sovereignty risk for the entity/relationship layer because Supabase is a managed service. The platform cannot self-host Supabase's managed PostgreSQL without significant operational changes. By moving entity and relationship data to JanusGraph/ScyllaDB/Cassandra — all deployable on Fly.io or self-hosted on Nyuchi's own infrastructure via MAAS/K3s — the platform reduces its dependency on managed services for its most critical data. The ACID ledger that remains in Supabase is small, narrowly-scoped, and could be migrated to self-hosted PostgreSQL if needed with minimal effort.

### The AI Readiness Argument

The Digital Twin's personal data model is already described in the architecture as "graph-structured for native AI reasoning." If the platform's data model is also graph-structured, the sync between platform and pod is graph-to-graph — a subgraph replication, not a relational-to-graph transformation. When a person verifies and their pod is provisioned, the migration is: extract the person's vertex and their personal edges from the platform graph, replicate to the pod's graph. Clean, native, scalable to a billion users.

Shamwari's context-building becomes a pure graph operation. Start at the person's vertex. Traverse MEMBER_OF edges to their organisations. Traverse LOCATED_IN edges to their places. Traverse INTERESTED_IN edges to their categories. Traverse PARTICIPATES_IN edges to their circles. Traverse MESSAGES_WITH edges to their contacts. The entire context model — who this person is, what they care about, who they're connected to, where they are — is built in a single Gremlin traversal. No SQL. No JOINs. No foreign key resolution. The AI reasons over the graph natively.

Ubuntu score computation moves from SQL aggregate queries to graph algorithms. PageRank for influence. Community detection for cluster identification. Betweenness centrality for bridge identification. A person who mentors 10 people who each become active contributors has a different graph signature than someone who writes 100 reviews. Graph metrics produce fairer, richer reputation scores than counting rows in a table.

Recommendation quality improves structurally. "People who are in the same circles as you and who transacted with this seller" is a graph traversal, not a SQL subquery. "Organisations in the same industry, in the same city, that your second-degree connections have verified" is two hops in Gremlin. These graph features, served via Feast (Layer 7), dramatically improve recommendation relevance.

### The 2050 Argument

Building for 2050 and 2100 means making decisions that will not need to be reversed. The relational database as a primary data paradigm is a decision that every major platform has already reversed — at enormous cost. Facebook built TAO (a distributed graph layer) on top of MySQL. Google built Knowledge Graph on top of Bigtable. Amazon built Neptune on top of their existing infrastructure. LinkedIn built a custom graph serving layer. Each of these was a multi-year, multi-billion-dollar effort to escape the relational paradigm.

Mukoko makes the graph-first decision at the beginning. The cost of this decision in 2026 is measured in weeks of schema design and migration planning. The cost of making the relational decision now and reversing it in 2035 would be measured in years of engineering and millions of dollars.

JanusGraph is Apache 2.0. It is governed by the Linux Foundation. It supports hundreds of billions of vertices and edges. It has been proven at 20-billion-node scale on ScyllaDB. Its storage model (CQL) is compatible with both a high-performance commercial database (ScyllaDB) and a fully sovereign open-source database (Cassandra). Its query language (Gremlin/TinkerPop) is the Apache standard for graph computing. Its analytics layer integrates with Apache Spark for OLAP workloads. No component of this stack is controlled by a single corporation. No component can be relicensed out from under the platform.

This is the foundation for a century.

---

## The Migration

### What Stays in Supabase (The ACID Ledger)

Supabase retains five functional domains. Each is inherently tabular, inherently transactional, and small relative to the full data set.

**1. Authentication Stubs (identity schema — reduced)**

The `identity.person` table shrinks to an authentication stub. Only the columns necessary for session validation, JWT minting, platform role enforcement, and MIT lifecycle governance remain.

Columns retained in `identity.person`:

| Column                       | Reason                                                                                                                  |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `id`                         | Primary identifier. UUID. Referenced by JWT `sub` claim.                                                                |
| `stytch_user_id`             | Stytch authentication binding. ACID-critical — must never have stale reads.                                             |
| `email`                      | Authentication identifier. Unique constraint.                                                                           |
| `telephone`                  | Authentication identifier. Unique constraint.                                                                           |
| `name`                       | Display name — cached here for JWT claims and fast UI rendering. Authoritative copy is the Person vertex in JanusGraph. |
| `alternatename`              | Username/handle. Unique constraint requires ACID enforcement.                                                           |
| `role`                       | Platform access control role. RBAC enforcement.                                                                         |
| `auth_method`                | Last authentication method used.                                                                                        |
| `email_verified`             | Verification status for email auth.                                                                                     |
| `email_verified_at`          | Timestamp.                                                                                                              |
| `phone_verified`             | Verification status for phone auth.                                                                                     |
| `phone_verified_at`          | Timestamp.                                                                                                              |
| `mit_status`                 | MIT lifecycle state (living/ancestral). Governs platform-wide behaviour.                                                |
| `last_liveness_verified_at`  | MIT Ancestral Lifecycle Protocol.                                                                                       |
| `next_liveness_due_at`       | MIT Ancestral Lifecycle Protocol.                                                                                       |
| `liveness_grace_expires_at`  | MIT Ancestral Lifecycle Protocol.                                                                                       |
| `liveness_extension_granted` | MIT Ancestral Lifecycle Protocol.                                                                                       |
| `death_verified_at`          | MIT Ancestral Lifecycle Protocol.                                                                                       |
| `death_verification_method`  | MIT Ancestral Lifecycle Protocol.                                                                                       |
| `ancestral_transition_at`    | MIT Ancestral Lifecycle Protocol.                                                                                       |
| `ancestral_reversal_count`   | MIT Ancestral Lifecycle Protocol.                                                                                       |
| `beneficiary_person_id`      | MIT Ancestral Lifecycle Protocol. Beneficiary FK.                                                                       |
| `beneficiary_type`           | MIT Ancestral Lifecycle Protocol.                                                                                       |
| `heritage_opt_out`           | MIT Ancestral Lifecycle Protocol.                                                                                       |
| `pod_provisioned`            | Pod lifecycle tracking.                                                                                                 |
| `pod_endpoint`               | Pod connection.                                                                                                         |
| `pod_stream_id`              | Pod identity binding.                                                                                                   |
| `pod_provisioned_at`         | Pod lifecycle timestamp.                                                                                                |
| `last_login_at`              | Session tracking.                                                                                                       |
| `last_login_method`          | Session tracking.                                                                                                       |
| `last_login_platform`        | Session tracking.                                                                                                       |
| `last_seen_at`               | Activity tracking.                                                                                                      |
| `profile_completed`          | Onboarding state.                                                                                                       |
| `onboarding_completed`       | Onboarding state.                                                                                                       |
| `created_at`                 | System timestamp.                                                                                                       |
| `updated_at`                 | System timestamp.                                                                                                       |

Columns removed from `identity.person` (moved to Person vertex in JanusGraph):

| Column                | Graph Representation                                                            |
| --------------------- | ------------------------------------------------------------------------------- |
| `givenname`           | Person vertex property                                                          |
| `familyname`          | Person vertex property                                                          |
| `additionalname`      | Person vertex property                                                          |
| `honorificprefix`     | Person vertex property                                                          |
| `honorificsuffix`     | Person vertex property                                                          |
| `image`               | Person vertex property                                                          |
| `url`                 | Person vertex property                                                          |
| `birthdate`           | Person vertex property                                                          |
| `deathdate`           | Person vertex property                                                          |
| `gender`              | Person vertex property                                                          |
| `address`             | Person vertex property (jsonb → structured properties)                          |
| `nationality`         | Person vertex property (jsonb → structured properties)                          |
| `jobtitle`            | EMPLOYED_BY edge property                                                       |
| `worksfor`            | EMPLOYED_BY edge to Organisation vertex                                         |
| `affiliation`         | AFFILIATED_WITH edge(s) to Organisation vertices                                |
| `alumniof`            | ALUMNI_OF edge(s) to Organisation vertices                                      |
| `knowslanguage`       | KNOWS_LANGUAGE edge(s) to Language vertices                                     |
| `knowsabout`          | KNOWS_ABOUT edge(s) to Topic/Interest vertices                                  |
| `hasoccupation`       | HAS_OCCUPATION edge to Occupation vertex                                        |
| `award`               | HAS_AWARD edge(s) to Award vertices                                             |
| `contactpoint`        | Person vertex property                                                          |
| `description`         | Person vertex property                                                          |
| `bio`                 | Person vertex property                                                          |
| `cover_image`         | Person vertex property                                                          |
| `portfolio_url`       | Person vertex property                                                          |
| `social_links`        | LINKED_ON edge(s) to Platform vertices                                          |
| `memberof_summary`    | Derived from MEMBER_OF edges (no longer stored — computed from graph)           |
| `active_roles`        | Derived from HAS_ROLE edges (no longer stored — computed from graph)            |
| `total_contributions` | Derived from graph metrics (no longer stored — computed from CONTRIBUTED edges) |
| `total_published`     | Derived from graph metrics (no longer stored — computed from CREATED edges)     |
| `ubuntu_score`        | Derived from graph algorithms (PageRank, centrality — computed by JanusGraph)   |
| `theme_id`            | Person vertex property                                                          |
| `registered_devices`  | Person vertex property                                                          |
| `sameas`              | Person vertex property (social profile URLs)                                    |

Columns removed from `identity.person` (stale references — deleted, not migrated):

| Column                          | Reason                                                                  |
| ------------------------------- | ----------------------------------------------------------------------- |
| `d1_person_id`                  | D1 removed from architecture. Dead reference.                           |
| `d1_synced_at`                  | D1 removed from architecture. Dead reference.                           |
| `sync_version`                  | References D1/PouchDB sync. Dead reference.                             |
| `personal_d1_database_id`       | D1 removed from architecture. Dead reference.                           |
| `personal_d1_database_name`     | D1 removed from architecture. Dead reference.                           |
| `nft_identity_token_id`         | Moves to MIT token management (wallet schema or pod).                   |
| `nft_identity_blockchain`       | Moves to MIT token management.                                          |
| `nft_identity_contract_address` | Moves to MIT token management.                                          |
| `nft_identity_token_uri`        | Moves to MIT token management.                                          |
| `nft_identity_ipfs_hash`        | Moves to MIT token management.                                          |
| `couchdb_doc_id`                | CouchDB is sync-only. Not a source of truth reference.                  |
| `ceramic_did`                   | Superseded by Nyuchi Honeycomb pod architecture.                        |
| `scylladb_doc_id`               | No longer needed — the Person vertex IS the ScyllaDB/JanusGraph record. |

**2. Financial Ledger (wallet schema — unchanged)**

The wallet schema remains entirely in Supabase. Financial transactions (transfers, balances, payment intents, emission records, legacy transfers, swaps, payment methods, MIT tokens, NFT holdings) require serializable multi-row ACID transactions. Double-spending must be impossible. Balance updates must be atomic. This is the purest ledger use case in the platform, and PostgreSQL is the right tool.

**3. Service Bus (service_bus schema — unchanged)**

The service bus remains entirely in Supabase. Transactional event publishing (the `publish_event` function) requires ACID guarantees — an event must be written to the events table and the subscriptions table atomically, or not at all. Maestro reads from the service bus for orchestration. This is inherently transactional.

**4. Platform Configuration (system schema — unchanged)**

The system schema remains in Supabase: verification tiers, verification subject types, verification evidence matrix, themes, activity logs, change history, notifications, unified submissions. These are small, rarely-changing reference tables that benefit from SQL's querying convenience and that other schemas reference via foreign keys. Interest category definitions (`engagement.interest_category`) also stay — the 40 locked categories are platform configuration, not user data. The Interest Category vertices in JanusGraph reference these definitions by UUID.

**5. RBAC Enforcement (identity RBAC tables — unchanged)**

`identity.platform_permission`, `identity.role_permission`, `identity.role_type` remain in Supabase. These are small permission lookup tables (43 permissions, 98 role-permission mappings, 34 role types) that the API layer checks on every authenticated request. They need to be fast, consistent, and ACID-protected.

### What Moves to JanusGraph/ScyllaDB (The Knowledge Graph)

**Identity tables that become graph structures:**

| Supabase Table                                               | Graph Representation                                                                                   | Reasoning                                                                                                      |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| `identity.interest`                                          | `INTERESTED_IN` edge (Person → InterestCategory) with `interest_strength` as edge property             | This is literally a weighted edge stored as a join table.                                                      |
| `identity.credential`                                        | `HAS_CREDENTIAL` edge (Person → Credential vertex) with all credential properties on the vertex        | A credential is an entity connected to a person. The issuer is another edge (Credential → Organisation).       |
| `identity.family` + `identity.family_member`                 | Family subgraph: `FAMILY_MEMBER_OF` edges between Person vertices with `relationship` as edge property | Family relationships are the purest graph structure. Parent-child, spouse, sibling — these are edges.          |
| `identity.person_role`                                       | `HAS_ROLE` edge (Person → Role vertex) with context, status, verification as edge properties           | A role is a relationship between a person and a context (organisation, schema, entity).                        |
| `identity.oauth_connection`                                  | `AUTHENTICATED_VIA` edge (Person → OAuthProvider vertex) with tokens as encrypted edge properties      | Authentication connections are relationships.                                                                  |
| `identity.wallet_address`                                    | `HAS_WALLET` edge (Person → WalletAddress vertex) with blockchain, type as vertex properties           | Wallet addresses are entities connected to people.                                                             |
| `identity.digital_document`                                  | `HAS_DOCUMENT` edge (Person → Document vertex)                                                         | Documents are entities connected to people.                                                                    |
| `identity.verification`                                      | `VERIFIED_AT` edge properties on Person vertex, or dedicated Verification vertex for audit trail       | Verification is a state of the person node. The audit trail (who verified, when, what evidence) is a subgraph. |
| `identity.moderation_action`                                 | `MODERATED` edge (Moderator Person → Target entity) with action, reason, timestamps                    | Moderation is a relationship between a moderator and a target.                                                 |
| `identity.liveness_verification`                             | Subgraph of Person vertex: `LIVENESS_CHECK` edges with outcomes and timestamps                         | Liveness is a lifecycle event connected to a person.                                                           |
| `identity.death_verification` + `identity.death_attestation` | `DEATH_VERIFIED` edge (Person → DeathVerification vertex), `ATTESTED_BY` edges from attesters          | Death verification is a subgraph of relationships — the deceased, the submitter, the attesters, the verifier.  |

**Business tables that become graph structures:**

| Supabase Table          | Graph Representation                                                                                                                                                                                      | Reasoning                                                                                                |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `business.organization` | Organisation vertex with properties. Edges: `FOUNDED_BY` (→ Person), `LOCATED_IN` (→ Place), `CATEGORIZED_AS` (→ Industry/Type), `IN_COUNTRY` (→ Country), `IN_PROVINCE` (→ Province), `IN_CITY` (→ City) | An organisation is a node defined by its connections. Its type, location, industry, founder — all edges. |
| `business.membership`   | `MEMBER_OF` edge (Person → Organisation) with role, permissions, capabilities, verification status as edge properties                                                                                     | Membership is the textbook graph edge use case.                                                          |
| `business.verification` | `VERIFICATION_REQUEST` edge (Person → Organisation) with workflow state, evidence, reviewer as properties                                                                                                 | Business verification is a relationship workflow between a person and an organisation.                   |
| `business.api_key`      | `HAS_API_KEY` edge (Organisation → APIKey vertex) with scopes, limits, status                                                                                                                             | API keys are entities bound to organisations and issued to people.                                       |

**Organisation vertex — columns retained as vertex properties vs. derived from edges:**

Properties on the Organisation vertex: `id`, `name`, `alternatename`, `description`, `url`, `logo`, `foundingdate`, `email`, `telephone`, `address`, `registration_number`, `tax_id`, `employee_count_range`, `member_count`, `total_members`, `created_at`, `updated_at`.

Derived from edges (no longer stored as columns): `founder` (→ FOUNDED_BY edge), `place_id` (→ LOCATED_IN edge), `country_id` (→ IN_COUNTRY edge), `province_id` (→ IN_PROVINCE edge), `city_id` (→ IN_CITY edge), `organizationtype` (→ CATEGORIZED_AS edge to type vertex), `industry` (→ CATEGORIZED_AS edge to industry vertex), `ubuntu_score` (→ computed from graph algorithms).

Verification and capability columns that remain as vertex properties (governance-critical): `verification_status`, `verification_tier`, `verified_at`, `verified_by`, `verification_expires_at`, `can_publish_news`, `can_list_commerce`, `can_manage_events`, `can_claim_places`, `can_run_honeycomb_nodes`, `can_access_api`, `api_tier`, `api_rate_limit_per_hour`.

Note: Verification and capability flags could remain in Supabase as an alternative design if the API layer's permission checks need ACID guarantees. This is a deployment decision — if the FastAPI middleware checks capabilities via JanusGraph with `LOCAL_QUORUM` consistency and acceptable latency, they can live on the vertex. If sub-millisecond ACID reads are required for every API call, a minimal `business.organization_governance` table in Supabase holds only `id`, `verification_status`, `verification_tier`, and the six capability booleans. Bryan to decide.

**Engagement tables that become graph structures:**

| Supabase Table                      | Graph Representation                                                             | Reasoning                                                            |
| ----------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `engagement.follow_action`          | `FOLLOWS` edge (Person → Person/Organisation/Place)                              | Following is a relationship.                                         |
| `engagement.review`                 | `REVIEWED` edge (Person → Entity) with rating, headline, body as edge properties | A review is a relationship between a reviewer and a reviewed entity. |
| `engagement.unverified_interaction` | Untyped interaction edges, flowing to Doris for analytics                        | Interactions are relationships.                                      |
| `engagement.user_action`            | Action edges (Person → Entity) with action type, timestamp                       | Actions are relationships between people and content.                |
| `engagement.interaction_counter`    | Derived from edge counts (no longer stored — computed from graph)                | Counters are graph metrics, not stored values.                       |
| `engagement.recommendation`         | `RECOMMENDED` edge from system to Person, with item reference                    | Recommendations are relationships.                                   |

Note: Tables marked "DORIS DOMAIN" in Supabase (`user_action`, `interaction_counter`, `unverified_interaction`, `recommendation`) are already caches. In the graph-first model, the authoritative engagement data flows through JanusGraph (as edges) → Redpanda (Layer 4) → Flink → Doris (Layer 7). The Supabase cache tables for these can be dropped entirely.

**Ubuntu tables that become graph structures:**

| Supabase Table            | Graph Representation                                                                                                                                        | Reasoning                                                                    |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `ubuntu.contribution`     | `CONTRIBUTED` edge (Person → Entity) with contribution type (content, review, verification, moderation, translation, curation, mentorship) as edge property | Contributions are relationships. The seven types map to seven edge subtypes. |
| `ubuntu.impact_scores`    | Derived from graph algorithms — PageRank, betweenness centrality, community detection                                                                       | Impact is a graph metric, not a stored value.                                |
| `ubuntu.leaderboards`     | Derived from graph metrics                                                                                                                                  | Rankings are computed from the graph.                                        |
| `ubuntu.user_badges`      | `EARNED` edge (Person → Badge vertex)                                                                                                                       | Badge awards are relationships.                                              |
| `ubuntu.mission_progress` | `PROGRESSING` edge (Person → Mission vertex) with completion state                                                                                          | Mission progress is a relationship.                                          |
| `ubuntu.dao_proposals`    | `PROPOSED` edge (Person → Proposal vertex)                                                                                                                  | Governance proposals are relationships.                                      |

Note: `ubuntu.badges` (badge definitions) and `ubuntu.missions` (mission definitions) stay in Supabase as platform configuration — they are reference data, not user data.

**Shamwari tables that become graph structures:**

| Supabase Table              | Graph Representation                                       | Reasoning                                        |
| --------------------------- | ---------------------------------------------------------- | ------------------------------------------------ |
| `shamwari.preference`       | Person vertex properties or `PREFERS` edges                | AI preferences are personal attributes.          |
| `shamwari.user_preferences` | Person vertex properties                                   | AI preferences are personal attributes.          |
| `shamwari.suggestions`      | `SUGGESTED` edge (System → Person) with suggestion content | Suggestions are relationships.                   |
| `shamwari.context_windows`  | Ephemeral — lives in ScyllaDB as session data, not graph   | Context windows are temporary computation state. |
| `shamwari.tool_usage`       | `USED_TOOL` edge (Person → Tool) with timestamp, context   | Tool usage is a relationship.                    |
| `shamwari.feedback`         | `GAVE_FEEDBACK` edge (Person → Conversation)               | Feedback is a relationship.                      |

Note: `shamwari.conversation` already stores only metadata in Supabase (messages are in ScyllaDB). In the graph-first model, conversation metadata becomes a Conversation vertex with `PARTICIPATED_IN` edges from Person vertices. The Supabase record is dropped.

Note: `shamwari.knowledge_base` stays in Supabase — it is platform configuration (features, policies, FAQs, tutorials), not user data.

**Places tables that gain graph edges:**

The places schema is inherently geographic and already has graph-like structures (`places.place_relations` is literally a graph edges table). In the graph-first model:

`places.place_relations` moves entirely to JanusGraph — it is typed edges between Place vertices.

`places.community_confirmation` becomes `CONFIRMED` edges from Person vertices to Place vertices.

`places.place_ownership` becomes `OWNS` or `MANAGES` edges from Person vertices to Place vertices.

`places.places`, `places.countries`, `places.provinces`, `places.places_geo` become Place vertices in the geographic hierarchy — connected by `CONTAINS`, `PART_OF`, `LOCATED_IN` edges. The current geographic foreign keys (`country_id`, `province_id`) become graph edges.

Note: The core places data (2,079 places, 54 countries, 73 provinces, 93 geo places) can remain in Supabase as reference data during the transition, with JanusGraph holding the graph copy. This is a deployment decision — Places is the geographic knowledge graph, and its long-term home is JanusGraph. But the migration can be progressive.

### The Revised Authentication Flow

The current flow: Stytch authenticates → FastAPI validates session + upserts `identity.person` (full row) → FastAPI mints platform JWT → client uses JWT for PostgREST.

The graph-first flow:

1. **Stytch authenticates** (email OTP primary, SMS OTP secondary). Returns `stytch_user_id` and session token.
2. **FastAPI validates session** with Stytch. On first login, upserts the minimal `identity.person` authentication stub in Supabase (id, stytch_user_id, email, telephone, role, mit_status).
3. **FastAPI creates or updates the Person vertex** in JanusGraph with the full profile data. On first login, this is a new vertex with minimal properties. On subsequent logins, this is a property update (last_login_at, last_login_platform).
4. **FastAPI mints platform JWT** signed with Supabase JWT secret. `sub` = `identity.person.id`. The JWT is valid for PostgREST requests to the ACID ledger (wallet, service bus, system config).
5. **Client uses JWT** for PostgREST requests to Supabase (wallet operations, permission checks) AND for authenticated Gremlin queries to JanusGraph via the Graph API (profile reads, relationship traversals, context building).

The key change: the JWT `sub` claim (the person's UUID) is the linkage between the Supabase authentication stub and the JanusGraph Person vertex. The UUID is identical in both systems. This is the same UUID that will be used for the pod's graph node. One UUID, three graph-structured representations: platform graph (JanusGraph), personal graph (pod), and authentication stub (Supabase).

### The Graph API

JanusGraph is accessed via a **Graph API** — a FastAPI service that wraps the Gremlin query engine and exposes typed endpoints for graph operations. This is not raw Gremlin exposed to clients. The Graph API provides:

**Read endpoints** for entity retrieval (get person by UUID, get organisation by UUID, get place by UUID) that return the vertex and its direct edges. These replace PostgREST reads for profile and entity data.

**Traversal endpoints** for context building (get person context with N hops, get organisation network, get place connections) that execute Gremlin traversals and return structured results. These power Shamwari's context-building and recommendation features.

**Write endpoints** for entity mutations (update profile, add membership, add credential, change interest) that execute transactional graph writes with configurable consistency levels. These replace PostgREST writes for profile and relationship data.

**The Graph MCP Server** (already described in the Layer 3 Amendment) exposes graph capabilities to Shamwari and the Analytical Agent. The Graph API and the Graph MCP Server share the same JanusGraph connection pool.

---

## The Extended Graph Schema

The Layer 3 Amendment defined six primary node types. The graph-first amendment expands and refines this to reflect the full entity model:

### Vertex Types

| Vertex Label     | Source                                                                   | Key Properties                                                        |
| ---------------- | ------------------------------------------------------------------------ | --------------------------------------------------------------------- |
| Person           | `identity.person` (auth stub in Supabase, full profile in graph)         | id, givenname, familyname, birthdate, gender, bio, image, description |
| Organisation     | `business.organization` (governance in Supabase, profile in graph)       | id, name, description, url, logo, foundingdate, registration_number   |
| Place            | `places.places` + `places.places_geo`                                    | id, name, schema_type, latitude, longitude, geo                       |
| Country          | `places.countries`                                                       | id, name, iso_code                                                    |
| Province         | `places.provinces`                                                       | id, name                                                              |
| Content          | `content.creative_work` + all mini-app content types                     | id, type, name, headline, datepublished                               |
| InterestCategory | `engagement.interest_category` (definition in Supabase, vertex in graph) | id, name, slug, icon                                                  |
| Circle           | `circles.circle`                                                         | id, name, description, circle_type                                    |
| Conversation     | `campfire.conversation` metadata                                         | id, type, name, created_at                                            |
| Event            | `events.event` metadata                                                  | id, name, startdate, enddate, location                                |
| Credential       | `identity.credential`                                                    | id, name, category, issuer_name, date_issued                          |
| Badge            | `ubuntu.badges`                                                          | id, name, description, category                                       |
| Mission          | `ubuntu.missions`                                                        | id, name, type, status                                                |
| Language         | New — language reference vertices                                        | id, name, iso_code                                                    |
| Occupation       | New — occupation reference vertices (Schema.org Occupation)              | id, name, occupational_category                                       |
| Industry         | New — industry reference vertices                                        | id, name                                                              |

### Edge Types

| Edge Label        | From → To                                              | Key Properties                                                                                 |
| ----------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------- |
| INTERESTED_IN     | Person → InterestCategory                              | interest_strength, added_at                                                                    |
| MEMBER_OF         | Person → Organisation                                  | role, business_role, permissions, status, joined_at, can_manage_org, can_publish, can_transact |
| MEMBER_OF         | Person → Circle                                        | role, joined_at, status                                                                        |
| FOUNDED_BY        | Organisation → Person                                  |                                                                                                |
| EMPLOYED_BY       | Person → Organisation                                  | jobtitle, role_title, start_date, end_date                                                     |
| LOCATED_IN        | Organisation/Event/Place → Place/City/Province/Country |                                                                                                |
| CONTAINS          | Country → Province, Province → City, City → Place      |                                                                                                |
| PART_OF           | Place → Place (geographic hierarchy)                   | relation_type                                                                                  |
| CREATED           | Person → Content                                       | role (author, editor, contributor), datepublished                                              |
| FOLLOWS           | Person → Person/Organisation/Place                     | created_at                                                                                     |
| MESSAGES_WITH     | Person → Person (via Conversation)                     | last_message_at                                                                                |
| PARTICIPATES_IN   | Person → Conversation/Event/Circle                     | role, joined_at                                                                                |
| HAS_CREDENTIAL    | Person → Credential                                    | verification_level, verified_at                                                                |
| ISSUED_BY         | Credential → Organisation                              |                                                                                                |
| ALUMNI_OF         | Person → Organisation                                  |                                                                                                |
| AFFILIATED_WITH   | Person → Organisation                                  |                                                                                                |
| KNOWS_LANGUAGE    | Person → Language                                      | proficiency                                                                                    |
| HAS_OCCUPATION    | Person → Occupation                                    |                                                                                                |
| CATEGORIZED_AS    | Organisation/Content/Event → InterestCategory/Industry |                                                                                                |
| REVIEWED          | Person → Content/Organisation/Place                    | rating, headline, body                                                                         |
| CONTRIBUTED       | Person → Entity                                        | contribution_type (7 Ubuntu types), created_at                                                 |
| EARNED            | Person → Badge                                         | earned_at                                                                                      |
| CONFIRMED         | Person → Place                                         | confirmed_at                                                                                   |
| OWNS              | Person → Place                                         | verified, verified_at                                                                          |
| FAMILY_OF         | Person → Person                                        | relationship (parent, child, spouse, sibling, relatedTo)                                       |
| HAS_WALLET        | Person → WalletAddress                                 | blockchain, wallet_type, is_primary                                                            |
| VERIFIED_BY       | Organisation/Place/Person → Person                     | verification_tier, verified_at                                                                 |
| TRANSACTS_WITH    | Person → Person/Organisation                           | transaction_type, last_transaction_at                                                          |
| AUTHENTICATED_VIA | Person → OAuthProvider                                 | provider, connected_at                                                                         |

### Tri-Mode for the Knowledge Graph

**Mode 1 — Live Graph (Musha).** Real-time entity retrieval, relationship traversal, and context building. Powers profile pages, organisation pages, Shamwari context, search results. Served from ScyllaDB at sub-millisecond latency.

**Mode 2 — Intelligence Graph (Basa).** Graph algorithms (PageRank, community detection, influence propagation, recommendation features) run as scheduled Maestro workflows. Results feed into Feast (Layer 7) as pre-computed features for Ray's recommendation models. Ubuntu scores are computed from graph metrics, not SQL counts.

**Mode 3 — Heritage Graph (Nhaka).** When a person transitions to ancestral status (MIT Ancestral Lifecycle Protocol), their subgraph — all their vertices, all their edges, all their contributions — migrates to Cassandra as part of the heritage archive. PII is stripped by Flink. Anonymised graph patterns flow to Doris for the open data commons. The heritage graph preserves the network of relationships, contributions, and community connections that an ancestor built during their lifetime. Researchers can query how community structures evolved over decades by analysing heritage graph patterns.

---

## Changes Per Canonical Document

### THE MUKOKO ORDER v4

**Section 5 — The Data: Seven Layers of the Covenant Architecture**

Replace the Layer 2 row in the table:

**REMOVE:**

```
| 2. Relational | Supabase/PostgreSQL | "The platform is structured and trustworthy" | The platform and the community |
```

**INSERT:**

```
| 2. ACID Ledger | Supabase/PostgreSQL | "The platform's transactions are exact" | The platform |
```

The covenant changes from structural trustworthiness (which now belongs to the graph) to transactional exactness (which is what ACID guarantees). The stakeholder narrows from "the platform and the community" to "the platform" — because community data (relationships, contributions, interactions) now lives in the graph.

Update the Layer 3 row (additive to Layer 3 Amendment):

**REMOVE:**

```
| 3. Document | ScyllaDB (hot) + Cassandra (cold) + JanusGraph | "All content has a home" | The creator and the community |
```

**INSERT:**

```
| 3. Knowledge Graph | ScyllaDB (hot) + Cassandra (cold) + JanusGraph | "All knowledge has a home" | The creator, the community, and the intelligence |
```

The layer name changes from "Document" to "Knowledge Graph" — reflecting that it now holds not just content documents but the entire entity-relationship graph of the platform. The covenant expands from content having a home to all knowledge having a home — entities, relationships, context, and content. The stakeholder adds "the intelligence" — acknowledging that AI systems (Shamwari, the Analytical Agent, future AGI) are first-class consumers of this layer.

The mathematical argument: Layer 2 (now the ACID ledger) is the foundation (4 = foundation in the Mukoko Order — the ledger is the financial and governance foundation). Layer 3 (now the Knowledge Graph) is the covenant (7 = covenant — the graph embodies the platform's covenant with its users that their identity, relationships, and contributions are preserved, connected, and meaningful).

### MUKOKO ARCHITECTURE v4

**Section 5 — The Three Sources of Truth**

**REMOVE** the current Supabase/PostgreSQL 17 paragraph.

**INSERT:**

```
**Supabase/PostgreSQL 17** — the platform's ACID ledger. Financial transactions,
service bus event delivery, platform configuration, and RBAC enforcement. The narrow
set of operations that require serializable multi-row atomic transactions. Authentication
stubs for session validation and JWT minting. Schema.org compliance at the column level.
PostgREST generates REST API for ledger operations. Primary project:
`mukoko_platform_cloud` (`tdcpuzqyoodrdsxldgsh`, eu-central-1, PostgreSQL 17.6.1.084).
```

**UPDATE** the ScyllaDB + Apache Cassandra paragraph to add:

```
JanusGraph is the platform's primary data engine for all entities, relationships, profiles,
preferences, and contextual data. Every person, every organisation, every place, every
piece of content, every community is a vertex. Every connection between them — membership,
authorship, location, interest, credential, family, employment — is an edge with typed
properties. The entire platform is a single traversable knowledge graph. Shamwari builds
context through Gremlin traversals, not SQL JOINs. Ubuntu scores are computed through
graph algorithms, not aggregate queries. Recommendations are powered by graph features,
not table scans. The graph-first paradigm is the platform's native data representation
for the AI era.
```

**Section 6 — The Seven Data Layers, Layer 2**

**REMOVE** the current Layer 2 description.

**INSERT:**

```
### Layer 2 — The ACID Ledger (Supabase/PostgreSQL 17)

**Covenant:** "The platform's transactions are exact."
**Stakeholder:** The platform.

The platform's ACID ledger. PostgreSQL does what it was born to do — precise,
transactional, ACID-guaranteed ledger operations — and nothing else. Five functional
domains: (1) authentication stubs — minimal `identity.person` records for session
validation and JWT minting, (2) financial ledger — the wallet schema (transfers,
balances, payment intents, emissions, legacy transfers), (3) service bus — transactional
event publishing and subscription management, (4) platform configuration — verification
tiers, permission definitions, themes, badge definitions, mission definitions, interest
category definitions, (5) RBAC enforcement — platform permissions and role-permission
mappings. Schema.org compliance on all tables. PostgREST generates REST API. Row Level
Security for access control.

Entity data, relationship data, profile data, preference data, and contextual data do
not live here. They live in Layer 3's knowledge graph. The ACID ledger is the minimum
viable relational database — small, narrow, and precisely scoped to the operations that
genuinely require serializable transactions.
```

**Section 6 — The Seven Data Layers, Layer 3**

Update the Layer 3 title and description (additive to Layer 3 Amendment):

**REMOVE** the title "Layer 3 — The Document Layer"

**INSERT:** "Layer 3 — The Knowledge Graph (ScyllaDB + Cassandra + JanusGraph)"

Add to the Layer 3 description:

```
JanusGraph is the platform's primary data engine. All entities (people, organisations,
places, content, communities, interest categories, credentials, languages, occupations)
are vertices. All relationships (membership, employment, authorship, interest, location,
family, credentials, follows, transactions, contributions) are edges with typed
properties. The platform's data model is a property graph — the same data structure
that mirrors human cognition, that AI systems reason over natively, and that scales
to a billion interconnected users without the multi-table JOIN complexity that
constrains relational databases.

The graph has three modes. Live Graph (Musha): real-time entity retrieval, relationship
traversal, context building. Intelligence Graph (Basa): scheduled graph algorithms
(PageRank, community detection, influence propagation) feeding Feast and Ray in Layer 7.
Heritage Graph (Nhaka): ancestral subgraph preservation in Cassandra, PII-stripped
patterns flowing to Doris.
```

**Section 11 — What Is Built vs. What Is Designed**

**INSERT** into "Designed, Not Yet Built":

```
Graph-first migration: identity.person column reduction, business.organization column
reduction, identity relationship tables → JanusGraph edges, business relationship
tables → JanusGraph edges, engagement tables → JanusGraph edges, ubuntu contribution
graph, Graph API (FastAPI wrapping Gremlin), revised authentication flow with
authentication stub + Person vertex pattern. Stale column cleanup (D1 references,
ceramic_did, couchdb_doc_id references). Places progressive migration to JanusGraph
geographic knowledge graph.
```

### MUKOKO MANIFESTO v4

**Section 05 — Open Source & Sovereign**

**INSERT** after the ScyllaDB/Cassandra sovereignty paragraph:

```
The relational database was the paradigm of the past — a ledger designed in 1970 for
accounting and business transactions. Every major platform built on relational
databases has spent billions migrating toward graph-based systems because the relational
model cannot represent how the world is actually connected. Mukoko does not inherit
that technical debt. JanusGraph — Apache 2.0, governed by the Linux Foundation — is the
platform's primary data engine. Every person, every business, every place, every
connection between them lives in a knowledge graph that AI systems reason over natively.
The relational database remains for what it does best: exact financial transactions.
Nothing more. Frontier infrastructure does not play catch-up.
```

**Covenant Two — The Structure**

The second covenant corresponds to Layer 2. Update to reflect the ACID ledger reduction:

```
*The Structure:* Your transactions are exact. When you send money, when you receive
a payment, when the platform processes your contribution — these operations are
precise, atomic, and guaranteed. The ledger does not approximate. It does not
eventually-consistent your wallet balance. It is exact, because financial trust
requires exactness.
```

**Covenant Three — The Home**

The third covenant corresponds to Layer 3. Expand to include the knowledge graph:

```
*The Home:* All knowledge has a home. Your identity — who you are, who you're connected
to, what you care about, what you've built — is not a row in a table. It is a living
graph of relationships that grows as you grow. Your contributions are edges that
strengthen the community. Your connections are paths that others can discover. Active
knowledge lives where it is served instantly. Heritage knowledge lives where it is
preserved permanently. A message you sent this morning, a mentor-student relationship
you built over years, and a novel your grandmother published two decades ago — all have
homes, all are connected, all are part of the graph that makes the platform intelligent.
```

---

## What This Preserves

The seven data layers remain seven. The three sources of truth remain three — the boundaries shift (Supabase narrows, JanusGraph expands) but the count is unchanged. The locked counts (17 mini-apps, 7 Nyuchi products, 7 data layers, 7 covenants, 40 interest categories) are unaffected. The mathematical order is intact.

The Schema.org compliance requirement is preserved — JanusGraph vertex labels correspond to Schema.org types (Person, Organisation, Place, CreativeWork), and vertex properties use Schema.org property names. The compliance moves from PostgreSQL column naming to JanusGraph property naming. The standard is the same; the storage engine changes.

The tri-mode principle is preserved and extended — the knowledge graph operates in Musha (live graph), Basa (intelligence graph), and Nhaka (heritage graph) simultaneously.

The MIT Ancestral Lifecycle Protocol is strengthened — an ancestor's subgraph (their vertices, edges, contributions, relationships) migrates to Cassandra as a coherent graph structure, not as disconnected table rows. The heritage graph preserves the shape of a person's life in the community.

The sovereignty model is preserved — JanusGraph is Apache 2.0, ScyllaDB is the hot tier (speed rented), Cassandra is the cold tier (sovereignty owned). The graph-first paradigm adds no new sovereignty risks.

---

## What This Changes

The philosophical position of Supabase/PostgreSQL in the architecture shifts from "the platform's relational source of truth" (implying primacy) to "the platform's ACID ledger" (implying a specialised, narrow role). This is the most significant architectural change since the platform's founding. It declares that the relational paradigm — the dominant data model of the last 55 years — is not the foundation of Mukoko's data architecture. The graph is.

This is a frontier decision. It positions Mukoko ahead of platforms that started relational and are still migrating. It gives AI systems — current and future — a native data structure to reason over. It makes the platform's data model mirror the structure of human cognition. And it is irreversible in the best sense: once the graph is the primary data engine, no one will propose going back to tables.

---

## Migration Strategy

The migration is progressive, not big-bang. The order of operations:

**Phase 1 — Graph Schema.** Deploy JanusGraph vertex and edge schema on ScyllaDB. Define all vertex labels, edge labels, property keys, and composite indexes. This is schema work, not data migration.

**Phase 2 — Dual Write.** FastAPI writes to both Supabase and JanusGraph for all affected tables. JanusGraph is populated from existing Supabase data via a backfill Maestro workflow. Both systems hold the same data. Reads still come from Supabase.

**Phase 3 — Graph Read.** Profile reads, relationship queries, and context-building queries shift to JanusGraph. Supabase remains the write master. The Graph API handles reads; PostgREST handles writes.

**Phase 4 — Graph Primary.** JanusGraph becomes the write master for all entity and relationship data. Supabase receives a reduced sync (authentication stub only) via the service bus. PostgREST is used only for ledger operations (wallet, service bus, config).

**Phase 5 — Supabase Reduction.** Drop the migrated tables from Supabase. Clean up stale columns (D1 references, couchdb_doc_id, ceramic_did). The `identity.person` table shrinks to the authentication stub. The `business.organization` table either shrinks to governance columns or is dropped entirely (if governance checks move to JanusGraph with `LOCAL_QUORUM`).

**Phase 6 — Stale Column Cleanup.** Independent of the graph migration, remove all dead references from `identity.person`: `d1_person_id`, `d1_synced_at`, `sync_version`, `personal_d1_database_id`, `personal_d1_database_name`, `ceramic_did`, and all `couchdb_doc_id` / `couchdb_model_id` references across schemas. These are artifacts of previous architectural eras.

Each phase is independently deployable. Each phase can be rolled back. The platform is never in a state where data exists in only one system until Phase 5, which only executes after Phase 4 has been validated in production.

---

_Graph-First Architectural Amendment — April 2026_
_Drafted for Bryan Fawcett_
_Nyuchi Africa / The Bundu Family_

_"The relational database was the best tool we had. The graph database is the tool we need. Frontier infrastructure does not inherit the constraints of the previous era."_
