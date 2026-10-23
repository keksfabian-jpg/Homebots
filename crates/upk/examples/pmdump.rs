//! Dev: DamageTypeInstigators of a PhysicalMaterialPropertyEx export (damage types, particle
//! systems, sound cues, every other member).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let pr = p.export_props(i).unwrap();
    for (k, st) in p.struct_array(&pr, "DamageTypeInstigators").into_iter().enumerate() {
        let dts: Vec<String> = st.get("DamageTypes").map(|v| p.array_objects(v)).unwrap_or_default().into_iter().map(|o| p.object_name(o)).collect();
        println!("[{k}] {dts:?}");
        if let Some(ce) = p.struct_prop(&st, "CollisionEffect") {
            for x in &ce.0 {
                match &x.value {
                    upk::props::Value::Raw { type_name, .. } if type_name == "ArrayProperty" => {
                        let objs: Vec<String> = p.array_objects(&x.value).into_iter().map(|o| if o != 0 { p.object_path(o) } else { "None".into() }).collect();
                        println!("    {} = {objs:?}", x.name);
                    }
                    v => println!("    {} = {v:?}", x.name),
                }
            }
        }
    }
}
