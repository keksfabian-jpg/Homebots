//! Dev: any export with a Location property near x,y,z (all classes).
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let a: Vec<String> = std::env::args().collect();
    let (x, y, z, r): (f32, f32, f32, f32) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap(), a[5].parse().unwrap());
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        let Ok(Some(pr)) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| p.export_props(i))) else { continue };
        let Some(l) = pr.vector("Location") else { continue };
        let d = ((l[0] - x).powi(2) + (l[1] - y).powi(2) + (l[2] - z).powi(2)).sqrt();
        if d < r { println!("{d:.0} {} [{}] {:?}", p.object_path(i as i32 + 1), p.export_class(e), l); }
    }
}
