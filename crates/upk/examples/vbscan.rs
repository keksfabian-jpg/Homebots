//! Dev: scan an export for (stride, n, stride, n) headers with n > min.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse().unwrap();
    let min: i32 = a[3].parse().unwrap();
    let b = p.export_bytes(&p.exports[i - 1]);
    let rd = |o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    for o in 0..b.len().saturating_sub(16) {
        let (s, n, s2, n2) = (rd(o), rd(o + 4), rd(o + 8), rd(o + 12));
        if s == s2 && n == n2 && s > 0 && s < 256 && n > min && n < 1_000_000 {
            println!("off {o} stride {s} n {n} end {} len {}", o + 16 + (s * n) as usize, b.len());
        }
        if s > 0 && s < 256 && n > min && n < 1_000_000 && o + 16 + (s * n) as usize <= b.len() && rd(o+8) == n {
            println!("  alt off {o} {s} {n}");
        }
    }
}
