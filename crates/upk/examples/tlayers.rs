//! Dev: dump Terrain layers -> setups -> terrain materials, and weightmap texture info.
use upk::props::{Props, Rd, Value};
fn show(p: &upk::Package, pr: &Props, pad: &str) {
    for q in &pr.0 {
        let v = match &q.value {
            Value::Object(o) => format!("#{o} {} [{}]", p.object_path(*o), if *o > 0 { p.export_class(&p.exports[*o as usize - 1]) } else { "import".into() }),
            Value::Raw { struct_name: Some(sn), data, .. } => format!("{sn} {:?}", p.read_props(&mut Rd::new(data)).map(|x| x.0.iter().map(|y| format!("{}={:?}", y.name, y.value)).collect::<Vec<_>>())),
            Value::Raw { type_name, data, .. } => format!("{type_name} {} bytes", data.len()),
            v => format!("{v:?}"),
        };
        println!("{pad}{} = {}", q.name, v);
    }
}
fn props(p: &upk::Package, i: i32) -> Props { let mut r = p.export_reader(i as usize - 1); p.read_props(&mut r).unwrap_or_default() }
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let t = p.exports.iter().position(|e| p.export_class(e) == "Terrain").unwrap() as i32 + 1;
    let tp = props(&p, t);
    for (li, l) in p.array_structs(tp.get("Layers").unwrap()).iter().enumerate() {
        println!("Layer {li}:"); show(&p, l, "  ");
        if let Some(s) = l.object("Setup").filter(|s| *s > 0) {
            for m in p.array_structs(props(&p, s).get("Materials").unwrap()) {
                println!("    setup material:"); show(&p, &m, "      ");
                if let Some(tm) = m.object("Material").filter(|x| *x > 0) { println!("      TerrainMaterial #{tm}:"); show(&p, &props(&p, tm), "        "); }
            }
        }
    }
    for (i, e) in p.exports.iter().enumerate() {
        let c = p.export_class(e);
        if c == "TerrainWeightMapTexture" || c == "TerrainWeightMapVolume" {
            println!("{c} #{} {} size {}", i + 1, p.object_path(i as i32 + 1), e.serial_size);
            show(&p, &props(&p, i as i32 + 1), "  ");
        }
    }
}
