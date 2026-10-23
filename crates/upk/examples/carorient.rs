//! Dev: offline orientation check of the level's interactive cars (levelcars.rs). For every
//! placed GCInteractiveSkelMeshActor (with an InteractiveInfo) of a level package: its mesh's
//! vertices through UE3 LocalToWorld = RotationTranslation(RotOrigin, Origin) * actor
//! (FRotationMatrix(Rotation), DrawScale) and the world-space extents. A car stands upright when
//! the z extent is the car's height (smallest of the three, ~1.3-3 m) and its lowest vertex lies
//! at the actor's ground. Also prints the extents of the old fixed vehicle convention
//! (m.z, -m.x, -m.y) that stood the cars on end.
//! Usage: carorient <level package> [more packages to find the meshes in]
fn rot(r: [i32; 3]) -> [[f32; 3]; 3] {
    // FRotationMatrix rows (X, Y, Z axes); v' = v.x X + v.y Y + v.z Z
    let a = |u: i32| u as f32 / 65536.0 * std::f32::consts::TAU;
    let (sp, cp) = a(r[0]).sin_cos();
    let (sy, cy) = a(r[1]).sin_cos();
    let (sr, cr) = a(r[2]).sin_cos();
    [[cp * cy, cp * sy, sp], [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp], [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp]]
}
fn apply(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    let mut o = [0.0; 3];
    for k in 0..3 {
        o[k] = v[0] * m[0][k] + v[1] * m[1][k] + v[2] * m[2][k];
    }
    o
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pk: Vec<upk::Package> = a[1..].iter().filter_map(|f| upk::Package::open(f).ok()).collect();
    let p = &pk[0];
    let (mut ok, mut bad) = (0, 0);
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "GCInteractiveSkelMeshActor" || !p.object_path(i as i32 + 1).starts_with("TheWorld.PersistentLevel.") {
            continue;
        }
        let Some(ap) = p.export_props(i) else { continue };
        let Some(info) = ap.object("InteractiveInfo").filter(|&o| o != 0) else { continue };
        let ipath = p.object_path(info);
        // the info's SkeletalMesh path, then the mesh in any given package
        let mesh_path = pk.iter().find_map(|q| {
            let k = q.exports.iter().enumerate().position(|(k, e)| q.export_class(e) == "GCInteractiveObjectInfo" && q.object_path(k as i32 + 1).eq_ignore_ascii_case(&ipath))?;
            let m = q.export_props(k)?.object("SkeletalMesh")?;
            Some(q.object_path(m))
        });
        let Some(mesh_path) = mesh_path else { continue };
        let Some(mesh) = pk.iter().find_map(|q| {
            let k = q.exports.iter().enumerate().position(|(k, e)| q.export_class(e) == "SkeletalMesh" && q.object_path(k as i32 + 1).eq_ignore_ascii_case(&mesh_path))?;
            q.read_skeletal_mesh(k)
        }) else {
            continue;
        };
        let loc = ap.vector("Location").unwrap_or([0.0; 3]);
        let r = ap.rotator("Rotation").unwrap_or([0; 3]);
        let ds = ap.float("DrawScale").unwrap_or(1.0);
        let s3 = ap.vector("DrawScale3D").unwrap_or([1.0; 3]);
        let (ra, ro) = (rot(r), rot(mesh.rot_origin));
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let (mut olo, mut ohi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for q in &mesh.lod.positions {
            let v: [f32; 3] = (*q).into();
            let m = apply(&ro, v);
            let m = [(m[0] + mesh.origin[0]) * ds * s3[0], (m[1] + mesh.origin[1]) * ds * s3[1], (m[2] + mesh.origin[2]) * ds * s3[2]];
            let w = apply(&ra, m);
            let old = apply(&ra, [v[2], -v[0], -v[1]]);
            for k in 0..3 {
                lo[k] = lo[k].min(w[k] + loc[k]);
                hi[k] = hi[k].max(w[k] + loc[k]);
                olo[k] = olo[k].min(old[k]);
                ohi[k] = ohi[k].max(old[k]);
            }
        }
        let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        let oext = [ohi[0] - olo[0], ohi[1] - olo[1], ohi[2] - olo[2]];
        let upright = ext[2] < ext[0].max(ext[1]) && ext[2] > 50.0 && ext[2] < 400.0;
        if upright {
            ok += 1;
        } else {
            bad += 1;
        }
        println!(
            "{} {} loc {:?} rot {:?} scale {ds}x{s3:?} rot_origin {:?}: extents {:.0},{:.0},{:.0} (z {:.0}..{:.0}) {} | old convention {:.0},{:.0},{:.0}",
            p.object_path(i as i32 + 1).rsplit('.').next().unwrap_or(""),
            mesh_path.rsplit('.').next().unwrap_or(""),
            loc.map(|x| x.round()),
            r,
            mesh.rot_origin,
            ext[0],
            ext[1],
            ext[2],
            lo[2] - loc[2],
            hi[2] - loc[2],
            if upright { "UPRIGHT" } else { "NOT UPRIGHT" },
            oext[0],
            oext[1],
            oext[2]
        );
    }
    println!("{ok} upright, {bad} not");
}
