//! UE3 material graph -> WGSL compiler.
//!
//! Compiles one material instance (MIC chain resolved, static switches applied,
//! parameters inlined as constants) into two WGSL functions in the style of UE3's own
//! material compiler:
//!   fn mat_normal(m: MatIn) -> vec3<f32>      // tangent-space Normal input
//!   fn mat_eval(m: MatIn) -> MatOut           // all other inputs
//! The surrounding shader (vertex factory, base-pass lighting) lives in the renderer.
//! Node semantics follow UE3's MaterialExpression*::Compile; nodes the renderer cannot
//! provide yet are listed in `Compiled::stubbed`.

use crate::level::{Blend, MaterialInfo};
use crate::props::{Props, Rd, Value};
use crate::Package;
use std::collections::HashMap;
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Address {
    Wrap,
    Clamp,
    Mirror,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TexSlot {
    /// Texture object index in the package (export > 0, import < 0).
    pub object: i32,
    pub cube: bool,
    pub address: [Address; 2],
}

#[derive(Debug, Clone)]
pub struct Compiled {
    pub code: String,
    pub textures: Vec<TexSlot>,
    pub info: MaterialInfo,
    pub lighting: String,
    /// Node classes compiled to neutral values (not supported yet).
    pub stubbed: Vec<String>,
    /// Parameter names reachable from any material input under the instance's static switches.
    pub reachable_params: std::collections::HashSet<String>,
    /// per material input property: the parameters it reaches
    pub reachable_by_input: Vec<(String, std::collections::HashSet<String>)>,
}

#[derive(Clone)]
struct Val {
    e: String,
    n: usize,
}

fn ty(n: usize) -> &'static str {
    match n {
        1 => "f32",
        2 => "vec2<f32>",
        3 => "vec3<f32>",
        _ => "vec4<f32>",
    }
}

fn lit(x: f32) -> String {
    if x.is_finite() {
        let s = format!("{x:?}");
        if s.contains('.') || s.contains('e') { s } else { format!("{s}.0") }
    } else {
        "0.0".into()
    }
}

/// Convert a value to `n` components (scalar broadcast, truncate, or zero-extend).
fn cast(v: &Val, n: usize) -> String {
    if v.n == n {
        return v.e.clone();
    }
    if v.n == 1 {
        return format!("{}({})", ty(n), v.e);
    }
    if v.n > n {
        let sw = &"xyzw"[..n];
        return format!("({}).{sw}", v.e);
    }
    // extend with zeros
    let pad: Vec<&str> = (v.n..n).map(|_| "0.0").collect();
    format!("{}({}, {})", ty(n), v.e, pad.join(", "))
}

struct Ctx<'a> {
    pkg: &'a Package,
    tex_params: Vec<(String, i32)>,
    scalar_params: Vec<(String, f32)>,
    /// scalar parameters read at run time: `mat_param(i)` (i = index in this list)
    dynamic: Vec<String>,
    vector_params: Vec<(String, [f32; 4])>,
    mask_params: Vec<(String, [bool; 4])>,
    /// Native data of each MIC in the chain (child first), searched for switch GUIDs.
    mic_native: Vec<Vec<u8>>,
    /// switch values by ExpressionGUID that take precedence
    switches: Vec<([u8; 16], bool)>,
    textures: Vec<TexSlot>,
    lines: String,
    memo: HashMap<i32, Val>,
    next: usize,
    depth: usize,
    stubbed: Vec<String>,
}

fn struct_props(pkg: &Package, v: &Value) -> Option<Props> {
    let Value::Raw { data, .. } = v else { return None };
    pkg.read_props(&mut Rd::new(data))
}

fn lin_color(v: &Value) -> Option<[f32; 4]> {
    match v {
        Value::LinearColor(c) => Some(*c),
        Value::Raw { data, .. } if data.len() >= 16 => {
            let f = |i: usize| f32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap());
            Some([f(0), f(1), f(2), f(3)])
        }
        _ => None,
    }
}

impl<'a> Ctx<'a> {
    fn tmp(&mut self, v: Val) -> Val {
        let name = format!("v{}", self.next);
        self.next += 1;
        let _ = writeln!(self.lines, "    let {name}: {} = {};", ty(v.n), v.e);
        Val { e: name, n: v.n }
    }

    fn stub(&mut self, class: &str) {
        if !self.stubbed.iter().any(|s| s == class) {
            self.stubbed.push(class.to_string());
        }
    }

    fn texture_slot(&mut self, object: i32, cube: bool) -> usize {
        let address = if object > 0 {
            let p = self.pkg.export_props(object as usize - 1).unwrap_or_default();
            let a = |k: &str| match p.enum_name(k) {
                Some("TA_Clamp") => Address::Clamp,
                Some("TA_Mirror") => Address::Mirror,
                _ => Address::Wrap,
            };
            [a("AddressX"), a("AddressY")]
        } else {
            [Address::Wrap; 2]
        };
        let slot = TexSlot { object, cube, address };
        if let Some(i) = self.textures.iter().position(|t| *t == slot) {
            return i;
        }
        self.textures.push(slot);
        self.textures.len() - 1
    }

