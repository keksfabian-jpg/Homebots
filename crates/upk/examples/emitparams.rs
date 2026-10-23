//! Dev: ParticleSystemComponent properties of placed emitters using a template (by name).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if !p.export_class(e).contains("Emitter") || !p.object_path(i as i32 + 1).starts_with("TheWorld.") { continue; }
        let Some(ap) = p.export_props(i) else { continue };
        let Some(pc) = ap.object("ParticleSystemComponent").filter(|&x| x > 0) else { continue };
        let Some(cp) = p.export_props(pc as usize - 1) else { continue };
        let t = cp.object("Template").unwrap_or(0);
        if !p.object_path(t).contains(&a[2]) { continue; }
        println!("{} -> {}", p.object_path(i as i32 + 1), p.object_path(pc));
        if let Some(upk::props::Value::Raw { data, .. }) = cp.0.iter().find(|x| x.name == "InstanceParameters").map(|x| &x.value) {
            let mut rd = upk::props::Rd::new(&data[4..]);
            while let Some(ip) = p.read_props(&mut rd) {
                println!("   param: {:?}", ip.0.iter().map(|q| format!("{}={:?}", q.name, q.value)).collect::<Vec<_>>());
                if rd.pos >= data.len() - 4 { break; }
            }
        }
        for x in &cp.0 { println!("   {} = {}", x.name, format!("{:?}", x.value).chars().take(300).collect::<String>()); }
        for x in &ap.0 { println!("   actor {} = {}", x.name, format!("{:?}", x.value).chars().take(120).collect::<String>()); }
    }
}
