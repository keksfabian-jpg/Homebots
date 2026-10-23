//! Dev: GameMode_PlayerStart groups / locations / other props of a map.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let want = a.get(2).cloned();
    let mut groups = std::collections::BTreeMap::new();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "GameMode_PlayerStart" { continue; }
        let pr = p.export_props(i).unwrap();
        let g = match pr.get("Group") { Some(upk::props::Value::Name(n)) => n.clone(), _ => String::new() };
        *groups.entry(g.clone()).or_insert(0) += 1;
        if want.as_ref().is_some_and(|w| &g == w) {
            let keys: Vec<String> = pr.0.iter().filter(|q| !["Base","CylinderComponent","Components","Tag","DrawScale"].contains(&q.name.as_str())).map(|q| format!("{}={:?}", q.name, q.value)).collect();
            println!("{} {}", p.object_path(i as i32 + 1), keys.join(" ").chars().take(500).collect::<String>());
        }
    }
    println!("{groups:?}");
}
