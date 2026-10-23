//! Dev: print PostProcessSettings structs of PostProcessVolumes / WorldInfo.
use upk::props::{Rd, Value};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        let c = p.export_class(e);
        if c != "PostProcessVolume" && c != "WorldInfo" { continue; }
        let mut r = p.export_reader(i);
        let Some(pr) = p.read_props(&mut r) else { continue };
        for x in &pr.0 {
            if let Value::Raw { struct_name: Some(sn), data, .. } = &x.value {
                if sn != "PostProcessSettings" { continue; }
                println!("{} . {}", p.object_path(i as i32 + 1), x.name);
                for q in p.read_props(&mut Rd::new(data)).unwrap_or_default().0 {
                    let v = match &q.value { Value::Raw { data, struct_name, .. } => format!("{struct_name:?} {:?}", data.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap_or([0;4]))).collect::<Vec<_>>()), v => format!("{v:?}") };
                    println!("   {} = {}", q.name, v);
                }
            }
        }
    }
}
