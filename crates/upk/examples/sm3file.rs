//! Dev: translate an SM3 shader blob file to WGSL (CTAB names as constants).
struct B;
impl upk::sm3::Bindings for B {
    fn constant(&self, reg: u32, name: Option<&str>) -> String {
        format!("C{reg}_{}", name.unwrap_or("?").replace(['[', ']'], "_"))
    }
    fn sampler(&self, reg: u32, name: Option<&str>) -> (String, String) {
        (format!("T{reg}_{}", name.unwrap_or("?")), format!("S{reg}"))
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let s = upk::sm3::Shader::parse(&std::fs::read(&a[1]).unwrap()).unwrap();
    println!("// defs {:?}\n// inputs {:?}\n// outputs {:?}\n// ctab {:?}", s.defs, s.inputs, s.outputs, s.ctab);
    println!("{}", upk::sm3::translate(&s, &B).body);
}
