//! Dev: skeletal meshes whose LOD0 LODMaterialMap is not the identity.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "SkeletalMesh" { continue }
            let Some(pr) = p.export_own_props(i) else { continue };
            let lods = p.struct_array(&pr, "LODInfo");
            let Some(l0) = lods.first() else { continue };
            let map: Vec<i32> = match l0.get("LODMaterialMap") {
                Some(upk::props::Value::Raw { data, .. }) if data.len() >= 4 => data[4..].chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect(),
                _ => Vec::new(),
            };
            let ident = map.iter().enumerate().all(|(k, &v)| k as i32 == v);
            println!("{} {} map {:?}", if ident { "ID " } else { "NON" }, p.object_path(i as i32 + 1), map);
        }
    }
}
