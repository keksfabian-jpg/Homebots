//! Dev: properties of AnimSequences by SequenceName (substring), raw data abbreviated.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "AnimSequence" { continue; }
        let Some(pr) = p.export_props(i) else { continue };
        let n = pr.enum_name("SequenceName").unwrap_or("").to_string();
        if !n.to_lowercase().contains(&a[2].to_lowercase()) { continue; }
        println!("#{} {} {}", i + 1, p.object_path(i as i32 + 1), n);
        for x in &pr.0 {
            match &x.value { upk::props::Value::Raw { type_name, data, .. } => println!("   {} {type_name} {}b", x.name, data.len()), v => println!("   {} = {v:?}", x.name) }
        }
    }
}
