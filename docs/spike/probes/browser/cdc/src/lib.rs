use grafeo::GrafeoDB;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn cdc_probe() -> Result<String, JsError> {
    let e = |x: grafeo::Error| JsError::new(&x.to_string());
    let db = GrafeoDB::new_in_memory();
    let before = db.is_cdc_enabled();
    db.set_cdc_enabled(true);
    for q in ["CREATE (:P {name:'a', n:1})", "CREATE (:P {name:'b', n:2})", "CREATE (:P {name:'c', n:3})"] {
        db.execute(q).map_err(e)?;
    }
    let latest = db.current_epoch();
    let ev = db.changes_between(0.into(), latest).map_err(e)?;
    let json: Vec<String> = ev
        .iter()
        .map(|x| serde_json::to_string(x).unwrap_or_else(|_| format!("{x:?}")))
        .collect();
    Ok(format!(
        "{{\"enabled_before\":{before},\"latest_epoch\":\"{:?}\",\"count\":{},\"events\":[{}]}}",
        latest,
        ev.len(),
        json.join(",")
    ))
}
