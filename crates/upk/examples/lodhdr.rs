//! Dev: print each LOD header's trailing 8 dwords (after sections) and section summary.
use upk::props::Rd;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for name in &a[2..] {
        let i = p.exports.iter().position(|e| p.export_class(e) == "StaticMesh" && &p.object_name_of(e) == name).unwrap();
        let mut r = p.export_reader(i);
        p.read_props(&mut r).unwrap();
        let _ = (r.u32(), r.vec3(), r.vec3(), r.f32(), r.i32(), r.i32());
        let nn = r.i32().unwrap(); r.bytes(nn as usize * 8);
        let nl = r.i32().unwrap();
        println!("{name}: {nl} LODs, size {}", r.b.len());
        for l in 0..nl {
            let (a0, a1) = (r.u32().unwrap(), r.u32().unwrap());
            let ns = r.i32().unwrap();
            let mut tris = 0;
            for _ in 0..ns { let v: Vec<u32> = (0..9).map(|_| r.u32().unwrap()).collect(); tris += v[5]; let nf = r.i32().unwrap(); r.bytes(nf as usize * 8); }
            let t: Vec<u32> = (0..8).map(|_| r.u32().unwrap()).collect();
            println!("  LOD{l}: hdr ({a0}, {a1:#x}) sections {ns} tris {tris} | tail {t:?} | ends @{}", r.pos);
        }
        let _ = Rd::new(&[]);
    }
}
