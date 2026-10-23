//! Dev: StaticMesh serialized header of one export (by name): LOD headers and bulk headers.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let Some(i) = p.exports.iter().position(|e| p.export_class(e) == "StaticMesh" && p.object_name_of(e) == a[2]) else { return };
    let mut r = p.export_reader(i);
    let _ = p.read_props(&mut r);
    println!("props end {} of {}", r.pos, p.exports[i].serial_size);
    let unk = r.u32();
    let o = r.vec3();
    let ex = r.vec3();
    let rad = r.f32();
    let bs = r.i32();
    let iv = r.i32();
    let nn = r.i32().unwrap_or(0);
    println!("unk {unk:?} origin {o:?} extent {ex:?} radius {rad:?} body {bs:?} version {iv:?} names {nn}");
    r.bytes(nn.max(0) as usize * 8);
    let nl = r.i32().unwrap_or(0);
    println!("lods {nl} at {}", r.pos);
    for _ in 0..nl.clamp(0, 8) {
        let li = r.u32();
        let u = r.u32();
        let ns = r.i32().unwrap_or(0);
        println!(" lod {li:?} unk {u:?} sections {ns}");
        for _ in 0..ns.clamp(0, 64) {
            let v: Vec<u32> = (0..9).filter_map(|_| r.u32()).collect();
            let nf = r.i32().unwrap_or(0);
            println!("   sec {v:?} frags {nf}");
            r.bytes(nf.max(0) as usize * 8);
        }
        let h: Vec<u32> = (0..8).filter_map(|_| r.u32()).collect();
        println!("   hdr {h:?}");
    }
    for _ in 0..nl.clamp(0, 8) {
        let h: Vec<u32> = (0..4).filter_map(|_| r.u32()).collect();
        println!(" bulk {h:x?} at {}", r.pos);
        if h.first().is_some_and(|f| f & 1 == 0) {
            r.bytes(h[2] as usize);
        }
    }
    println!("end at {} of {}", r.pos, r.b.len());
}
