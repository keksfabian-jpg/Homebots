//! Dev: list DepthBiasedAlpha expressions of a package with their properties.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "MaterialExpressionDepthBiasedAlpha" { continue; }
        let mut r = p.export_reader(i);
        let Some(pr) = p.read_props(&mut r) else { continue };
        println!("{} : {:?}", p.object_path(i as i32 + 1), pr.0.iter().map(|x| (x.name.clone(), format!("{:?}", x.value).chars().take(60).collect::<String>())).collect::<Vec<_>>());
    }
}
