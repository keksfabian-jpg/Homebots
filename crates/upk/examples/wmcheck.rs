//! Dev: verify weightmap channel/orientation by correlating each channel with terrain slope.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let wi = p.exports.iter().position(|e| p.export_class(e) == "TerrainWeightMapTexture").unwrap();
    let t = p.read_texture2d(wi).expect("weightmap parse");
    println!("weightmap {} {}x{} mips {} inline {:?}", t.format, t.width, t.height, t.mips.len(), t.best_mip().map(|m| (m.width, m.height)));
    let (w, h, px) = t.decode_rgba().unwrap();
    let ti = p.exports.iter().position(|e| p.export_class(e) == "Terrain").unwrap();
    let mut r = p.export_reader(ti);
    let tp = p.read_props(&mut r).unwrap();
    let (x0, x1, y0, y1) = (tp.float("TerrainBoundsMinX").unwrap(), tp.float("TerrainBoundsMaxX").unwrap(), tp.float("TerrainBoundsMinY").unwrap(), tp.float("TerrainBoundsMaxY").unwrap());
    let ter = &p.terrains()[0];
    for transpose in [false, true] {
        let mut sum = [[0f64; 2]; 4]; let mut zs = [[0f64; 2]; 4];
        for g in &ter.grids {
            for j in 0..g.ny - 1 { for i in 0..g.nx - 1 {
                let a0 = g.positions[j * g.nx + i]; let ax = g.positions[j * g.nx + i + 1]; let ay = g.positions[(j + 1) * g.nx + i];
                let sx = (ax[2] - a0[2]) / (ax[0] - a0[0]).abs().max(1.0); let sy = (ay[2] - a0[2]) / (ay[1] - a0[1]).abs().max(1.0);
                let slope = (sx * sx + sy * sy).sqrt() as f64;
                let (mut u, mut v) = ((a0[0] - x0) / (x1 - x0), (a0[1] - y0) / (y1 - y0));
                if transpose { std::mem::swap(&mut u, &mut v); }
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) { continue; }
                let k = ((v * h as f32) as usize * w as usize + (u * w as f32) as usize) * 4;
                for c in 0..4 { let wgt = px[k + c] as f64 / 255.0; sum[c][0] += wgt * slope; sum[c][1] += wgt; zs[c][0] += wgt * a0[2] as f64; zs[c][1] += wgt; }
            }}
        }
        println!("transpose={transpose}: mean height by channel R,G,B,A = {:?}", zs.iter().map(|s| format!("{:.0}", s[0] / s[1].max(1e-9))).collect::<Vec<_>>());
        println!("transpose={transpose}: mean slope weighted by channel R,G,B,A = {:?}", sum.iter().map(|s| format!("{:.3} (w {:.0})", s[0] / s[1].max(1e-9), s[1])).collect::<Vec<_>>());
    }
}
