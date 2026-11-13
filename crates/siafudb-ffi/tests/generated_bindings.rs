//! Grep-level check that the committed generated bindings expose the
//! expected public symbols. Regenerate with `scripts/ffi/generate-bindings.sh`.

use std::fs;
use std::path::PathBuf;

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../bindings")
        .join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn assert_has(src: &str, file: &str, needles: &[&str]) {
    // Generated code backtick-escapes identifiers that may be keywords.
    let src = src.replace('`', "");
    for n in needles {
        assert!(src.contains(n), "{file} is missing `{n}`");
    }
}

#[test]
fn kotlin_exposes_public_api() {
    let f = "kotlin/src/main/kotlin/uniffi/siafudb_ffi/siafudb_ffi.kt";
    assert_has(
        &read(f),
        f,
        &[
            "open class Database",
            "fun inMemory()",
            "fun open(",
            "fun fromSnapshot(",
            "fun exportSnapshot()",
            "fun execute(",
            "fun query(",
            "fun kvGet(",
            "fun kvSet(",
            "fun kvDelete(",
            "fun changesSince(",
            "fun setMutationTracking(",
            "data class Rows",
            "data class Change",
            "sealed class Value",
            "object Null",
            "class Bool",
            "class Int",
            "class Float",
            "class Text",
            "class Bytes",
            "class List",
            "sealed class DbException",
            "class Locked",
            "class Corrupt",
            "class Query",
            "class InvalidArgument",
            "class Engine",
        ],
    );
}

#[test]
fn swift_exposes_public_api() {
    let f = "swift/Sources/SiafuDB/siafudb_ffi.swift";
    assert_has(
        &read(f),
        f,
        &[
            "open class Database",
            "func inMemory()",
            "func open(path:",
            "func fromSnapshot(",
            "func exportSnapshot()",
            "func execute(query:",
            "func query(query:",
            "func kvGet(key:",
            "func kvSet(key:",
            "func kvDelete(key:",
            "func changesSince(seq:",
            "func setMutationTracking(",
            "struct Rows",
            "struct Change",
            "enum Value",
            "case null",
            "case bool(",
            "case int(",
            "case float(",
            "case text(",
            "case bytes(",
            "case list(",
            "enum DbError",
            "case Locked(",
            "case Corrupt(",
            "case Query(",
            "case InvalidArgument(",
            "case Engine(",
        ],
    );
}
