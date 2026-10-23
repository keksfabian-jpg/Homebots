//! CPU evaluation of UE3 material expression graphs.
//!
//! Instead of picking "the diffuse texture", the material's own node graph is evaluated
//! over the [0,1] UV square and baked into an RGBA image: DiffuseColor (lit) or
//! EmissiveColor (unlit) into RGB, Opacity / OpacityMask into A. Instance overrides
//! (texture/scalar/vector parameters) and static switch values (read from the
//! instance's native StaticParameters entries by expression GUID) are applied.
//! View/time dependent nodes evaluate to neutral values.

use crate::level::{Blend, MaterialInfo};
use crate::library::Library;
use crate::props::{Props, Rd, Value};
use crate::Package;
use std::collections::HashMap;

type V4 = [f32; 4];

/// A buffer of values over all texels; `n` = number of meaningful components (1..4).
#[derive(Clone)]
struct Buf {
    v: Vec<V4>,
    n: usize,
}

impl Buf {
    fn splat(len: usize, c: V4, n: usize) -> Buf {
        Buf { v: vec![c; len], n }
    }
    fn get(&self, i: usize, c: usize) -> f32 {
        if self.n == 1 { self.v[i][0] } else { self.v[i][c.min(3)] }
    }
}

pub struct Baked {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub info: MaterialInfo,
    /// For lit materials with an EmissiveColor input: sRGB texture of emissive / scale,
    /// and the scale (max linear channel value, can exceed 1).
    pub emissive: Option<(Vec<u8>, f32)>,
}

pub struct Tex {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

/// Instance chain context: overrides gathered from MIC parents (child first).
struct Ctx<'a> {
    pkg: &'a Package,
    lib: &'a Library,
    max_tex: u32,
    tex_params: Vec<(String, i32)>,
    scalar_params: Vec<(String, f32)>,
    vector_params: Vec<(String, V4)>,
    /// Native data of each MIC in the chain (child first), searched for switch GUIDs.
    mic_native: Vec<Vec<u8>>,
    uv: Vec<[f32; 2]>,
    texcache: &'a mut HashMap<i32, Option<std::rc::Rc<Tex>>>,
    depth: usize,
}

fn struct_props(pkg: &Package, v: &Value) -> Option<Props> {
    let Value::Raw { data, .. } = v else { return None };
    pkg.read_props(&mut Rd::new(data))
}

