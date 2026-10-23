//! Dev: per TerrainComponent, print section params and native array counts.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "TerrainComponent" { continue; }
        let mut r = p.export_reader(i);
        let pr = p.read_props(&mut r).unwrap();
        let g = |n: &str| pr.int(n).map(|v| v.to_string()).unwrap_or("-".into());
        let (h0, h1, n) = (r.i32().unwrap(), r.i32().unwrap(), r.i32().unwrap());
        println!("base {:>5},{:<5} size {:>3}x{:<3} tess {:>2} stride {:>2} | {} {} count {} | bytes {}",
            g("SectionBaseX"), g("SectionBaseY"), g("SectionSizeX"), g("SectionSizeY"), g("MaxTessellationLevel"), g("TessellationStride"), h0, h1, n, e.serial_size);
    }
}
