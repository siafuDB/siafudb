// siafudb-access/src/document/mod.rs
//
// DOCUMENT ACCESS PATTERN
//
// Store and query JSON documents in named collections.
// Each document becomes a graph node with the collection name as its label
// and the JSON properties as node properties.
//
// This is how SiafuDB replaces MongoDB-style document storage.
// The developer thinks "I'm storing a document." The database thinks
// "I'm creating a node in the graph." Both are correct.
//
// TODO: Phase 1 implementation
// - doc_insert(collection, json) → creates a labeled node
// - doc_find(collection, filter) → matches nodes by label and properties
// - doc_update(collection, filter, update) → updates matching nodes
// - doc_delete(collection, filter) → deletes matching nodes
// - Nested objects become related nodes (Phase 2)
