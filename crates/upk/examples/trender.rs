//! Dev: print the parsed TerrainRender data (resources with cache names and textures).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[2])).unwrap();
    let t = p.terrain_render(&|id| sc.is_record(id)).unwrap();
    println!("loc {:?} scale {:?} nv {:?} wmb {:?} sections {:?} weightmaps {:?} normal {:?}", t.location, t.scale, t.num_vertices, t.weight_map_bounds, t.sections, t.weight_maps, t.normal_map);
    println!("palettes {:?}", t.palette_names);
    for r in &t.resources {
        let name = sc.record(&r.id).map(|x| x.friendly_name).unwrap_or_default();
        println!("{name}");
        for (k, v) in r.uniforms.vectors.iter().enumerate() { println!("   V{k} {v:?} = {:?}", v.eval(0.0)); }
        for (k, v) in r.uniforms.scalars.iter().enumerate() { println!("   S{k} {v:?}"); }
        for (k, v) in r.uniforms.tex2d.iter().enumerate() {
            let (n, o) = v.texture().unwrap();
            println!("   T{k} {n:?} {}", p.object_path(o));
        }
    }
    let mut fl = std::collections::BTreeMap::new();
    for c in &t.components {
        for f in &c.quad_flags { *fl.entry(*f).or_insert(0) += 1; }
        if c.quad_flags.is_empty() { println!("comp {:?} no flags", c.base); }
    }
    println!("{} comps, flags {:?}", t.components.len(), fl);
    for (i, c) in t.components.iter().enumerate() {
        let x0 = t.location[0] + c.base[0] as f32 * t.scale[0];
        let y0 = t.location[1] + c.base[1] as f32 * t.scale[1];
        let hmin = *c.heights.iter().min().unwrap() as f32; let hmax = *c.heights.iter().max().unwrap() as f32;
        let z = |h: f32| t.location[2] + (h - 32768.0) / 128.0 * t.scale[2];
        println!("comp {i} base {:?} size {:?} tess {} x {x0}..{} y {y0}..{} z {}..{}", c.base, c.size, c.tess, x0 + c.size[0] as f32 * 32.0 * t.scale[0], y0 + c.size[1] as f32 * 32.0 * t.scale[1], z(hmin), z(hmax));
    }
}
// (component listing appended)
