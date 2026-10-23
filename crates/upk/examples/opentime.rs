//! Dev: time Package::open for files.
fn main() {
    for f in std::env::args().skip(1) {
        let t = std::time::Instant::now();
        let p = upk::Package::open(&f);
        println!("{:7.3} s {} {}", t.elapsed().as_secs_f32(), p.map(|p| p.exports.len()).unwrap_or(0), f);
    }
}
