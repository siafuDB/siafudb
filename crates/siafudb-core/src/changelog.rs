// Copyright (C) 2026 The Bundu Foundation
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// siafudb-core/src/changelog.rs
//
// Engine-level change log.
//
// Every successful `SiafuDB::execute()` appends one entry while mutation
// tracking is on. The entries are intentionally lean — query text plus
// what the engine reports affected — so the sync protocol layer can
// translate them into richer Mutation / MutationBatch structures.
//
// The log is the bridge between the engine (which knows what was written)
// and the sync layer (which knows where it needs to go). It does not
// itself decide identity, transformation, or transport.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// One captured engine-level change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeLogEntry {
    /// Monotonic sequence number; starts at 1 within a database.
    pub sequence: u64,
    /// Unix timestamp in milliseconds when the change was captured.
    pub timestamp_ms: u64,
    /// The raw query string executed.
    pub query: String,
    /// How many graph elements the engine reported affected.
    pub rows_affected: usize,
}

/// Append-only log of engine-level changes.
#[derive(Debug, Default)]
pub struct ChangeLog {
    entries: Vec<ChangeLogEntry>,
    next_sequence: u64,
}

impl ChangeLog {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_sequence: 1,
        }
    }

    /// Append a new entry and return its sequence number.
    pub fn append(&mut self, query: String, rows_affected: usize) -> u64 {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let sequence = self.next_sequence;
        self.entries.push(ChangeLogEntry {
            sequence,
            timestamp_ms,
            query,
            rows_affected,
        });
        self.next_sequence += 1;
        sequence
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clone of every entry currently held.
    pub fn snapshot(&self) -> Vec<ChangeLogEntry> {
        self.entries.clone()
    }

    /// Clone of every entry with `sequence > since`.
    pub fn since(&self, since: u64) -> Vec<ChangeLogEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.sequence > since)
            .cloned()
            .collect()
    }
}

/// Thread-safe handle to a [`ChangeLog`] — what the engine and sync layer share.
pub type SharedChangeLog = Arc<Mutex<ChangeLog>>;
