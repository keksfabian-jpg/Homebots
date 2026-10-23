//! Dev: validate dumped HomeBots WGSL files with naga (Bevy imports replaced by stubs).
const STUB: &str = r#"
struct View { clip_from_world: mat4x4<f32>, world_position: vec3<f32>, viewport: vec4<f32> }
struct Globals { time: f32 }
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> globals: Globals;
fn get_world_from_local(i: u32) -> mat4x4<f32> { return mat4x4<f32>(); }
fn get_tag(i: u32) -> u32 { return 0u; }
"#;
fn main() {
    for f in std::env::args().skip(1) {
        let src = std::fs::read_to_string(&f).unwrap();
        let body: String = src.lines().filter(|l| !l.trim_start().starts_with("#import")).collect::<Vec<_>>().join("\n");
        let full = format!("{STUB}\n{body}");
        match naga::front::wgsl::parse_str(&full) {
            Err(e) => println!("{f}: PARSE {}", e.emit_to_string(&full)),
            Ok(m) => {
                let mut v = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
                if let Err(e) = v.validate(&m) {
                    println!("{f}: VALID {}", e.emit_to_string(&full));
                }
            }
        }
    }
}