    /// Value of a texture object's CompressionSettings == TC_Normalmap* (UE3 unpacks those).
    fn is_normal_map(&self, object: i32) -> bool {
        if object <= 0 {
            return false;
        }
        let p = self.pkg.export_props(object as usize - 1).unwrap_or_default();
        // a colour texture mislabelled TC_Normalmap (Interior_US_AH64_c: LODGroup
        // TEXTUREGROUP_Vehicle, its _n TEXTUREGROUP_VehicleNormalMap): the cooked base-pass PS of
        // M_Interior_US_AH64 (RefShaderCache, examples/matps) adds a uniform to the sample, no
        // unpack; unpacked, the AH64 cockpit came out black (user 2026-10-20)
        let colour_group = matches!(p.enum_name("LODGroup"), Some(g) if !g.contains("NormalMap"));
        let colour_name = self.pkg.object_name(object).to_ascii_lowercase().ends_with("_c");
        if colour_group && colour_name {
            return false;
        }
        matches!(p.enum_name("CompressionSettings"), Some(s) if s.starts_with("TC_Normalmap"))
    }

    fn input(&mut self, props: &Props, name: &str) -> Option<Val> {
        let sp = struct_props(self.pkg, props.get(name)?)?;
        let expr = sp.object("Expression")?;
        let v = self.eval(expr)?;
        if sp.int("Mask").unwrap_or(0) != 0 {
            let sel: String = ["MaskR", "MaskG", "MaskB", "MaskA"]
                .iter()
                .zip("xyzw".chars())
                .enumerate()
                // components the value does not have are dropped (a float2 masked RGB
                // would be invalid HLSL/WGSL as well)
                .filter(|(k, (m, _))| sp.int(m).unwrap_or(0) != 0 && *k < v.n)
                .map(|(_, (_, c))| c)
                .collect();
            if !sel.is_empty() && v.n > 1 {
                return Some(Val { e: format!("({}).{sel}", v.e), n: sel.len() });
            }
        }
        Some(v)
    }

    fn input_or(&mut self, props: &Props, name: &str, default: Val) -> Val {
        self.input(props, name).unwrap_or(default)
    }

    /// Parameter names reachable from expression `obj` (static switches honoured).
    fn collect_params(&self, obj: i32, seen: &mut std::collections::HashSet<i32>, out: &mut std::collections::HashSet<String>) {
        if obj <= 0 || !seen.insert(obj) {
            return;
        }
        let idx = obj as usize - 1;
        let class = self.pkg.export_class(&self.pkg.exports[idx]);
        let class = class.strip_prefix("MaterialExpression").unwrap_or(&class).to_string();
        let Some(p) = self.pkg.export_props(idx) else { return };
        if let Some(n) = p.enum_name("ParameterName") {
            out.insert(n.to_string());
        }
        if class == "StaticSwitchParameter" || class == "StaticSwitch" {
            let on = self.switch_value(&p);
            if let Some(e) = p.get(if on { "A" } else { "B" }).and_then(|v| struct_props(self.pkg, v)).and_then(|sp| sp.object("Expression")) {
                self.collect_params(e, seen, out);
            }
            return;
        }
        for pr in &p.0 {
            if let Value::Raw { struct_name: Some(sn), .. } = &pr.value {
                if sn.contains("Input") {
                    if let Some(e) = struct_props(self.pkg, &pr.value).and_then(|sp| sp.object("Expression")) {
                        self.collect_params(e, seen, out);
                    }
                }
            }
        }
    }

    fn switch_value(&self, props: &Props) -> bool {
        if let Some(Value::Raw { data: guid, .. }) = props.get("ExpressionGUID") {
            if guid.len() >= 16 {
                if let Some(&(_, v)) = self.switches.iter().find(|(g, _)| g[..] == guid[..16]) {
                    return v;
                }
                for nat in &self.mic_native {
                    if let Some(o) = (8..nat.len().saturating_sub(16)).find(|&o| nat[o..o + 16] == guid[..16]) {
                        // entry: FName(8) bValue(4) bOverride(4) GUID(16)
                        let ov = u32::from_le_bytes(nat[o - 4..o].try_into().unwrap()) != 0;
                        if ov {
                            return u32::from_le_bytes(nat[o - 8..o - 4].try_into().unwrap()) != 0;
                        }
                    }
                }
            }
        }
        matches!(props.get("DefaultValue"), Some(Value::Bool(true)))
    }

    fn scalar(&self, p: &Props, k: &str, d: f32) -> f32 {
        p.float(k).unwrap_or(d)
    }

