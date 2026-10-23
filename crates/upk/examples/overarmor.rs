//! Dev: GCOverArmorInfo archetypes of packages (DestructibleBones compact: bone, outer,
//! DamagePassThrough, VehicleEvents, VehicleEventsOnHidden, VehicleMaterialInfos, sound, impact socket).
use upk::props::{Rd, Value};
fn names(p: &upk::Package, v: Option<&Value>) -> Vec<String> {
    let Some(Value::Raw { data, .. }) = v else { return vec![] };
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    (0..n).map(|k| { let o = 4 + k * 8; let i = u32::from_le_bytes(data[o..o + 4].try_into().unwrap()) as usize; p.names.get(i).map(|x| x.name.clone()).unwrap_or_default() }).collect()
}
fn main() {
    for f in std::env::args().skip(1) {
        let Ok(p) = upk::Package::open(&f) else { continue };
        for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.export_class(e) == "GCOverArmorInfo") {
            println!("== {} {}", f.rsplit('/').next().unwrap(), p.object_path(i as i32 + 1));
            let Some(pr) = p.read_props(&mut p.export_reader(i)) else { continue };
            for b in p.struct_array(&pr, "DestructibleBones") {
                let mats: Vec<String> = p.struct_array(&b, "VehicleMaterialInfos").iter().map(|m| format!("{}={}", m.enum_name("ParameterName").unwrap_or("?"), m.float("ParameterValue").unwrap_or(0.0))).collect();
                let imp = p.struct_prop(&b, "ImpactEffect");
                let ui = p.struct_prop(&b, "MaterialInfo");
                println!("  {:24} outer {:24} pass {:4} static {} ignore {} ev {:?} hid {:?} mat {:?} snd {} imp {:?}/{:?} ui {:?}",
                    b.enum_name("BoneName").unwrap_or("?"), b.enum_name("Outer").unwrap_or("None"), b.float("DamagePassThrough").unwrap_or(0.0),
                    b.bool("bBoneIsStatic").unwrap_or(false), b.bool("bIgnoreDamage").unwrap_or(false),
                    names(&p, b.get("VehicleEvents")), names(&p, b.get("VehicleEventsOnHidden")), mats,
                    b.object("DestructionSound").filter(|&o| o != 0).map(|o| p.object_path(o)).unwrap_or_default(),
                    imp.as_ref().and_then(|x| x.enum_name("SocketName").map(String::from)), imp.as_ref().and_then(|x| x.bool("bMoveSocketToImpactLocation")),
                    ui.as_ref().and_then(|x| x.enum_name("ParameterName").map(String::from)));
            }
            let _ = Rd::new(&[]);
        }
    }
}
