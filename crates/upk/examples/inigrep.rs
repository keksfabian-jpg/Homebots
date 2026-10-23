//! Dev: search .ini entries of a Coalesced file by key substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let loc = upk::ui::Loc::load(std::path::Path::new(&a[1])).unwrap();
    let pat = a[2].to_lowercase();
    let mut v: Vec<_> = loc.ini.iter().filter(|(k, _)| k.contains(&pat)).collect();
    v.sort();
    for (k, val) in v.into_iter().take(400) { for x in val { println!("{k} = {x}"); } }
}
