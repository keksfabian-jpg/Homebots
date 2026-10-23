//! Dev: actor + component properties of static mesh placements whose mesh name contains a substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        let cls = p.export_class(e);
        if !cls.contains("StaticMeshComponent") { continue; }
        let Some(cp) = p.export_props(i) else { continue };
        let Some(m) = cp.object("StaticMesh") else { continue };
        if m == 0 || !p.object_name(m).contains(&a[2]) { continue; }
        println!("#{} {} [{cls}]", i + 1, p.object_path(i as i32 + 1));
        for x in &cp.0 { println!("   {} = {:?}", x.name, match &x.value { upk::props::Value::Raw{type_name, ..} => format!("raw {type_name}"), v => format!("{v:?}") }); }
        let outer = e.outer_index;
        if outer > 0 {
            println!("  outer {} [{}]", p.object_path(outer), p.export_class(&p.exports[outer as usize - 1]));
            if let Some(ap) = p.export_props(outer as usize - 1) { for x in &ap.0 { println!("   {} = {:?}", x.name, match &x.value { upk::props::Value::Raw{type_name, ..} => format!("raw {type_name}"), v => format!("{v:?}") }); } }
        }
    }
}
