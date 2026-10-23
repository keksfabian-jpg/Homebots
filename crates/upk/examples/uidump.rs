//! Dev: UI widget tree dump (props merged with archetypes, structs expanded), following Children
//! and component objects. usage: uidump <pkg> <export#> [maxdepth]
use upk::props::{Props, Rd, Value};

fn val(p: &upk::Package, v: &Value, ind: &str, depth: usize) -> String {
    match v {
        Value::Object(o) => format!("-> {}", if *o != 0 { p.object_path(*o) } else { "None".into() }),
        Value::Raw { type_name, struct_name: Some(_), data } if type_name == "StructProperty" => {
            let mut rd = Rd::new(data);
            match p.read_props(&mut rd) {
                Some(sp) if depth < 6 => format!("{{\n{}{ind}}}", props(p, &sp, &format!("{ind}    "), depth + 1)),
                _ => format!("raw {}b {:?}", data.len(), data.chunks_exact(4).take(8).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>()),
            }
        }
        Value::Raw { type_name, data, .. } if type_name == "ArrayProperty" => {
            let mut rd = Rd::new(data);
            let n = rd.i32().unwrap_or(0);
            // array of structs?
            let mut r2 = Rd::new(&data[4.min(data.len())..]);
            if n > 0 && data.len() > 4 + 4 * n as usize {
                if let Some(sp) = p.read_props(&mut r2) {
                    let mut s = format!("[{n}] first {{\n{}{ind}}}", props(p, &sp, &format!("{ind}    "), depth + 1));
                    if n > 1 {
                        s.push_str(&format!(" ... ({} more)", n - 1));
                    }
                    return s;
                }
            }
            if data.len() == 4 + 4 * n.max(0) as usize {
                let objs: Vec<String> = (0..n).map(|k| {
                    let r = i32::from_le_bytes(data[4 + 4 * k as usize..8 + 4 * k as usize].try_into().unwrap());
                    if r != 0 && ((r > 0 && (r as usize) <= p.exports.len()) || (r < 0 && ((-r) as usize) <= p.imports.len())) { p.object_name(r) } else { format!("{r}") }
                }).collect();
                return format!("[{n}] {objs:?}");
            }
            format!("array n={n} {}b", data.len())
        }
        v => format!("{v:?}").chars().take(200).collect(),
    }
}

fn props(p: &upk::Package, pr: &Props, ind: &str, depth: usize) -> String {
    let mut s = String::new();
    for x in &pr.0 {
        if x.name.ends_with("Delegate__") || x.name.starts_with("__") { continue; }
        s.push_str(&format!("{ind}{}[{}] = {}\n", x.name, x.index, val(p, &x.value, ind, depth)));
    }
    s
}

fn dump(p: &upk::Package, o: i32, depth: usize, max: usize) {
    if o <= 0 || depth > max { return; }
    let ind = "  ".repeat(depth);
    println!("{ind}#{o} {} [{}]", p.object_path(o), p.export_class(&p.exports[o as usize - 1]));
    let Some(pr) = p.export_props(o as usize - 1) else { return };
    print!("{}", props(p, &pr, &format!("{ind}  "), 0));
    let follow = ["StringRenderComponent", "ImageComponent", "BackgroundImageComponent", "LabelBackground", "CaptionRenderComponent"];
    for x in &pr.0 {
        if let Value::Object(c) = x.value {
            if follow.contains(&x.name.as_str()) && c > 0 { dump(p, c, depth + 1, max); }
        }
    }
    if let Some(ch) = pr.get("Children") {
        for c in p.array_objects(ch) { dump(p, c, depth + 1, max); }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    dump(&p, a[2].parse().unwrap(), 0, a.get(3).and_then(|s| s.parse().ok()).unwrap_or(8));
}