    fn eval(&mut self, expr: i32) -> Option<Val> {
        if expr <= 0 || self.depth > 60 {
            return None;
        }
        if let Some(v) = self.memo.get(&expr) {
            return Some(v.clone());
        }
        let idx = expr as usize - 1;
        let class = self.pkg.export_class(&self.pkg.exports[idx]);
        let class = class.trim_start_matches("MaterialExpression").to_string();
        let p = self.pkg.export_props(idx)?;
        self.depth += 1;
        let c = |x: f32| Val { e: lit(x), n: 1 };
        let out: Option<Val> = match class.as_str() {
            "Constant" => Some(c(self.scalar(&p, "R", 0.0))),
            "Constant2Vector" => Some(Val { e: format!("vec2<f32>({}, {})", lit(self.scalar(&p, "R", 0.0)), lit(self.scalar(&p, "G", 0.0))), n: 2 }),
            "Constant3Vector" => Some(Val {
                e: format!("vec3<f32>({}, {}, {})", lit(self.scalar(&p, "R", 0.0)), lit(self.scalar(&p, "G", 0.0)), lit(self.scalar(&p, "B", 0.0))),
                n: 3,
            }),
            "Constant4Vector" => Some(Val {
                e: format!(
                    "vec4<f32>({}, {}, {}, {})",
                    lit(self.scalar(&p, "R", 0.0)),
                    lit(self.scalar(&p, "G", 0.0)),
                    lit(self.scalar(&p, "B", 0.0)),
                    lit(self.scalar(&p, "A", 0.0))
                ),
                n: 4,
            }),
            "ScalarParameter" => {
                let name = p.enum_name("ParameterName").unwrap_or("").to_string();
                if let Some(i) = self.dynamic.iter().position(|k| *k == name) {
                    return Some(Val { e: format!("mat_param({i})"), n: 1 });
                }
                let v = self.scalar_params.iter().find(|(k, _)| *k == name).map(|x| x.1).unwrap_or(self.scalar(&p, "DefaultValue", 0.0));
                Some(c(v))
            }
            "VectorParameter" => {
                let name = p.enum_name("ParameterName").unwrap_or("").to_string();
                let v = self
                    .vector_params
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|x| x.1)
                    .or_else(|| p.get("DefaultValue").and_then(lin_color))
                    .unwrap_or([0.0; 4]);
                Some(Val { e: format!("vec4<f32>({}, {}, {}, {})", lit(v[0]), lit(v[1]), lit(v[2]), lit(v[3])), n: 4 })
            }
            "TextureCoordinate" => {
                let ci = p.int("CoordinateIndex").unwrap_or(0).clamp(0, 3);
                let (ut, vt) = (self.scalar(&p, "UTiling", 1.0), self.scalar(&p, "VTiling", 1.0));
                Some(Val { e: format!("(m.uv[{ci}] * vec2<f32>({}, {}))", lit(ut), lit(vt)), n: 2 })
            }
            // Homefront MaterialExpressionRMapSample (Texture + Coordinates, e.g. GLO_Fences
            // M_indGateNonDEST_C: Texture GLO.white_C on TextureCoordinate_1) is an ordinary 2D
            // sample: the cached PS (FLocalVertexFactory, NoLightMap SkyLight) samples
            // Texture2D_1 at the second UV set (v0.wz) and multiplies it into the diffuse. As a
            // stub it passed the UVs through as a colour: red-green "rainbow" gate pillars.
            c if c.starts_with("TextureSample") || c == "ParticleSubUV" || c == "RMapSample" => {
                let pname = p.enum_name("ParameterName").map(|s| s.to_string());
                let t = pname
                    .as_ref()
                    .and_then(|pn| self.tex_params.iter().find(|(k, _)| k == pn).map(|x| x.1))
                    .or(p.object("Texture"))
                    .unwrap_or(0);
                let cube = c.contains("Cube");
                let coords = self.input(&p, "Coordinates");
                let slot = self.texture_slot(t, cube);
                let slot = if cube {
                    self.textures[..slot].iter().filter(|x| x.cube).count()
                } else {
                    self.textures[..slot].iter().filter(|x| !x.cube).count()
                };
                let uv = match coords {
                    Some(v) => cast(&v, if cube { 3 } else { 2 }),
                    None => if cube { "m.refl_world".into() } else { "m.uv[0]".into() },
                };
                // cube maps are the levels' static environment reflections: the renderer's
                // hf_env_cube may scale them (homebots gamemat.rs: weather night darkening)
                let s = if cube { format!("hf_env_cube(textureSample(tc{slot}, sc{slot}, {uv}))") } else { format!("textureSample(t{slot}, s{slot}, {uv})") };
                let s = if !cube && self.is_normal_map(t) { format!("vec4<f32>(({s}).xyz * 2.0 - 1.0, ({s}).w)") } else { s };
                Some(self.tmp(Val { e: s, n: 4 }))
            }
            "Multiply" | "Add" | "Subtract" | "Divide" | "Max" | "Min" => {
                let one = c(if class == "Multiply" || class == "Divide" { 1.0 } else { 0.0 });
                let a = self.input_or(&p, "A", c(if class == "Multiply" { 1.0 } else { 0.0 }));
                let b = self.input_or(&p, "B", one);
                let n = if a.n == 1 { b.n } else if b.n == 1 { a.n } else { a.n.min(b.n) };
                let (x, y) = (cast(&a, n), cast(&b, n));
                let e = match class.as_str() {
                    "Multiply" => format!("({x} * {y})"),
                    "Add" => format!("({x} + {y})"),
                    "Subtract" => format!("({x} - {y})"),
                    "Divide" => format!("({x} / {y})"),
                    "Max" => format!("max({x}, {y})"),
                    _ => format!("min({x}, {y})"),
                };
                Some(self.tmp(Val { e, n }))
            }
            "DotProduct" => {
                let a = self.input_or(&p, "A", c(0.0));
                let b = self.input_or(&p, "B", c(0.0));
                let n = a.n.max(b.n);
                if n == 1 {
                    Some(self.tmp(Val { e: format!("({} * {})", a.e, b.e), n: 1 }))
                } else {
                    Some(self.tmp(Val { e: format!("dot({}, {})", cast(&a, n), cast(&b, n)), n: 1 }))
                }
            }
            "CrossProduct" => {
                let a = self.input_or(&p, "A", c(0.0));
                let b = self.input_or(&p, "B", c(0.0));
                Some(self.tmp(Val { e: format!("cross({}, {})", cast(&a, 3), cast(&b, 3)), n: 3 }))
            }
            "LinearInterpolate" => {
                let a = self.input_or(&p, "A", c(0.0));
                let b = self.input_or(&p, "B", c(1.0));
                let t = self.input_or(&p, "Alpha", c(0.5));
                let n = a.n.max(b.n);
                let tt = if t.n == 1 { format!("{}({})", ty(n), t.e) } else { cast(&t, n) };
                let tt = if n == 1 { t.e.clone() } else { tt };
                Some(self.tmp(Val { e: format!("mix({}, {}, {tt})", cast(&a, n), cast(&b, n)), n }))
            }
            "Power" => {
                let a = self.input_or(&p, "Base", c(1.0));
                let e = self.input_or(&p, "Exponent", c(1.0));
                // UE3 ClampedPow: pow(max(abs(X), 1e-6), Y)
                let ee = cast(&e, a.n);
                Some(self.tmp(Val { e: format!("pow(max(abs({}), {}(0.000001)), {ee})", a.e, ty(a.n)), n: a.n }))
            }
            "OneMinus" | "Abs" | "Floor" | "Ceil" | "Frac" | "SquareRoot" | "Normalize" | "Sine" | "Cosine" => {
                // MaterialExpressionNormalize names its input VectorInput
                let key = if class == "Normalize" && p.get("VectorInput").is_some() { "VectorInput" } else { "Input" };
                let a = self.input_or(&p, key, c(0.0));
                let e = match class.as_str() {
                    "OneMinus" => format!("(1.0 - {})", a.e),
                    "Abs" => format!("abs({})", a.e),
                    "Floor" => format!("floor({})", a.e),
                    "Ceil" => format!("ceil({})", a.e),
                    "Frac" => format!("fract({})", a.e),
                    "SquareRoot" => format!("sqrt({})", a.e),
                    "Normalize" if a.n == 1 => format!("sign({})", a.e),
                    "Normalize" => format!("normalize({})", a.e),
                    k => {
                        let period = self.scalar(&p, "Period", 1.0);
                        let f = if k == "Sine" { "sin" } else { "cos" };
                        if period > 0.0 {
                            format!("{f}({} * {})", a.e, lit(2.0 * std::f32::consts::PI / period))
                        } else {
                            format!("{f}({})", a.e)
                        }
                    }
                };
                Some(self.tmp(Val { e, n: a.n }))
            }
            "Clamp" => {
                let a = self.input_or(&p, "Input", c(0.0));
                let lo = self.input_or(&p, "Min", c(0.0));
                let hi = self.input_or(&p, "Max", c(1.0));
                Some(self.tmp(Val { e: format!("clamp({}, {}, {})", a.e, cast(&lo, a.n), cast(&hi, a.n)), n: a.n }))
            }
            "ConstantClamp" => {
                let a = self.input_or(&p, "Input", c(0.0));
                let (lo, hi) = (self.scalar(&p, "Min", 0.0), self.scalar(&p, "Max", 1.0));
                Some(self.tmp(Val { e: format!("clamp({}, {}({}), {}({}))", a.e, ty(a.n), lit(lo), ty(a.n), lit(hi)), n: a.n }))
            }
            "ComponentMask" | "StaticComponentMaskParameter" => {
                let a = self.input(&p, "Input")?;
                let mut m = [p.get("R"), p.get("G"), p.get("B"), p.get("A")].map(|v| matches!(v, Some(Value::Bool(true))));
                if class == "StaticComponentMaskParameter" {
                    m = [p.get("DefaultR"), p.get("DefaultG"), p.get("DefaultB"), p.get("DefaultA")].map(|v| matches!(v, Some(Value::Bool(true))));
                    if let Some(name) = p.enum_name("ParameterName") {
                        if let Some((_, o)) = self.mask_params.iter().find(|(k, _)| k == name) {
                            m = *o;
                        }
                    }
                }
                let sel: String = m.iter().zip("xyzw".chars()).filter(|(b, _)| **b).map(|(_, c)| c).collect();
                if sel.is_empty() || a.n == 1 {
                    Some(a)
                } else {
                    let want = sel.len();
                    let sel: String = sel.chars().filter(|ch| "xyzw".find(*ch).unwrap() < a.n).collect();
                    if sel.is_empty() {
                        // a mask of components the input does not have (ScreenPosition.zw on our
                        // 2-wide screen position: FX_lighting M_LightPlaneWarehouse*, the shader
                        // failed to parse): zeros (NOT from data)
                        Some(Val { e: if want == 1 { "0.0".into() } else { format!("vec{want}<f32>(0.0)") }, n: want })
                    } else {
                        Some(Val { e: format!("({}).{sel}", a.e), n: sel.len() })
                    }
                }
            }
            "AppendVector" => {
                let a = self.input_or(&p, "A", c(0.0));
                let b = self.input_or(&p, "B", c(0.0));
                let n = (a.n + b.n).min(4);
                let bb = if a.n + b.n > 4 { cast(&b, 4 - a.n) } else { b.e.clone() };
                Some(self.tmp(Val { e: format!("{}({}, {})", ty(n), a.e, bb), n }))
            }
            "Desaturation" => {
                let a = self.input_or(&p, "Input", c(0.0));
                let pc = self.input_or(&p, "Percent", c(1.0));
                let lf = p.get("LuminanceFactors").and_then(lin_color).unwrap_or([0.3, 0.59, 0.11, 0.0]);
                let a3 = cast(&a, 3);
                let l = format!("dot({a3}, vec3<f32>({}, {}, {}))", lit(lf[0]), lit(lf[1]), lit(lf[2]));
                Some(self.tmp(Val { e: format!("mix({a3}, vec3<f32>({l}), {})", cast(&pc, 1)), n: 3 }))
            }
            "StaticSwitchParameter" | "StaticSwitch" => {
                let on = self.switch_value(&p);
                self.input(&p, if on { "A" } else { "B" })
            }
            "Panner" => {
                let coord = self.input(&p, "Coordinate").unwrap_or(Val { e: "m.uv[0]".into(), n: 2 });
                let t = self.input(&p, "Time").map(|v| cast(&v, 1)).unwrap_or("m.time".into());
                let (sx, sy) = (self.scalar(&p, "SpeedX", 0.0), self.scalar(&p, "SpeedY", 0.0));
                Some(self.tmp(Val { e: format!("({} + {t} * vec2<f32>({}, {}))", cast(&coord, 2), lit(sx), lit(sy)), n: 2 }))
            }
            "Rotator" => {
                let coord = self.input(&p, "Coordinate").unwrap_or(Val { e: "m.uv[0]".into(), n: 2 });
                let t = self.input(&p, "Time").map(|v| cast(&v, 1)).unwrap_or("m.time".into());
                let (cx, cy, sp) = (self.scalar(&p, "CenterX", 0.5), self.scalar(&p, "CenterY", 0.5), self.scalar(&p, "Speed", 0.25));
                let arg = self.tmp(Val { e: format!("({t} * {})", lit(sp)), n: 1 });
                let d = self.tmp(Val { e: format!("({} - vec2<f32>({}, {}))", cast(&coord, 2), lit(cx), lit(cy)), n: 2 });
                Some(self.tmp(Val {
                    e: format!(
                        "(vec2<f32>(dot(vec2<f32>(cos({a}), -sin({a})), {d}), dot(vec2<f32>(sin({a}), cos({a})), {d})) + vec2<f32>({}, {}))",
                        lit(cx),
                        lit(cy),
                        a = arg.e,
                        d = d.e
                    ),
                    n: 2,
                }))
            }
            "Time" => Some(Val { e: "m.time".into(), n: 1 }),
            "VertexColor" | "MeshEmitterVertexColor" => Some(Val { e: "m.vcolor".into(), n: 4 }),
            "CameraVector" => Some(Val { e: "m.cam".into(), n: 3 }),
            "ReflectionVector" => Some(Val { e: "m.refl".into(), n: 3 }),
            "LightVector" => Some(Val { e: "m.light".into(), n: 3 }),
            "WorldPosition" => Some(Val { e: "m.world_pos".into(), n: 3 }),
            "CameraWorldPosition" => Some(Val { e: "m.camera_world".into(), n: 3 }),
            "PixelDepth" => Some(Val { e: "m.depth".into(), n: 1 }),
            "ScreenPosition" => Some(Val { e: "m.screen".into(), n: 2 }),
            "Fresnel" => {
                let exp = self.scalar(&p, "Exponent", 3.0);
                let n = self.input(&p, "Normal").map(|v| cast(&v, 3)).unwrap_or("vec3<f32>(0.0, 0.0, 1.0)".into());
                Some(self.tmp(Val { e: format!("pow(max(abs(1.0 - abs(dot({n}, m.cam))), 0.000001), {})", lit(exp)), n: 1 }))
            }
            "BumpOffset" => {
                let coord = self.input(&p, "Coordinate").unwrap_or(Val { e: "m.uv[0]".into(), n: 2 });
                let h = self.input_or(&p, "Height", c(0.0));
                let (ratio, refp) = (self.scalar(&p, "HeightRatio", 0.05), self.scalar(&p, "ReferencePlane", 0.5));
                Some(self.tmp(Val {
                    e: format!("({} + m.cam.xy * (({} - {}) * {}))", cast(&coord, 2), cast(&h, 1), lit(refp), lit(ratio)),
                    n: 2,
                }))
            }
            "If" => {
                let a = self.input_or(&p, "A", c(0.0));
                let b = self.input_or(&p, "B", c(0.0));
                let gt = self.input_or(&p, "AGreaterThanB", c(0.0));
                let lt = self.input_or(&p, "ALessThanB", c(0.0));
                let eq = self.input(&p, "AEqualsB").unwrap_or(gt.clone());
                let n = gt.n.max(lt.n).max(eq.n);
                let (a1, b1) = (cast(&a, 1), cast(&b, 1));
                Some(self.tmp(Val {
                    e: format!("select({}, select({}, {}, {a1} > {b1}), {a1} >= {b1})", cast(&lt, n), cast(&eq, n), cast(&gt, n)),
                    n,
                }))
            }
            "Transform" => {
                let a = self.input_or(&p, "Input", c(0.0));
                // TRANSFORM_World (default): tangent -> world; others -> view (approximated as world).
                let kind = p.enum_name("TransformType").unwrap_or("TRANSFORM_World").to_string();
                if kind != "TRANSFORM_World" {
                    self.stub(&format!("Transform:{kind}"));
                }
                Some(self.tmp(Val { e: format!("(m.tangent_to_world * {})", cast(&a, 3)), n: 3 }))
            }
            // cache PS 49ed7ca6… (Z_Smoke_32FRM_Flat_fogged: Bias = ZFeather -30, BiasScale 1):
            // k = 1 / (1 - Bias) for Bias <= 0.999 else 1000; PS 3696ad02… (M_firetest_alpha:
            // no Bias input, BiasScale 60): k = 1/30, i.e. Bias defaults to 0.5. Both:
            // sat((SceneW - PixelW) * k) * Alpha, k = 1 / max((1 - Bias) * BiasScale, 0.001).
            "DepthBiasedAlpha" => {
                let scale = p.float("BiasScale").unwrap_or(1.0);
                let a = self.input_or(&p, "Alpha", c(1.0));
                let b = self.input_or(&p, "Bias", c(0.5));
                let b1 = cast(&b, 1);
                Some(self.tmp(Val {
                    e: format!("(clamp((m.scene_w - m.pixel_w) / max((1.0 - {b1}) * {scale:?}, 0.001), 0.0, 1.0) * {})", cast(&a, 1)),
                    n: 1,
                }))
            }
            // SceneColor lookup at screen UV (Coordinates, default ScreenPosition); the renderer
            // provides hf_scene_color (3D: copy of the opaque scene, UI: scene behind the canvas).
            // UI PS 2e5b4200 (MI_Gaussian_1pass_sigma15, trace frame 400) uses only .rgb.
            "SceneTexture" => {
                let uv = self.input(&p, "Coordinates").map(|v| cast(&v, 2)).unwrap_or("m.screen".into());
                Some(self.tmp(Val { e: format!("hf_scene_color({uv})"), n: 4 }))
            }
            "Constant2" => None,
            // Homefront / scene nodes not provided by the renderer yet.
            "PrimaryLightColor" => Some(Val { e: "m.light_color".into(), n: 3 }),
            "PrimaryLightDirection" => Some(Val { e: "m.light".into(), n: 3 }),
            "LightAttenuation" | "DynamicShadowFactor" => Some(c(1.0)),
            "DynamicParameter" => Some(Val { e: "vec4<f32>(1.0, 1.0, 1.0, 1.0)".into(), n: 4 }),
            "ParticleTintColor" => Some(Val { e: "vec4<f32>(1.0, 1.0, 1.0, 1.0)".into(), n: 4 }),
            // the engine's GlobalMaterialTintColor (default white); the renderer provides
            // hf_material_tint (homebots gamemat.rs: weather brightness; menu / minimap: white)
            "MaterialTintColor" => Some(Val { e: "hf_material_tint()".into(), n: 4 }),
            // Homefront node (cache PS 49ed7ca6…, Z_Smoke_32FRM_Flat_fogged):
            // lerp(Input, TranslucentFogColorTexture(screen).rgb, sat((w - FarFogParams.x) / (FarFogParams.y - FarFogParams.x))).
            // The renderer has no fog yet (game shaders use fog factor 0), so this is Input.
            "ParticleFogColor" => Some(self.input_or(&p, "Input", c(0.0))),
            // Homefront node: sky light for the normal A (world space). The renderer's
            // hf_sky_light gives the GameMat ambient hemisphere (Upper/LowerSkyColor weighted
            // (0.5 +- 0.5 N.z)^2); the node's own shader code is not decoded. Passing A through
            // (the generic stub) added the normal as a colour: rainbow fireballs / smoke.
            "SkyLightContribution" => {
                let a = self.input(&p, "A").map(|v| cast(&v, 3)).unwrap_or("vec3<f32>(0.0, 0.0, 1.0)".into());
                Some(self.tmp(Val { e: format!("hf_sky_light({a})"), n: 3 }))
            }
            other => {
                self.stub(other);
                // pass through the first connected input, else 1
                let mut res = None;
                for x in &p.0 {
                    if let Some(sp) = struct_props(self.pkg, &x.value) {
                        if sp.object("Expression").is_some() {
                            res = self.input(&p, &x.name);
                            break;
                        }
                    }
                }
                res.or(Some(c(1.0)))
            }
        };
        self.depth -= 1;
        if let Some(v) = &out {
            self.memo.insert(expr, v.clone());
        }
        out
    }

    /// A material input (ColorMaterialInput etc.): constant if UseConstant, else the
    /// connected expression, else `default`.
    fn material_input(&mut self, base: &Props, name: &str, n: usize, default: &str) -> String {
        if let Some(sp) = base.get(name).and_then(|v| struct_props(self.pkg, v)) {
            if matches!(sp.get("UseConstant"), Some(Value::Bool(true))) {
                return match sp.get("Constant") {
                    Some(Value::Color(c)) => {
                        let g = |x: u8| lit((x as f32 / 255.0).powf(2.2));
                        cast(&Val { e: format!("vec4<f32>({}, {}, {}, {})", g(c[0]), g(c[1]), g(c[2]), lit(c[3] as f32 / 255.0)), n: 4 }, n)
                    }
                    Some(Value::Float(f)) => cast(&Val { e: lit(*f), n: 1 }, n),
                    Some(Value::Vector(v)) => cast(&Val { e: format!("vec3<f32>({}, {}, {})", lit(v[0]), lit(v[1]), lit(v[2])), n: 3 }, n),
                    _ => default.to_string(),
                };
            }
            if sp.object("Expression").is_some() {
                if let Some(v) = self.input(base, name) {
                    return cast(&v, n);
                }
            }
        }
        default.to_string()
    }
}

