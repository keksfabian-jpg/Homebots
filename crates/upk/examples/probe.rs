//! Dev probe: hex-dump the start of the first export of a given class.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let class = &a[2];
    let nth: usize = a.get(3).map(|s| s.parse().unwrap()).unwrap_or(0);
    let len: usize = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(192);
    let (i, e) = p
        .exports
        .iter()
        .enumerate()
        .filter(|(_, e)| &p.export_class(e) == class)
        .nth(nth)
        .expect("no such export");
    println!("#{} {} size {} flags {:#x}", i + 1, p.object_path(i as i32 + 1), e.serial_size, e.object_flags);
    let b = p.export_bytes(e);
    for (row, c) in b[..len.min(b.len())].chunks(16).enumerate() {
        let words: Vec<String> = c
            .chunks(4)
            .map(|w| {
                if w.len() == 4 {
                    let v = i32::from_le_bytes(w.try_into().unwrap());
                    let n = if v >= 0 && (v as usize) < p.names.len() { p.names[v as usize].name.as_str() } else { "" };
                    format!("{v:>10}{:<1}{:<14}", "", if n.len() > 14 { &n[..14] } else { n })
                } else {
                    format!("{w:?}")
                }
            })
            .collect();
        println!("{:5}: {}", row * 16, words.join(""));
    }
}
