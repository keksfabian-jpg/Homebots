//! Dev: compile a MIC whose parent lives in another package (e.g. a vehicle *_DEST MIC with
//! MAT_LIB M_Vehicle_DEST) like world.rs build_material does; prints params and mat_eval.
//! Usage: wreckmat <mic package> <mic name> <parent package>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2]).unwrap() as i32 + 1;
    let (chain, parent) = p.mic_chain(i);
    println!("parent ref {parent} {}", p.object_path(parent));
    println!("scalar {:?}\nvector {:?}\ntex {:?}", chain.scalar, chain.vector, chain.tex.iter().map(|t| (t.0.clone(), p.object_path(t.1))).collect::<Vec<_>>());
    let bp = upk::Package::open(&a[3]).unwrap();
    let pname = p.object_name(parent);
    let bi = bp.exports.iter().position(|e| bp.object_name_of(e) == pname && bp.export_class(e) == "Material").unwrap() as i32 + 1;
    let c = bp.compile_material_ext(bi, &[], &chain).unwrap();
    if a.len() < 5 { println!("{}", c.code); }
    println!("// textures {:?}", c.textures.iter().map(|t| bp.object_path(t.object)).collect::<Vec<_>>());
    println!("// lighting {} blend {:?} stubbed {:?}", c.lighting, c.info.blend, c.stubbed);
}