impl Package {
    /// Compile a Material / MaterialInstanceConstant (export index, 1-based object ref).
    pub fn compile_material(&self, material: i32) -> Option<Compiled> {
        self.compile_material_with(material, &[])
    }

    /// As compile_material, with static switch values given by ExpressionGUID (e.g. the
    /// FStaticParameterSet the shader cache compiled the shaders with); these win over the
    /// instance chain.
    pub fn compile_material_with(&self, material: i32, switches: &[([u8; 16], bool)]) -> Option<Compiled> {
        self.compile_material_ext(material, switches, &MicChain::default())
    }

    /// MaterialInstanceConstant chain inside this package up to the first parent that is not
    /// exported here (an import, e.g. weapon MICs whose parent lives in MAT_LIB): parameter
    /// overrides (child first) and that parent reference.
    pub fn mic_chain(&self, material: i32) -> (MicChain, i32) {
        let mut ch = MicChain::default();
        let mut cur = material;
        for _ in 0..10 {
            if cur <= 0 {
                break;
            }
            let idx = cur as usize - 1;
            if self.export_class(&self.exports[idx]) != "MaterialInstanceConstant" {
                break;
            }
            let mut r = self.export_reader(idx);
            let Some(p) = self.read_props(&mut r) else { break };
            ch.native.push(r.b[r.pos..].to_vec());
            if let Some(v) = p.get("TextureParameterValues") {
                for st in self.array_structs(v) {
                    if let (Some(k), Some(t)) = (st.enum_name("ParameterName"), st.object("ParameterValue")) {
                        if !ch.tex.iter().any(|(n, _)| n == k) {
                            ch.tex.push((k.to_string(), t));
                        }
                    }
                }
            }
            if let Some(v) = p.get("ScalarParameterValues") {
                for st in self.array_structs(v) {
                    if let (Some(k), Some(x)) = (st.enum_name("ParameterName"), st.float("ParameterValue")) {
                        if !ch.scalar.iter().any(|(n, _)| n == k) {
                            ch.scalar.push((k.to_string(), x));
                        }
                    }
                }
            }
            if let Some(v) = p.get("VectorParameterValues") {
                for st in self.array_structs(v) {
                    if let (Some(k), Some(x)) = (st.enum_name("ParameterName"), st.get("ParameterValue").and_then(lin_color)) {
                        if !ch.vector.iter().any(|(n, _)| n == k) {
                            ch.vector.push((k.to_string(), x));
                        }
                    }
                }
            }
            cur = p.object("Parent").unwrap_or(0);
        }
        (ch, cur)
    }

