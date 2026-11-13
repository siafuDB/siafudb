use wasm_bindgen::prelude::*;
use grafeo::GrafeoDB;

#[wasm_bindgen]
pub fn graph_roundtrip() -> String {
    let db = GrafeoDB::new_in_memory();
    db.execute("CREATE (:Person {name:'Amara'})").map_err(|e| e.to_string()).unwrap();
    db.execute("CREATE (:Person {name:'Tatenda'})").unwrap();
    let r = db.execute("MATCH (p:Person) RETURN p.name").unwrap();
    format!("rows={}", r.rows().len())
}
#[wasm_bindgen]
pub fn snapshot_roundtrip() -> String {
    let db = GrafeoDB::new_in_memory();
    db.execute("CREATE (:Person {name:'Amara'})").unwrap();
    let bytes = db.export_snapshot().unwrap();
    let db2 = GrafeoDB::import_snapshot(&bytes).unwrap();
    let r = db2.execute("MATCH (p:Person) RETURN p.name").unwrap();
    format!("snapshot_bytes={} rows_after_import={}", bytes.len(), r.rows().len())
}
#[wasm_bindgen]
pub fn uuid_v4() -> String { uuid::Uuid::new_v4().to_string() }
#[wasm_bindgen]
pub fn systemtime() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}", t.as_millis())
}
#[wasm_bindgen]
pub fn sha256_hex() -> String { use sha2::{Digest,Sha256}; Sha256::digest(b"siafu").iter().map(|b| format!("{b:02x}")).collect::<String>() }
#[wasm_bindgen]
pub fn siafu_execute() -> String {
    let db = siafudb_core::SiafuDB::in_memory().unwrap();
    match db.execute("CREATE (:Person {name:'Amara'})") { Ok(r)=>format!("ok rows={}", r.rows_affected), Err(e)=>format!("err {e}") }
}
