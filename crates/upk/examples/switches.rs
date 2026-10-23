//! Dev: for a MIC, list base-material static switches (name, GUID) and where that GUID
//! occurs in the MIC's native data (after properties), with surrounding dwords.
use upk::props::{Rd, Value};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let mic = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "MaterialInstanceConstant").unwrap();
    let mut r = p.export_reader(mic);
    let pr = p.read_props(&mut r).unwrap();
    let native = &r.b[r.pos..];
    println!("MIC native bytes: {}", native.len());
    // base material
    let mut cur = pr.object("Parent").unwrap();
    while p.export_class(&p.exports[cur as usize - 1]) != "Material" {
        let mut r2 = p.export_reader(cur as usize - 1);
        cur = p.read_props(&mut r2).unwrap().object("Parent").unwrap();
    }
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "MaterialExpressionStaticSwitchParameter" || e.outer_index != cur { continue; }
        let mut r3 = p.export_reader(i);
        let ep = p.read_props(&mut r3).unwrap();
        let name = ep.enum_name("ParameterName").unwrap_or("?").to_string();
        let dv = matches!(ep.get("DefaultValue"), Some(Value::Bool(true)));
        let Some(Value::Raw { data: guid, .. }) = ep.get("ExpressionGUID") else { continue };
        let hits: Vec<usize> = (0..native.len().saturating_sub(16)).filter(|&o| &native[o..o + 16] == &guid[..16]).collect();
        for h in &hits {
            let w = |o: isize| { let o = (*h as isize + o) as usize; u32::from_le_bytes(native[o..o + 4].try_into().unwrap()) };
            println!("{name:24} default={dv} guid@{h}: before {:?}", [w(-16), w(-12), w(-8), w(-4)]);
        }
        if hits.is_empty() { println!("{name:24} default={dv} (no override entry)"); }
    }
    let _ = Rd::new(&[]);
}
