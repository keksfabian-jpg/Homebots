//! Dev: UField Next chain from a class's Children (declaration order), with property flags.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let ci: usize = a[2].parse().unwrap();
    let mut r = p.export_reader(ci - 1);
    let mut ints = Vec::new();
    for _ in 0..12 { ints.push(r.i32().unwrap_or(0)); }
    println!("class header ints {:?}", ints.iter().map(|&i| (i, if i > 0 && (i as usize) <= p.exports.len() { p.object_path(i) } else { String::new() })).collect::<Vec<_>>());
    let start: i32 = a[3].parse().unwrap();
    let mut cur = start;
    let mut n = 0;
    while cur > 0 && n < 200 {
        let mut r = p.export_reader(cur as usize - 1);
        let mut w = Vec::new();
        for _ in 0..6 { w.push(r.i32().unwrap_or(0)); }
        // net index, None props (fname 8 bytes), Next
        let next = w[a.get(4).and_then(|x| x.parse::<usize>().ok()).unwrap_or(3)];
        if n < 3 { println!("  raw {:?}", w); }
        println!("#{cur} {} [{}]", p.object_name(cur), p.export_class(&p.exports[cur as usize - 1]));
        cur = next;
        n += 1;
    }
}
