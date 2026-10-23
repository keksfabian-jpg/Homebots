//! Dev: Kismet graph of every SequenceOp whose path contains a substring: class, scalar props,
//! OutputLinks -> linked op (input index), VariableLinks -> variables (+ their ObjValue / value).
use upk::props::Value;
fn short(p: &upk::Package, o: i32) -> String {
    let path = p.object_path(o);
    path.rsplit('.').next().unwrap_or(&path).to_string()
}
fn val(p: &upk::Package, v: &Value) -> Option<String> {
    Some(match v {
        Value::Object(o) => format!("{}", p.object_path(*o)),
        Value::Raw { type_name, data, .. } if type_name == "ArrayProperty" && data.len() >= 4 => {
            let n = i32::from_le_bytes(data[0..4].try_into().unwrap());
            if data.len() == 4 + n as usize * 4 {
                let v: Vec<String> = (0..n as usize).map(|k| p.object_path(i32::from_le_bytes(data[4 + k * 4..8 + k * 4].try_into().unwrap()))).collect();
                format!("[{}]", v.join(", "))
            } else { return None; }
        }
        Value::Raw { .. } => return None,
        v => format!("{v:?}"),
    })
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let pat = a[2].to_lowercase();
    let skip = ["ObjPosX", "ObjPosY", "DrawWidth", "DrawHeight", "MaxWidth", "ParentSequence", "ObjInstanceVersion", "InputLinks", "OutputLinks", "VariableLinks", "EventLinks", "bDeleteMe", "ObjName", "OutputLabel", "VarName"];
    for (i, e) in p.exports.iter().enumerate() {
        let cls = p.export_class(e);
        if !(cls.starts_with("Seq") || cls.starts_with("GCSeq")) || cls.starts_with("SeqVar") { continue; }
        let path = p.object_path(i as i32 + 1);
        if !path.to_lowercase().contains(&pat) { continue; }
        let Some(pr) = p.export_props(i) else { continue };
        let mut s: Vec<String> = Vec::new();
        for q in &pr.0 {
            if skip.contains(&q.name.as_str()) { continue; }
            if let Some(v) = val(&p, &q.value) { s.push(format!("{}={}", q.name, v)); }
        }
        println!("{} [{}] {}", short(&p, i as i32 + 1), cls, s.join(" "));
        for ol in p.struct_array(&pr, "OutputLinks") {
            let desc = ol.get("LinkDesc").map(|v| format!("{v:?}")).unwrap_or_default();
            let mut tg = Vec::new();
            for l in p.struct_array(&ol, "Links") {
                let op = l.object("LinkedOp").unwrap_or(0);
                tg.push(format!("{}#in{}", if op > 0 { short(&p, op) } else { "-".into() }, l.int("InputLinkIdx").unwrap_or(0)));
            }
            if !tg.is_empty() { println!("    out {desc} -> {}", tg.join(", ")); }
        }
        for vl in p.struct_array(&pr, "VariableLinks") {
            let desc = vl.get("LinkDesc").map(|v| format!("{v:?}")).unwrap_or_default();
            let mut tg = Vec::new();
            if let Some(Value::Raw { data, .. }) = vl.get("LinkedVariables") {
                let n = i32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
                for k in 0..n {
                    if data.len() < 8 + k * 4 { break; }
                    let o = i32::from_le_bytes(data[4 + k * 4..8 + k * 4].try_into().unwrap());
                    if o <= 0 { continue; }
                    let vp = p.export_props(o as usize - 1);
                    let v = vp.as_ref().and_then(|vp| vp.0.iter().find(|q| matches!(q.name.as_str(), "ObjValue" | "IntValue" | "FloatValue" | "bValue" | "FindVarName")).and_then(|q| val(&p, &q.value)));
                    tg.push(format!("{}={}", short(&p, o), v.unwrap_or_default()));
                }
            }
            if !tg.is_empty() { println!("    var {desc}: {}", tg.join(", ")); }
        }
        for el in p.struct_array(&pr, "EventLinks") {
            let desc = el.get("LinkDesc").map(|v| format!("{v:?}")).unwrap_or_default();
            println!("    evlink {desc}");
        }
    }
}
