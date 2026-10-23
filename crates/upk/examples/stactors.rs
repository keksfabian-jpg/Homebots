//! Dev: SpeedTreeActor placements: location, rotation, scale, component props.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let mut n = 0;
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "SpeedTreeActor" { continue; }
        let Some(ap) = p.export_props(i) else { continue };
        let comp = ap.object("SpeedTreeComponent").unwrap_or(0);
        let cp = if comp > 0 { p.export_props(comp as usize - 1) } else { None };
        let tree = cp.as_ref().and_then(|c| c.object("SpeedTree")).map(|t| p.object_path(t)).unwrap_or_default();
        if n < (a.get(2).and_then(|s| s.parse().ok()).unwrap_or(5)) {
            println!("{} loc {:?} rot {:?} ds {:?} ds3 {:?} tree {tree}", p.object_path(i as i32 + 1), ap.vector("Location"), ap.rotator("Rotation"), ap.float("DrawScale"), ap.vector("DrawScale3D"));
            if let Some(c) = &cp { for x in &c.0 { println!("     {} = {:?}", x.name, match &x.value { upk::props::Value::Raw{type_name, ..} => format!("raw {type_name}"), v => format!("{v:?}") }); } }
        }
        n += 1;
    }
    println!("total {n}");
}