    /// compile_material_with plus parameter overrides from a MIC chain in another package
    /// (they win over this package's instance chain).
    pub fn compile_material_ext(&self, material: i32, switches: &[([u8; 16], bool)], ext: &MicChain) -> Option<Compiled> {
        let info = self.material_info(material);
        let mut tex_params = ext.tex.clone();
        let mut scalar_params = ext.scalar.clone();
        let mut vector_params = ext.vector.clone();
        let mask_params = Vec::new();
        let mut mic_native = ext.native.clone();
        let mut amb_override: Vec<(String, f32)> = Vec::new();
        let mut cur = material;
        let mut base = None;
        for _ in 0..10 {
            if cur <= 0 {
                break;
            }
            let idx = cur as usize - 1;
            let mut r = self.export_reader(idx);
            let p = self.read_props(&mut r)?;
            for k in ["AmbientSpecPower", "AmbientSpecBouncePower", "AmbientSpecBounceAmount", "AmbientSpecColorContribution"] {
                if let Some(f) = p.float(k) {
                    if !amb_override.iter().any(|(n, _)| n == k) {
                        amb_override.push((k.to_string(), f));
                    }
                }
            }
            if self.export_class(&self.exports[idx]) == "Material" {
                base = Some(p);
                break;
            }
            mic_native.push(r.b[r.pos..].to_vec());
            if let Some(v) = p.get("TextureParameterValues") {
                for s in self.array_structs(v) {
                    if let (Some(k), Some(t)) = (s.enum_name("ParameterName"), s.object("ParameterValue")) {
                        if !tex_params.iter().any(|(n, _): &(String, i32)| n == k) {
                            tex_params.push((k.to_string(), t));
                        }
                    }
                }
            }
            if let Some(v) = p.get("ScalarParameterValues") {
                for s in self.array_structs(v) {
                    if let (Some(k), Some(x)) = (s.enum_name("ParameterName"), s.float("ParameterValue")) {
                        if !scalar_params.iter().any(|(n, _): &(String, f32)| n == k) {
                            scalar_params.push((k.to_string(), x));
                        }
                    }
                }
            }
            if let Some(v) = p.get("VectorParameterValues") {
                for s in self.array_structs(v) {
                    if let (Some(k), Some(x)) = (s.enum_name("ParameterName"), s.get("ParameterValue").and_then(lin_color)) {
                        if !vector_params.iter().any(|(n, _): &(String, [f32; 4])| n == k) {
                            vector_params.push((k.to_string(), x));
                        }
                    }
                }
            }
            cur = p.object("Parent").unwrap_or(0);
        }
        let base = base?;
        let mut ctx = Ctx {
            pkg: self,
            tex_params,
            scalar_params,
            dynamic: ext.dynamic.clone(),
            vector_params,
            mask_params,
            mic_native,
            switches: switches.to_vec(),
            textures: Vec::new(),
            lines: String::new(),
            memo: HashMap::new(),
            next: 0,
            depth: 0,
            stubbed: Vec::new(),
        };
        let lighting = base.enum_name("LightingModel").unwrap_or("MLM_Phong").to_string();

        // Pass 1: normal.
        let normal = ctx.material_input(&base, "Normal", 3, "vec3<f32>(0.0, 0.0, 1.0)");
        let mut code = String::new();
        let _ = writeln!(code, "fn mat_normal(m: MatIn) -> vec3<f32> {{\n{}    return {normal};\n}}\n", ctx.lines);

        // Pass 2: everything else (fresh temporaries; m.refl now uses the material normal).
        ctx.lines.clear();
        ctx.memo.clear();
        let diffuse = ctx.material_input(&base, "DiffuseColor", 3, "vec3<f32>(0.0, 0.0, 0.0)");
        let specular = ctx.material_input(&base, "SpecularColor", 3, "vec3<f32>(0.0, 0.0, 0.0)");
        let spec_power = ctx.material_input(&base, "SpecularPower", 1, "15.0");
        let emissive = ctx.material_input(&base, "EmissiveColor", 3, "vec3<f32>(0.0, 0.0, 0.0)");
        let opacity = ctx.material_input(&base, "Opacity", 1, "1.0");
        let opacity_mask = ctx.material_input(&base, "OpacityMask", 1, "1.0");
        let custom = ctx.material_input(&base, "CustomLighting", 3, "vec3<f32>(0.0, 0.0, 0.0)");
        let diffuse_power = ctx.material_input(&base, "DiffusePower", 1, "1.0");
        let amb = |k: &str, d: f32| amb_override.iter().find(|(n, _)| n == k).map(|x| x.1).unwrap_or(d);
        let asp = ctx.material_input(&base, "AmbSpecPower", 1, &lit(amb("AmbientSpecPower", 4.0)));
        let asbp = ctx.material_input(&base, "AmbSpecBouncePower", 1, &lit(amb("AmbientSpecBouncePower", 4.0)));
        let asba = ctx.material_input(&base, "AmbSpecBounceAmt", 1, &lit(amb("AmbientSpecBounceAmount", 1.0)));
        let ascc = ctx.material_input(&base, "AmbSpecColorContrib", 1, &lit(amb("AmbientSpecColorContribution", 1.0)));
        let _ = writeln!(
            code,
            "fn mat_eval(m: MatIn) -> MatOut {{\n    var o: MatOut;\n{}    o.diffuse = {diffuse};\n    o.specular = {specular};\n    o.spec_power = {spec_power};\n    o.emissive = {emissive};\n    o.opacity = {opacity};\n    o.opacity_mask = {opacity_mask};\n    o.custom = {custom};\n    o.diffuse_power = {diffuse_power};\n    o.amb_spec_power = {asp};\n    o.amb_spec_bounce_power = {asbp};\n    o.amb_spec_bounce_amt = {asba};\n    o.amb_spec_color_contrib = {ascc};\n    return o;\n}}",
            ctx.lines
        );
        let _ = Blend::Opaque;
        let mut reachable_params = std::collections::HashSet::new();
        let mut seen = std::collections::HashSet::new();
        let mut reachable_by_input = Vec::new();
        for pr in &base.0 {
            if let Value::Raw { struct_name: Some(sn), .. } = &pr.value {
                if sn.contains("Input") {
                    if let Some(e) = struct_props(self, &pr.value).and_then(|sp| sp.object("Expression")) {
                        let mut own = std::collections::HashSet::new();
                        ctx.collect_params(e, &mut std::collections::HashSet::new(), &mut own);
                        reachable_by_input.push((pr.name.clone(), own));
                        ctx.collect_params(e, &mut seen, &mut reachable_params);
                    }
                }
            }
        }
        Some(Compiled { code, textures: ctx.textures, info, lighting, stubbed: ctx.stubbed, reachable_params, reachable_by_input })
    }
}

/// Parameter overrides of a MaterialInstanceConstant chain (see Package::mic_chain).
#[derive(Debug, Clone, Default)]
pub struct MicChain {
    pub tex: Vec<(String, i32)>,
    pub scalar: Vec<(String, f32)>,
    pub vector: Vec<(String, [f32; 4])>,
    pub native: Vec<Vec<u8>>,
    /// scalar parameters compiled as `mat_param(i)` (run-time values, e.g. UI progress)
    pub dynamic: Vec<String>,
}
