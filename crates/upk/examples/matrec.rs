//! Dev: shader cache records of a material/MIC (by object name): resource ids found in its
//! native data, their friendly names, vertex factories and uniform expressions.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[2])).unwrap();
    let (i, e) = p.exports.iter().enumerate().find(|(_, e)| p.object_name_of(e) == a[3]).unwrap();
    let mut r = p.export_reader(i);
    p.read_props(&mut r).unwrap();
    let b = p.export_bytes(e);
    let nat = &b[r.pos..];
    let verbose = a.len() > 4;
    for o in 0..nat.len().saturating_sub(16) {
        let mut id = [0u8; 16];
        id.copy_from_slice(&nat[o..o + 16]);
        if !sc.is_record(&id) { continue; }
        let rec = sc.record(&id).unwrap();
        println!("@{o} {} {}", rec.friendly_name, id.iter().map(|x| format!("{x:02x}")).collect::<String>());
        for m in &rec.maps {
            let types: Vec<&str> = m.shaders.iter().map(|(t, _)| t.as_str()).filter(|t| verbose || t.contains("BasePass")).collect();
            println!("   {} : {}", m.vertex_factory, types.join(" "));
        }
        if let Some((u, _)) = p.read_uniform_set(nat, o) {
            for (k, v) in u.vectors.iter().enumerate() { println!("   V{k} {v:?}"); }
            for (k, v) in u.scalars.iter().enumerate() { println!("   S{k} {v:?}"); }
            for (k, v) in u.tex2d.iter().enumerate() { let (n, ob) = v.texture().unwrap(); println!("   T{k} {n:?} {}", p.object_path(ob)); }
            for (k, v) in u.cubes.iter().enumerate() { let (n, ob) = v.texture().unwrap(); println!("   C{k} {n:?} {}", p.object_path(ob)); }
        }
    }
}