fn lin_color(v: &Value) -> Option<V4> {
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
    fn len(&self) -> usize {
        self.uv.len()
    }

    fn texture(&mut self, t: i32) -> Option<std::rc::Rc<Tex>> {
        if let Some(c) = self.texcache.get(&t) {
            return c.clone();
        }
        let res = (|| {
            if t <= 0 || self.pkg.export_class(&self.pkg.exports[t as usize - 1]) != "Texture2D" {
                return None;
            }
            let idx = t as usize - 1;
            let mut tx = self.pkg.read_texture2d(idx)?;
            tx.load_full(self.lib, &self.pkg.source_package(idx), self.max_tex);
            let (w, h, px) = tx.decode_rgba()?;
            let srgb = !matches!(tx.props.get("SRGB"), Some(Value::Bool(false)));
            let conv = |b: u8| {
                let x = b as f32 / 255.0;
                if srgb { x.powf(2.2) } else { x }
            };
            let px = px.chunks_exact(4).map(|c| [conv(c[0]), conv(c[1]), conv(c[2]), c[3] as f32 / 255.0]).collect();
            Some(std::rc::Rc::new(Tex { w: w as usize, h: h as usize, px }))
        })();
        self.texcache.insert(t, res.clone());
        res
    }

    fn input(&mut self, props: &Props, name: &str) -> Option<Buf> {
        let sp = struct_props(self.pkg, props.get(name)?)?;
        let expr = sp.object("Expression")?;
        let b = self.eval(expr)?;
        // Optional channel mask on the input connection.
        if sp.int("Mask").unwrap_or(0) != 0 {
            let sel: Vec<usize> = ["MaskR", "MaskG", "MaskB", "MaskA"]
                .iter()
                .enumerate()
                .filter(|(_, m)| sp.int(m).unwrap_or(0) != 0)
                .map(|(i, _)| i)
                .collect();
            if !sel.is_empty() {
                return Some(mask(&b, &sel));
            }
        }
        Some(b)
    }

    fn switch_value(&self, props: &Props) -> bool {
        if let Some(Value::Raw { data: guid, .. }) = props.get("ExpressionGUID") {
            if guid.len() >= 16 {
                for nat in &self.mic_native {
                    if let Some(o) = (8..nat.len().saturating_sub(16)).find(|&o| nat[o..o + 16] == guid[..16]) {
                        return u32::from_le_bytes(nat[o - 8..o - 4].try_into().unwrap()) != 0;
                    }
                }
            }
        }
        matches!(props.get("DefaultValue"), Some(Value::Bool(true)))
    }

    fn eval(&mut self, expr: i32) -> Option<Buf> {
        if expr <= 0 || self.depth > 40 {
            return None;
        }
        let idx = expr as usize - 1;
        let class = self.pkg.export_class(&self.pkg.exports[idx]);
        let class = class.trim_start_matches("MaterialExpression").to_string();
        let mut r = self.pkg.export_reader(idx);
        let p = self.pkg.read_props(&mut r)?;
        let n = self.len();
        let f = |p: &Props, k: &str, d: f32| p.float(k).unwrap_or(d);
        self.depth += 1;
        let out = match class.as_str() {
            "Constant" => Some(Buf::splat(n, [f(&p, "R", 0.0); 4], 1)),
            "Constant2Vector" => Some(Buf::splat(n, [f(&p, "R", 0.0), f(&p, "G", 0.0), 0.0, 0.0], 2)),
            "Constant3Vector" => Some(Buf::splat(n, [f(&p, "R", 0.0), f(&p, "G", 0.0), f(&p, "B", 0.0), 0.0], 3)),
            "Constant4Vector" => {
                Some(Buf::splat(n, [f(&p, "R", 0.0), f(&p, "G", 0.0), f(&p, "B", 0.0), f(&p, "A", 0.0)], 4))
            }
            "ScalarParameter" => {
                let name = p.enum_name("ParameterName").unwrap_or("").to_string();
                let v = self.scalar_params.iter().find(|(k, _)| *k == name).map(|x| x.1).unwrap_or(f(&p, "DefaultValue", 0.0));
                Some(Buf::splat(n, [v; 4], 1))
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
                Some(Buf::splat(n, v, 4))
            }
            "TextureCoordinate" => {
                let (ut, vt) = (f(&p, "UTiling", 1.0), f(&p, "VTiling", 1.0));
                Some(Buf { v: self.uv.iter().map(|u| [u[0] * ut, u[1] * vt, 0.0, 0.0]).collect(), n: 2 })
            }
            c if c.starts_with("TextureSample") => {
                let pname = p.enum_name("ParameterName").map(|s| s.to_string());
                let t = pname
                    .as_ref()
                    .and_then(|pn| self.tex_params.iter().find(|(k, _)| k == pn).map(|x| x.1))
                    .or(p.object("Texture"))
                    .unwrap_or(0);
                let coords = self.input(&p, "Coordinates");
                match self.texture(t) {
                    Some(tex) => {
                        let mut v = Vec::with_capacity(n);
                        for i in 0..n {
                            let (u, w) = match &coords {
                                Some(c) => (c.get(i, 0), c.get(i, 1)),
                                None => (self.uv[i][0], self.uv[i][1]),
                            };
                            v.push(sample(&tex, u, w));
                        }
                        Some(Buf { v, n: 4 })
                    }
                    None => Some(Buf::splat(n, [1.0; 4], 4)),
                }
            }
            "Multiply" | "Add" | "Subtract" | "Divide" | "Max" | "Min" | "DotProduct" => {
                let a = self.input(&p, "A").unwrap_or(Buf::splat(n, [if class == "Multiply" { 1.0 } else { 0.0 }; 4], 1));
                let b = self.input(&p, "B").unwrap_or(Buf::splat(n, [if class == "Multiply" || class == "Divide" { 1.0 } else { 0.0 }; 4], 1));
                Some(binop(&a, &b, &class))
            }
            "LinearInterpolate" => {
                let a = self.input(&p, "A").unwrap_or(Buf::splat(n, [0.0; 4], 1));
                let b = self.input(&p, "B").unwrap_or(Buf::splat(n, [1.0; 4], 1));
                let t = self.input(&p, "Alpha").unwrap_or(Buf::splat(n, [0.5; 4], 1));
                let nn = a.n.max(b.n);
                let v = (0..n)
                    .map(|i| {
                        let mut o = [0.0; 4];
                        for c in 0..4 {
                            let k = t.get(i, c);
                            o[c] = a.get(i, c) * (1.0 - k) + b.get(i, c) * k;
                        }
                        o
                    })
                    .collect();
                Some(Buf { v, n: nn })
            }
            "Power" => {
                let a = self.input(&p, "Base").unwrap_or(Buf::splat(n, [1.0; 4], 1));
                let e = self.input(&p, "Exponent").unwrap_or(Buf::splat(n, [1.0; 4], 1));
                let v = (0..n).map(|i| std::array::from_fn(|c| a.get(i, c).max(0.0).powf(e.get(i, c)))).collect();
                Some(Buf { v, n: a.n })
            }
            "OneMinus" | "Abs" | "Floor" | "Ceil" | "Frac" | "SquareRoot" => {
                let a = self.input(&p, "Input").unwrap_or(Buf::splat(n, [0.0; 4], 1));
                let op = |x: f32| match class.as_str() {
                    "OneMinus" => 1.0 - x,
                    "Abs" => x.abs(),
                    "Floor" => x.floor(),
                    "Ceil" => x.ceil(),
                    "Frac" => x.fract(),
                    _ => x.max(0.0).sqrt(),
                };
                Some(Buf { v: a.v.iter().map(|x| x.map(op)).collect(), n: a.n })
            }
            "Clamp" => {
                let a = self.input(&p, "Input").unwrap_or(Buf::splat(n, [0.0; 4], 1));
                let lo = self.input(&p, "Min").unwrap_or(Buf::splat(n, [0.0; 4], 1));
                let hi = self.input(&p, "Max").unwrap_or(Buf::splat(n, [1.0; 4], 1));
                let v = (0..n).map(|i| std::array::from_fn(|c| a.get(i, c).clamp(lo.get(i, c), hi.get(i, c).max(lo.get(i, c))))).collect();
                Some(Buf { v, n: a.n })
            }
            "ConstantClamp" => {
                let a = self.input(&p, "Input").unwrap_or(Buf::splat(n, [0.0; 4], 1));
                let (lo, hi) = (f(&p, "Min", 0.0), f(&p, "Max", 1.0));
                Some(Buf { v: a.v.iter().map(|x| x.map(|y| y.clamp(lo, hi.max(lo)))).collect(), n: a.n })
            }
            "ComponentMask" => {
                let a = self.input(&p, "Input")?;
                let sel: Vec<usize> = ["R", "G", "B", "A"]
                    .iter()
                    .enumerate()
                    .filter(|(_, k)| matches!(p.get(k), Some(Value::Bool(true))))
                    .map(|(i, _)| i)
                    .collect();
                Some(if sel.is_empty() { a } else { mask(&a, &sel) })
            }
            "AppendVector" => {
                let a = self.input(&p, "A").unwrap_or(Buf::splat(n, [0.0; 4], 1));
                let b = self.input(&p, "B").unwrap_or(Buf::splat(n, [0.0; 4], 1));
                let nn = (a.n + b.n).min(4);
                let v = (0..n)
                    .map(|i| {
                        let mut o = [0.0; 4];
                        for c in 0..nn {
                            o[c] = if c < a.n { a.get(i, c) } else { b.get(i, c - a.n) };
                        }
                        o
                    })
                    .collect();
                Some(Buf { v, n: nn })
            }
            "Desaturation" => {
                let a = self.input(&p, "Input").unwrap_or(Buf::splat(n, [0.0; 4], 3));
                let pc = self.input(&p, "Percent").unwrap_or(Buf::splat(n, [1.0; 4], 1));
                let lf = p.get("LuminanceFactors").and_then(lin_color).unwrap_or([0.3, 0.59, 0.11, 0.0]);
                let v = (0..n)
                    .map(|i| {
                        let l = a.get(i, 0) * lf[0] + a.get(i, 1) * lf[1] + a.get(i, 2) * lf[2];
                        let k = pc.get(i, 0);
                        std::array::from_fn(|c| a.get(i, c) * (1.0 - k) + l * k)
                    })
                    .collect();
                Some(Buf { v, n: 3 })
            }
            "StaticSwitchParameter" | "StaticSwitch" => {
                let on = self.switch_value(&p);
                self.input(&p, if on { "A" } else { "B" })
            }
            "Panner" | "Rotator" => self
                .input(&p, "Coordinate")
                .or_else(|| Some(Buf { v: self.uv.iter().map(|u| [u[0], u[1], 0.0, 0.0]).collect(), n: 2 })),
            "VertexColor" | "MeshEmitterVertexColor" => Some(Buf::splat(n, [1.0; 4], 4)),
            "Time" | "CameraVector" | "ReflectionVector" | "LightVector" | "ScreenPosition" | "PixelDepth" | "DestDepth" => {
                Some(Buf::splat(n, [0.0, 0.0, 1.0, 0.0], 3))
            }
            "Fresnel" => Some(Buf::splat(n, [0.0; 4], 1)),
            // Unknown nodes: pass through their first connected input, else neutral 1.
            _ => {
                let mut res = None;
                for x in &p.0 {
                    if let Some(sp) = struct_props(self.pkg, &x.value) {
                        if sp.object("Expression").is_some() {
                            res = self.input(&p, &x.name);
                            break;
                        }
                    }
                }
                res.or(Some(Buf::splat(n, [1.0; 4], 1)))
            }
        };
        self.depth -= 1;
        out
    }
}

