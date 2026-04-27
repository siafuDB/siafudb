// siafudb-access/src/kv/mod.rs
//
// KEY-VALUE ACCESS PATTERN
//
// Fast get/set operations for simple key-value storage.
// Each KV pair is a graph node: (:_KV {key: "...", value: "..."})
//
// This is how SiafuDB replaces Redis-style caching and SQLite's
// simple key-value usage patterns. Session tokens, feature flags,
// user preferences, cached API responses — all stored as graph nodes,
// all participating in sync, all queryable with Cypher if needed.
//
// TODO: Phase 1 implementation
// - kv_set(key, value) → MERGE on :_KV node
// - kv_get(key) → MATCH on :_KV node
// - kv_delete(key) → DELETE :_KV node
// - kv_list(prefix) → MATCH with STARTS WITH
// - TTL support (auto-expire keys)
