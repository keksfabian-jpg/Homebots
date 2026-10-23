//! Dev: Vector2D properties (Position, Size, ...) of exports by index.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for x in &a[2..] {
        let i: usize = x.parse().unwrap();
        let Some(pr) = p.export_props(i - 1) else { continue };
        let mut out = Vec::new();
        for q in &pr.0 {
            if let upk::props::Value::Raw { struct_name: Some(sn), data, .. } = &q.value {
                if sn == "Vector2D" && data.len() >= 8 {
                    let f = |k: usize| f32::from_le_bytes(data[k..k + 4].try_into().unwrap());
                    out.push(format!("{}=({}, {})", q.name, f(0), f(4)));
                }
            }
        }
        println!("#{} {} {}", i, p.object_name(i as i32), out.join(" "));
    }
}
