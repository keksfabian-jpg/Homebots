//! Dev tool: decode every Texture2D in a package; optionally write PNGs to a folder.
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let out = a.get(2);
    let (mut ok, mut fail) = (0, 0);
    let mut flags: BTreeMap<String, usize> = BTreeMap::new();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "Texture2D" { continue; }
        let path = p.object_path(i as i32 + 1);
        match p.read_texture2d(i) {
            Some(t) => {
                for m in &t.mips { *flags.entry(format!("{} {:#x}", t.format, m.bulk.flags)).or_default() += 1; }
                match t.decode_rgba() {
                    Some((w, h, px)) => {
                        ok += 1;
                        if let Some(dir) = out {
                            let f = format!("{dir}/{}.png", path.replace('.', "_"));
                            image::save_buffer(&f, &px, w, h, image::ColorType::Rgba8).unwrap();
                        }
                    }
                    None => { fail += 1; eprintln!("no inline mip: {path} {} {}x{} mips {}", t.format, t.width, t.height, t.mips.len()); }
                }
            }
            None => { fail += 1; eprintln!("parse fail: {path}"); }
        }
    }
    println!("decoded {ok}, failed {fail}");
    for (k, v) in flags { println!("  {v:5} mips  {k}"); }
}
