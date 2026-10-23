//! Dev: translate a cached SM3 shader (by shader id hex) to WGSL with CTAB names as constants.
struct B<'a>(&'a upk::sm3::Shader);
impl upk::sm3::Bindings for B<'_> {
    fn constant(&self, reg: u32, name: Option<&str>) -> String {
        format!("C{reg}_{}", name.unwrap_or("?").replace(['[', ']'], "_"))
    }
    fn sampler(&self, reg: u32, name: Option<&str>) -> (String, String) {
        let _ = self.0;
        (format!("T{reg}_{}", name.unwrap_or("?")), format!("S{reg}"))
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let mut id = [0u8; 16];
    for i in 0..16 {
        id[i] = u8::from_str_radix(&a[2][2 * i..2 * i + 2], 16).unwrap();
    }
    let e = &sc.shaders[&id];
    let s = upk::sm3::Shader::parse(&sc.data[e.code_offset..e.code_offset + e.code_len]).unwrap();
    println!("// defs {:?}", s.defs);
    println!("// inputs {:?}\n// ctab {:?}", s.inputs, s.ctab);
    let t = upk::sm3::translate(&s, &B(&s));
    println!("{}", t.body);
}
