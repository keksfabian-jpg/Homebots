//! Dev: search localized strings (Coalesced_<lang>.bin) by value or key substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let loc = upk::ui::Loc::load(std::path::Path::new(&a[1])).unwrap();
    let pat = a[2].to_lowercase();
    let mut v: Vec<_> = loc.map.iter().filter(|(k, v)| k.contains(&pat) || v.to_lowercase().contains(&pat)).collect();
    v.sort();
    for (k, val) in v.into_iter().take(60) { println!("{k} = {val}"); }
}
