//! Dev: texel alignment of light-map CoordinateBias / CoordinateScale in the atlas (bias * size
//! and scale * size fractional parts) as serialized vs with bias x/y swapped. Args: packages.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (mut n, mut nat, mut swp) = (0, 0, 0);
    let near = |v: f32| { let f = v - v.floor(); (f - 0.5).abs() < 0.02 || f < 0.02 || f > 0.98 };
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for pl in p.static_mesh_placements() {
            for l in pl.lightmaps.iter().flatten() {
                let t = if l.directional() { l.textures[0] } else { l.textures[2] };
                if t <= 0 { continue; }
                let Some(pr) = p.export_props(t as usize - 1) else { continue };
                let (w, h) = (pr.int("SizeX").unwrap_or(0) as f32, pr.int("SizeY").unwrap_or(0) as f32);
                n += 1;
                if near(l.coord_bias[0] * w) && near(l.coord_bias[1] * h) && near(l.coord_scale[0] * w) && near(l.coord_scale[1] * h) { nat += 1; }
                if near(l.coord_bias[1] * w) && near(l.coord_bias[0] * h) { swp += 1; }
            }
        }
    }
    println!("light maps {n}: texel-aligned as serialized {nat}, with bias swapped {swp}");
}
