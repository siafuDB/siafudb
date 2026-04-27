// siafudb-access — Convenience access patterns on top of the graph.
//
// These modules let developers use SiafuDB the way they'd use SQLite,
// MongoDB, or Redis — without thinking about graphs. Everything is
// graph underneath, which means all data participates in relationships,
// syncs through the protocol, and is queryable with Cypher/GQL.
//
// But the developer doesn't need to know that. They just store documents,
// set keys, or log time-series data, and it works.

pub mod document;
pub mod kv;
pub mod timeseries;
