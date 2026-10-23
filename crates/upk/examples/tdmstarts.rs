//! Dev: GameMode_TDMFrontline / GameMode_FocusedFrontline spawn collections of a map: per
//! collection and spawn type the PlayerStarts (location, yaw, Group, other scalar props), plus
//! the frontline's TerritoryPlane. Usage: tdmstarts <map>
use upk::props::Value;
fn objs(data: &[u8]) -> Vec<i32> {
    if data.len() < 4 { return Vec::new() }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    (0..n).filter_map(|k| data.get(4 + k * 4..8 + k * 4)).map(|b| i32::from_le_bytes(b.try_into().unwrap())).collect()
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        let c = p.export_class(e);
        if c != "GameMode_TDMFrontline" && c != "GameMode_FocusedFrontline" { continue }
        let pr = p.export_props(i).unwrap();
        println!("{} [{c}]", p.object_path(i as i32 + 1));
        if let Some(Value::Raw { data, .. }) = pr.get("TerritoryPlane") {
            let f: Vec<f32> = data.chunks(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
            println!("  TerritoryPlane {f:?}");
        }
        for sc in pr.all("SpawnCollection") {
            let Value::Raw { data, .. } = &sc.value else { continue };
            let Some(col) = p.read_props(&mut upk::props::Rd::new(data)) else { continue };
            println!("  SpawnCollection[{}] Team={:?}", sc.index, col.enum_name("Team"));
            for sp in p.struct_array(&col, "SpawnPoints") {
                let ty = sp.enum_name("Type").unwrap_or("").to_string();
                let Some(Value::Raw { data, .. }) = sp.get("PlayerStart") else { continue };
                for o in objs(data).into_iter().filter(|&o| o > 0) {
                    let Some(st) = p.export_props(o as usize - 1) else { continue };
                    let loc = st.vector("Location").unwrap_or([0.0; 3]);
                    let extra: Vec<String> = st.0.iter().filter(|q| !matches!(q.value, Value::Raw { .. } | Value::Object(_)) && !["Location", "Tag", "DrawScale"].contains(&q.name.as_str())).map(|q| format!("{}={:?}", q.name, q.value)).collect();
                    println!("    {ty:24} {:30} {:8.0} {:8.0} {:6.0}  {}", p.object_name_of(&p.exports[o as usize - 1]), loc[0], loc[1], loc[2], extra.join(" "));
                }
            }
        }
    }
}
