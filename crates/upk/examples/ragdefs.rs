//! Dev: GCVehicle RagdollDefinitions (mesh, materials, events, hidden bones) and the stationary
//! / moving lists of a class default object (export index), with name arrays decoded.
fn names(p: &upk::Package, v: Option<&upk::props::Value>) -> Vec<String> {
    let Some(upk::props::Value::Raw { data, .. }) = v else { return vec![] };
    let n = data.get(0..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).unwrap_or(0);
    (0..n).filter_map(|k| { let o = 4 + k * 8; let b = data.get(o..o + 8)?; Some(p.name(upk::FName { index: i32::from_le_bytes(b[0..4].try_into().unwrap()), number: i32::from_le_bytes(b[4..8].try_into().unwrap()) })) }).collect()
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for s in &a[2..] {
        let i: usize = s.parse::<usize>().unwrap() - 1;
        println!("== {}", p.object_path(i as i32 + 1));
        let pr = p.export_own_props(i).unwrap();
        for (k, d) in p.struct_array(&pr, "RagdollDefinitions").iter().enumerate() {
            let mats: Vec<String> = d.get("RagdollMaterials").map(|v| p.array_objects(v).iter().map(|&o| if o == 0 { "-".into() } else { p.object_path(o) }).collect()).unwrap_or_default();
            println!("[{k}] mesh {} asset {} tree {} anim {:?} events {:?} hide {:?} offset {:?} rot {:?} watch {:?}", d.object("RagdollMesh").map(|o| p.object_path(o)).unwrap_or_default(), d.object("RagdollAsset").map(|o| p.object_path(o)).unwrap_or_default(), d.object("RagdollAnimTree").map(|o| p.object_path(o)).unwrap_or_default(), d.enum_name("RagdollAnimationName"), names(&p, d.get("VehicleEventsToFire")), names(&p, d.get("RagdollMeshBonesToHide")), d.vector("RagdollOffset"), d.rotator("RagdollRotationOffset"), d.float("RagdollSuggestedWatchTime"));
            println!("     mats {mats:?}");
        }
        for l in ["RagdollDefinitionListStationary", "RagdollDefinitionListMoving"] {
            println!("{l} {:?}", pr.get(l).map(|v| p.array_objects(v)));
        }
    }
}