fn mask(b: &Buf, sel: &[usize]) -> Buf {
    let v = b
        .v
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let mut o = [0.0; 4];
            for (k, &c) in sel.iter().enumerate() {
                o[k] = b.get(i, c);
            }
            o
        })
        .collect();
    Buf { v, n: sel.len() }
}

fn binop(a: &Buf, b: &Buf, op: &str) -> Buf {
    let n = a.v.len();
    if op == "DotProduct" {
        let k = a.n.max(b.n).min(4);
        let v = (0..n).map(|i| [(0..k).map(|c| a.get(i, c) * b.get(i, c)).sum::<f32>(); 4]).collect();
        return Buf { v, n: 1 };
    }
    let v = (0..n)
        .map(|i| {
            std::array::from_fn(|c| {
                let (x, y) = (a.get(i, c), b.get(i, c));
                match op {
                    "Multiply" => x * y,
                    "Add" => x + y,
                    "Subtract" => x - y,
                    "Divide" => if y.abs() > 1e-6 { x / y } else { 0.0 },
                    "Max" => x.max(y),
                    _ => x.min(y),
                }
            })
        })
        .collect();
    Buf { v, n: a.n.max(b.n) }
}

fn sample(t: &Tex, u: f32, v: f32) -> V4 {
    // Bilinear with wrap.
    let x = u.rem_euclid(1.0) * t.w as f32 - 0.5;
    let y = v.rem_euclid(1.0) * t.h as f32 - 0.5;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |xi: f32, yi: f32| {
        let xi = (xi as i64).rem_euclid(t.w as i64) as usize;
        let yi = (yi as i64).rem_euclid(t.h as i64) as usize;
        t.px[yi * t.w + xi]
    };
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1.0, y0), at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
    std::array::from_fn(|k| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy)
}

