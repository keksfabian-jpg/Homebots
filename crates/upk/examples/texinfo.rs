fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "Texture2D" && p.object_name_of(e) == a[2]).unwrap();
    let t = p.read_texture2d(i).unwrap();
    for pr in &t.props.0 { if !matches!(pr.value, upk::props::Value::Raw{..}) { println!("{} = {:?}", pr.name, pr.value); } else { println!("{} = <raw>", pr.name); } }
    for m in &t.mips { println!("mip {}x{} flags {:#x} count {} disk {} off {} inline {}", m.width, m.height, m.bulk.flags, m.bulk.element_count, m.bulk.size_on_disk, m.bulk.offset_in_file, m.bulk.data.len()); }
}
