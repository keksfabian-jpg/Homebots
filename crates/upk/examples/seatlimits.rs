//! Dev: vehicle seats of a class default (GCGameContent): names, gun class, camera, turret
//! controls and the view / turret limit curves (InterpCurveVector raw).
use upk::props::Value;
fn curve(data: &[u8]) -> String {
    if data.len() < 4 {
        return "-".into();
    }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let f = |o: usize| data.get(o..o + 4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).unwrap_or(f32::NAN);
    let stride = if n > 0 { (data.len() - 4 - 8) / n } else { 0 };
    let mut s = format!("n{n} stride{stride}:");
    for k in 0..n {
        let o = 4 + k * stride;
        s += &format!(" [{} -> ({:.1},{:.1},{:.1})]", f(o), f(o + 4), f(o + 8), f(o + 12));
    }
    s
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2]).unwrap();
    let pr = p.export_props(i).unwrap();
    for (k, s) in p.struct_array(&pr, "Seats").iter().enumerate() {
        println!("seat {k}: gun {:?} cam {:?} offset {:?} behind {:?}..{:?} prefix {:?}", s.object("GunClass").filter(|&o| o != 0).map(|o| p.object_name(o)), s.enum_name("CameraTag"), s.vector("CameraOffset"), s.vector("BehindCameraMinOffset"), s.vector("BehindCameraMaxOffset"), s.get("TurretVarPrefix"));
        for n in ["TurretControls", "TurretMoveSocketNames", "GunSocket"] {
            println!("   {n}: {:?}", p.name_array(s.get(n)));
        }
        for n in ["ViewMinLimit", "ViewMaxLimit", "ViewMinLimit1p", "ViewMaxLimit1p", "TurretControllerMinLimit", "TurretControllerMaxLimit"] {
            if let Some(Value::Raw { data, .. }) = s.get(n) {
                println!("   {n}: {}", curve(data));
            }
        }
        for (m, fm) in p.struct_array(s, "SeatFireModes").iter().enumerate() {
            println!("   mode {m}: sockets {:?} flash {:?} sound {:?}", p.name_array(fm.get("Sockets")), p.name_array(fm.get("FlashTags")), fm.object("SCFireSound").filter(|&o| o != 0).map(|o| p.object_path(o)));
        }
        for f in ["fTurretYawMaxSpeed", "fTurretPitchMaxSpeed", "fTurretYawSpeedModifier", "fTurretPitchSpeedModifier"] {
            print!(" {f}={:?}", s.float(f));
        }
        println!();
    }
}
