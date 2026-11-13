use siafudb_core::SiafuDB;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! { static DB: RefCell<Option<SiafuDB>> = const { RefCell::new(None) }; }
fn with<R>(f: impl FnOnce(&SiafuDB) -> Result<R, String>) -> Result<R, JsError> {
    DB.with(|d| match d.borrow().as_ref() {
        Some(db) => f(db).map_err(|e| JsError::new(&e)),
        None => Err(JsError::new("no db")),
    })
}
#[wasm_bindgen]
pub fn init() -> Result<(), JsError> {
    let db = SiafuDB::in_memory().map_err(|e| JsError::new(&e.to_string()))?;
    DB.with(|d| *d.borrow_mut() = Some(db));
    Ok(())
}
#[wasm_bindgen]
pub fn exec(q: &str) -> Result<usize, JsError> {
    with(|db| db.execute(q).map(|r| r.rows_affected).map_err(|e| e.to_string()))
}
/// Returns JSON {columns, rows}.
#[wasm_bindgen]
pub fn query(q: &str) -> Result<String, JsError> {
    with(|db| {
        let r = db.query(q).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({"columns": r.columns, "rows": r.rows}).to_string())
    })
}
#[wasm_bindgen]
pub fn export_snapshot() -> Result<Vec<u8>, JsError> {
    with(|db| db.export_snapshot().map_err(|e| e.to_string()))
}
#[wasm_bindgen]
pub fn load_snapshot(b: &[u8]) -> Result<(), JsError> {
    let db = SiafuDB::from_snapshot(b).map_err(|e| JsError::new(&e.to_string()))?;
    DB.with(|d| *d.borrow_mut() = Some(db));
    Ok(())
}
/// Insert n nodes with 3 props each.
#[wasm_bindgen]
pub fn bulk_insert(n: u32) -> Result<(), JsError> {
    with(|db| {
        for i in 0..n {
            db.execute(&format!("CREATE (:Item {{id: {i}, name: 'item-{i}', score: {}}})", i % 97))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })
}
