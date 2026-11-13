#![allow(warnings)] // SiafuDB fork: upstream warning-cleanliness is not ours to gate (see engine/CHANGES.md)
//! # grafeo-adapters
//!
//! The integration layer: parsers and plugins.
//!
//! This is where external formats meet Grafeo's internal representation.
//! You probably don't need this crate directly unless you're extending Grafeo.
//!
//! ## Modules
//!
//! - [`query`] - Parsers for GQL, Cypher, SPARQL, Gremlin, GraphQL
//! - [`plugins`] - Extension points for custom functions and algorithms

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod plugins;
pub mod query;
