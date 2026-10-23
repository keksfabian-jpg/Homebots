//! Dev: dump Terrain Palettes / Layers struct arrays (recursively).
use upk::props::Value;
fn dump(p: &upk::Package, pr: &upk::props::Props, ind: usize) {
    for x in &pr.0 {
        match &x.value {
            Value::Raw { type_name, data, .. } if type_name == "ArrayProperty" || type_name == "StructProperty" => {
                println!("{:ind$}{}[{}] {type_name} {} bytes", "", x.name, x.index, data.len(), ind = ind);
                let subs = if type_name == "ArrayProperty" { p.array_structs(&x.value) } else { vec![] };
                for (k, s) in subs.iter().enumerate() {
                    println!("{:ind$}  #{k}", "", ind = ind);
                    dump(p, s, ind + 4);
                }
                if subs.is_empty() && type_name == "ArrayProperty" {
                    println!("{:ind$}  raw {:02x?}", "", &data[..data.len().min(64)], ind = ind);
                }
            }
            v => println!("{:ind$}{}[{}] = {v:?}", "", x.name, x.index, ind = ind),
        }
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2]).unwrap();
    let pr = p.export_props(i).unwrap();
    for name in a[3..].iter() {
        let sub = upk::props::Props(pr.0.iter().filter(|x| &x.name == name).cloned().collect());
        dump(&p, &sub, 0);
    }
}
