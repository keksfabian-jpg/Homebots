//! Dev: render a top-down colour map of the terrain (layer bakes blended by the weightmap)
//! to visually check the terrain layer composition.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let lib = upk::library::Library::scan(&a[2]).unwrap();
    let tl = p.terrain_layers().unwrap();
    let mut cache = upk::material::TexCache::new();
    let lin = |b: u8| (b as f32 / 255.0).powf(2.2);
    let layers: Vec<Option<(usize, usize, Vec<[f32; 3]>, f32)>> = (0..4).map(|k| {
        let (m, s) = tl.layers[k]?;
        let bk = p.bake_material(&lib, m, 256, &mut cache)?;
        image::save_buffer(format!("{}/layer{k}.png", a[3]), &bk.rgba, bk.width, bk.height, image::ColorType::Rgba8).unwrap();
        Some((bk.width as usize, bk.height as usize, bk.rgba.chunks_exact(4).map(|c| [lin(c[0]), lin(c[1]), lin(c[2])]).collect(), s))
    }).collect();
    let n = 1024usize;
    let mut out = Vec::with_capacity(n * n * 3);
    for j in 0..n { for i in 0..n {
        let (u, v) = ((i as f32 + 0.5) / n as f32, (j as f32 + 0.5) / n as f32);
        let x = tl.bounds[0] + u * (tl.bounds[1] - tl.bounds[0]);
        let y = tl.bounds[2] + v * (tl.bounds[3] - tl.bounds[2]);
        let (px, py) = ((x - tl.location[0]) / tl.patch_size[0], (y - tl.location[1]) / tl.patch_size[1]);
        let k0 = ((v * (tl.weight_h - 1) as f32) as usize * tl.weight_w + (u * (tl.weight_w - 1) as f32) as usize) * 4;
        let (mut acc, mut ws) = ([0f32; 3], 0f32);
        for (k, l) in layers.iter().enumerate() {
            let Some((w, h, px_, s)) = l else { continue };
            let wt = tl.weights[k0 + upk::terrain::WEIGHT_CHANNEL[k]] as f32 / 255.0;
            let c = px_[(((py / s).rem_euclid(1.0) * *h as f32) as usize % h) * w + ((px / s).rem_euclid(1.0) * *w as f32) as usize % w];
            for q in 0..3 { acc[q] += c[q] * wt; } ws += wt;
        }
        for q in 0..3 { out.push(((acc[q] / ws.max(1e-6)).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8); }
    }}
    image::save_buffer(format!("{}/terrain_map.png", a[3]), &out, n as u32, n as u32, image::ColorType::Rgb8).unwrap();
    println!("layers {:?}", tl.layers);
}
