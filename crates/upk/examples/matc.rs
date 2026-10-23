//! Dev: compile a material (by object name) to WGSL and print it.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2]).unwrap() as i32 + 1;
    let c = p.compile_material(i).unwrap();
    println!("{}", c.code);
    println!("// textures: {:?}", c.textures.iter().map(|t| (p.object_path(t.object), t.cube, t.address)).collect::<Vec<_>>());
    println!("// lighting {} blend {:?} stubbed {:?}", c.lighting, c.info.blend, c.stubbed);
}
