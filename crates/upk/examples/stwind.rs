//! Dev: SpeedTree wind params (memory order) of a USpeedTree export.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let t = p.read_speedtree(i).unwrap();
    let w = t.wind.unwrap();
    for (k, v) in w.iter().enumerate() { print!("+{:#04x}={v} ", k * 4); if k % 6 == 5 { println!(); } }
    println!();
}
