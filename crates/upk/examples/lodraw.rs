//! Dev: StaticMesh LOD headers (2 dwords, sections, 8 dwords).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "StaticMesh" || (a.len() > 2 && p.object_name_of(e) != a[2]) { continue; }
        let mut r = p.export_reader(i);
        p.read_props(&mut r).unwrap();
        r.u32(); r.vec3(); r.vec3(); r.f32(); r.i32(); r.i32();
        let nn = r.i32().unwrap(); r.bytes(nn as usize * 8);
        let nl = r.i32().unwrap();
        let mut line = format!("{} size {} lods {nl}:", p.object_name_of(e), e.serial_size);
        for _ in 0..nl {
            let a0 = r.u32().unwrap(); let a1 = r.u32().unwrap();
            let ns = r.i32().unwrap();
            for _ in 0..ns { r.bytes(36); let nf = r.i32().unwrap(); r.bytes(nf as usize * 8); }
            let d: Vec<u32> = (0..8).map(|_| r.u32().unwrap()).collect();
            line += &format!(" [{a0} {a1} s{ns} {:?}]", d);
        }
        println!("{line}");
    }
}
