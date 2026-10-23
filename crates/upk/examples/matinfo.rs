//! Dev: a material's parent chain, params and compiled graph textures, by object name.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.object_name_of(e) == a[2]) {
        let o = i as i32 + 1;
        println!("#{o} {} [{}]", p.object_path(o), p.export_class(e));
        let mut cur = o;
        for _ in 0..6 {
            let Some(pr) = (if cur > 0 { p.export_props(cur as usize - 1) } else { None }) else { break };
            let par = pr.object("Parent").unwrap_or(0);
            println!("  parent of {} -> {} ({par})", p.object_path(cur), if par != 0 { p.object_path(par) } else { "-".into() });
            cur = par;
        }
        let mp = p.material_params(o);
        println!("  params {:?}", mp.textures.iter().map(|(n, t)| (n.clone(), p.object_path(*t))).collect::<Vec<_>>());
        match p.compile_material(o) { Some(c) => for t in &c.textures { println!("  graph tex {:?}", t) }, None => println!("  compile failed") }
    }
}
