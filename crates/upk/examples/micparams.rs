//! Dev: print a MIC's scalar/vector/texture parameter overrides.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2]).unwrap();
    let mut r = p.export_reader(i);
    let pr = p.read_props(&mut r).unwrap();
    for k in ["ScalarParameterValues", "VectorParameterValues", "TextureParameterValues"] {
        if let Some(v) = pr.get(k) {
            for s in p.array_structs(v) {
                let val = s.get("ParameterValue").map(|v| match v { upk::props::Value::Object(o) => format!("#{o} {} [{}]", p.object_path(*o), if *o > 0 { p.export_class(&p.exports[*o as usize - 1]) } else { "import".into() }), v => format!("{v:?}") });
                println!("{k}: {:?} = {:?}", s.enum_name("ParameterName"), val);
            }
        }
    }
}
