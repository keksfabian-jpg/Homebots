//! Dev: scalar / vector / texture parameter expressions of a material (by object name).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        let cl = p.export_class(e);
        if !cl.contains("Parameter") { continue; }
        let path = p.object_path(i as i32 + 1);
        if !path.contains(&a[2]) { continue; }
        let pr = p.export_props(i).unwrap();
        println!("#{} {} [{}] name={:?} default={:?}", i + 1, path, cl, pr.enum_name("ParameterName"), pr.float("DefaultValue"));
    }
}
