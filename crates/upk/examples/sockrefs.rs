//! Dev: every socket Reference entry (all tagged fields, also entries without a Reference
//! object) of the skeletal meshes with this object name. levelcars.rs: the jiggle cars'
//! damage thresholds are such entries (GCInteractiveObjectInfo init at HOMEFRONT.exe 0xd64cf0).
//! Usage: sockrefs <package> <mesh name>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, _) in p.exports.iter().enumerate().filter(|(_, e)| p.object_name_of(e).eq_ignore_ascii_case(&a[2]) && p.export_class(e) == "SkeletalMesh") {
        println!("== #{} {}", i + 1, p.object_path(i as i32 + 1));
        let Some(pr) = p.export_props(i) else { continue };
        let Some(upk::props::Value::Raw { data, .. }) = pr.get("Sockets") else { continue };
        let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        for k in 0..n {
            let o = i32::from_le_bytes(data[4 + k * 4..8 + k * 4].try_into().unwrap());
            if o <= 0 {
                continue;
            }
            let Some(sp) = p.export_props(o as usize - 1) else { continue };
            println!("socket {} bone {}", sp.enum_name("SocketName").unwrap_or(""), sp.enum_name("BoneName").unwrap_or(""));
            for r in p.struct_array(&sp, "References") {
                let f: Vec<String> = r
                    .0
                    .iter()
                    .map(|x| {
                        let v = match &x.value {
                            upk::props::Value::Object(o) if *o != 0 => p.object_path(*o),
                            upk::props::Value::Raw { type_name, data, .. } => match p.read_props(&mut upk::props::Rd::new(data)) {
                                Some(sub) => format!("{{{}}}", sub.0.iter().map(|y| format!("{}={:?}", y.name, y.value)).collect::<Vec<_>>().join(" ")),
                                None => format!("{type_name}[{}]", data.len()),
                            },
                            v => format!("{v:?}"),
                        };
                        format!("{}={}", x.name, v)
                    })
                    .collect();
                println!("   {}", f.join(" "));
            }
        }
    }
}
