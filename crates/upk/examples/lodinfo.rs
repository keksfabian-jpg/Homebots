//! Dev: SkeletalMesh LODInfo structs (LODMaterialMap etc.).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "SkeletalMesh").unwrap();
    let pr = p.export_own_props(i).unwrap();
    for (k, s) in p.struct_array(&pr, "LODInfo").iter().enumerate() {
        println!("LOD {k}:");
        for x in &s.0 { println!("  {} = {:?}", x.name, x.value); }
    }
}
