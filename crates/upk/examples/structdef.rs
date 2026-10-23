//! Dev: find the tagged-property block (struct defaults) inside a ScriptStruct export
//! by trying every offset and keeping the longest clean parse.
use upk::props::{Rd, Value};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.object_name_of(e) == a[2]) {
        let b = p.export_bytes(e);
        println!("#{} {} [{}] {} bytes", i + 1, p.object_path(i as i32 + 1), p.export_class(e), b.len());
        let mut best: Option<(usize, upk::props::Props)> = None;
        for off in 0..b.len().saturating_sub(24) {
            let mut r = Rd::new(&b[off..]);
            if let Some(pr) = p.read_props(&mut r) {
                if pr.0.len() >= 3 && best.as_ref().map_or(true, |(_, x)| pr.0.len() > x.0.len()) {
                    best = Some((off, pr));
                }
            }
        }
        if let Some((off, pr)) = best {
            println!("  props @{off}");
            for x in pr.0 {
                let v = match &x.value {
                    Value::Raw { data, struct_name, .. } => format!("{struct_name:?} {:?}", data.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap_or([0; 4]))).collect::<Vec<_>>()),
                    v => format!("{v:?}"),
                };
                println!("   {}[{}] = {}", x.name, x.index, v);
            }
        }
    }
}
