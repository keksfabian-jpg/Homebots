//! Dev: terrain grass layers, billboard sets and instance counts.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let g = p.terrain_grass().unwrap();
    println!("bounds {:?} color {}", g.bounds, p.object_path(g.color_texture));
    for l in &g.layers {
        println!("{}: {} instances, first {:?}", l.name, l.instances.len(), l.instances.first());
        for b in &l.billboards { println!("   {} {:?}", p.object_path(b.texture), b); }
    }
}
