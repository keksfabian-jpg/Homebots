//! Dev: muzzle flash templates of a weapon effects node (KFX_WeaponNodes).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2]).unwrap();
    let pr = p.export_own_props(i).unwrap();
    for view in ["View1P", "View3P", "ViewADS"] {
        let Some(v) = p.struct_prop(&pr, view) else { continue };
        let Some(d) = p.struct_prop(&v, "Day") else { continue };
        if let Some(upk::props::Value::Raw { data, .. }) = d.get("MuzzleFlash") {
            let n = u32::from_le_bytes(data[0..4].try_into().unwrap());
            for k in 0..n as usize { let o = i32::from_le_bytes(data[4 + k * 4..8 + k * 4].try_into().unwrap()); println!("{view} muzzle {}", p.object_path(o)); }
        }
    }
}