impl Package {
    /// Bake a material (Material or MaterialInstanceConstant) into an RGBA image of at
    /// most `size` x `size` texels.
    pub fn bake_material(
        &self,
        lib: &Library,
        material: i32,
        size: u32,
        texcache: &mut HashMap<i32, Option<std::rc::Rc<Tex>>>,
    ) -> Option<Baked> {
        let info = self.material_info(material);
        let mut tex_params = Vec::new();
        let mut scalar_params = Vec::new();
        let mut vector_params = Vec::new();
        let mut mic_native = Vec::new();
        let mut cur = material;
        let mut base = None;
        for _ in 0..10 {
            if cur <= 0 {
                break;
            }
            let idx = cur as usize - 1;
            let mut r = self.export_reader(idx);
            let p = self.read_props(&mut r)?;
            if self.export_class(&self.exports[idx]) == "Material" {
                base = Some(p);
                break;
            }
            mic_native.push(r.b[r.pos..].to_vec());
            if let Some(v) = p.get("TextureParameterValues") {
                for s in self.array_structs(v) {
                    if let (Some(k), Some(t)) = (s.enum_name("ParameterName"), s.object("ParameterValue")) {
                        tex_params.push((k.to_string(), t));
                    }
                }
            }
            if let Some(v) = p.get("ScalarParameterValues") {
                for s in self.array_structs(v) {
                    if let (Some(k), Some(x)) = (s.enum_name("ParameterName"), s.float("ParameterValue")) {
                        scalar_params.push((k.to_string(), x));
                    }
                }
            }
            if let Some(v) = p.get("VectorParameterValues") {
                for s in self.array_structs(v) {
                    if let (Some(k), Some(x)) = (s.enum_name("ParameterName"), s.get("ParameterValue").and_then(lin_color)) {
                        vector_params.push((k.to_string(), x));
                    }
                }
            }
            cur = p.object("Parent").unwrap_or(0);
        }
        let base = base?;
        let color_input = if info.unlit || matches!(info.blend, Blend::Modulate | Blend::Additive) {
            "EmissiveColor"
        } else {
            "DiffuseColor"
        };
        let alpha_input = match info.blend {
            Blend::Masked => Some("OpacityMask"),
            Blend::Translucent => Some("Opacity"),
            _ => None,
        };
        // Resolution: largest texture the graph samples, capped at `size`.
        let res = {
            let mut m = 4u32;
            let mut scan = |t: i32| {
                if t > 0 && self.export_class(&self.exports[t as usize - 1]) == "Texture2D" {
                    let mut r = self.export_reader(t as usize - 1);
                    if let Some(tp) = self.read_props(&mut r) {
                        m = m.max(tp.int("SizeX").unwrap_or(4) as u32).max(tp.int("SizeY").unwrap_or(4) as u32);
                    }
                }
            };
            for (_, t) in &tex_params {
                scan(*t);
            }
            if let Some(v) = base.get("ReferencedTextures") {
                for t in self.array_objects(v) {
                    scan(t);
                }
            }
            m.min(size).max(4)
        };
        let n = (res * res) as usize;
        let uv: Vec<[f32; 2]> =
            (0..n).map(|i| [((i % res as usize) as f32 + 0.5) / res as f32, ((i / res as usize) as f32 + 0.5) / res as f32]).collect();
        let mut ctx = Ctx { pkg: self, lib, max_tex: size, tex_params, scalar_params, vector_params, mic_native, uv, texcache, depth: 0 };
        let color = ctx.input(&base, color_input).unwrap_or(Buf::splat(n, [0.5, 0.5, 0.5, 1.0], 3));
        let alpha = alpha_input.and_then(|a| ctx.input(&base, a));
        // UE3 adds EmissiveColor on top of the lit diffuse; keep it for lit materials.
        let emissive = if color_input == "DiffuseColor" {
            ctx.input(&base, "EmissiveColor").and_then(|e| {
                let rgb = |i: usize| if e.n == 1 { [e.get(i, 0); 3] } else { [e.get(i, 0), e.get(i, 1), e.get(i, 2)] };
                let m = (0..n).map(|i| rgb(i).into_iter().fold(0.0f32, f32::max)).fold(0.0f32, f32::max);
                if m <= 1e-3 {
                    return None;
                }
                let mut px = Vec::with_capacity(n * 4);
                for i in 0..n {
                    let c = rgb(i);
                    for k in 0..3 {
                        px.push(((c[k] / m).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8);
                    }
                    px.push(255);
                }
                Some((px, m))
            })
        } else {
            None
        };
        let mut rgba = Vec::with_capacity(n * 4);
        let to8 = |x: f32| (x.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8;
        for i in 0..n {
            let (r, g, b) = if color.n == 1 {
                let x = color.get(i, 0);
                (x, x, x)
            } else {
                (color.get(i, 0), color.get(i, 1), color.get(i, 2))
            };
            let a = alpha.as_ref().map(|a| a.get(i, 0)).unwrap_or(1.0);
            rgba.extend_from_slice(&[to8(r), to8(g), to8(b), (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8]);
        }
        Some(Baked { width: res, height: res, rgba, info, emissive })
    }
}

pub type TexCache = HashMap<i32, Option<std::rc::Rc<Tex>>>;
