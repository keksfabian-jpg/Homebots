fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for pl in p.speedtree_placements().iter().take(8) { println!("{} {:?} {:?}", p.object_path(pl.tree), pl.location, pl.scale); }
}
