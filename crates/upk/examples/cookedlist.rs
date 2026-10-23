fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    for p in lib.cooked_files(&a[2]) { println!("{}", p.display()); }
}
