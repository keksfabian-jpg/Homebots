//! Cooked FMaterialResource data (UE3 v576 as cooked by Homefront): the uniform
//! expressions that feed a compiled shader's `UniformVector_N`, `UniformScalars_N` and
//! `Texture2D_N` / `TextureCube_N` constants.
//!
//! Layout as found in MaterialInstance natives and the Terrain native
//! (FTerrainMaterialResource), starting at the resource Id:
//! FGuid Id | i32 NumUserTexCoords | TArray PixelVectors | TArray PixelScalars |
//! TArray Uniform2DTextures | TArray UniformCubeTextures | ...
//! Each element is an FName class name (FMaterialUniformExpression*) + its data.

use crate::props::Rd;
use crate::Package;

#[derive(Debug, Clone)]
pub enum UExpr {
    VectorParameter { name: String, default: [f32; 4] },
    ScalarParameter { name: String, default: f32 },
    TextureParameter { name: String, object: i32 },
    Texture { object: i32 },
    Constant([f32; 4]),
    FoldedMath(Box<UExpr>, Box<UExpr>, u8),
    AppendVector(Box<UExpr>, Box<UExpr>, u32),
    Sine(Box<UExpr>, bool),
    Periodic(Box<UExpr>),
    Abs(Box<UExpr>),
    Ceil(Box<UExpr>),
    Floor(Box<UExpr>),
    Frac(Box<UExpr>),
    Clamp(Box<UExpr>, Box<UExpr>, Box<UExpr>),
    Min(Box<UExpr>, Box<UExpr>),
    Max(Box<UExpr>, Box<UExpr>),
    Time,
    RealTime,
}

#[derive(Debug, Clone, Default)]
pub struct UniformSet {
    pub vectors: Vec<UExpr>,
    pub scalars: Vec<UExpr>,
    pub tex2d: Vec<UExpr>,
    pub cubes: Vec<UExpr>,
}

impl Package {
    fn read_uexpr(&self, r: &mut Rd) -> Option<UExpr> {
        let class = self.name(r.fname()?);
        let class = class.strip_prefix("FMaterialUniformExpression").unwrap_or(&class).to_string();
        let lc = |r: &mut Rd| -> Option<[f32; 4]> { Some([r.f32()?, r.f32()?, r.f32()?, r.f32()?]) };
        Some(match class.as_str() {
            "VectorParameter" => UExpr::VectorParameter { name: self.name(r.fname()?), default: lc(r)? },
            "ScalarParameter" => UExpr::ScalarParameter { name: self.name(r.fname()?), default: r.f32()? },
            "TextureParameter" => UExpr::TextureParameter { name: self.name(r.fname()?), object: r.i32()? },
            "Texture" => UExpr::Texture { object: r.i32()? },
            "Constant" => {
                let v = lc(r)?;
                let _value_type = r.u8()?;
                UExpr::Constant(v)
            }
            "FoldedMath" => {
                let a = self.read_uexpr(r)?;
                let b = self.read_uexpr(r)?;
                UExpr::FoldedMath(Box::new(a), Box::new(b), r.u8()?)
            }
            "AppendVector" => {
                let a = self.read_uexpr(r)?;
                let b = self.read_uexpr(r)?;
                UExpr::AppendVector(Box::new(a), Box::new(b), r.u32()?)
            }
            "Sine" => {
                let x = self.read_uexpr(r)?;
                UExpr::Sine(Box::new(x), r.u32()? != 0)
            }
            "Periodic" => UExpr::Periodic(Box::new(self.read_uexpr(r)?)),
            "Abs" => UExpr::Abs(Box::new(self.read_uexpr(r)?)),
            "Ceil" => UExpr::Ceil(Box::new(self.read_uexpr(r)?)),
            "Floor" => UExpr::Floor(Box::new(self.read_uexpr(r)?)),
            "Frac" => UExpr::Frac(Box::new(self.read_uexpr(r)?)),
            "Clamp" => {
                let a = self.read_uexpr(r)?;
                let b = self.read_uexpr(r)?;
                let c = self.read_uexpr(r)?;
                UExpr::Clamp(Box::new(a), Box::new(b), Box::new(c))
            }
            "Min" => {
                let a = self.read_uexpr(r)?;
                UExpr::Min(Box::new(a), Box::new(self.read_uexpr(r)?))
            }
            "Max" => {
                let a = self.read_uexpr(r)?;
                UExpr::Max(Box::new(a), Box::new(self.read_uexpr(r)?))
            }
            "Time" => UExpr::Time,
            "RealTime" => UExpr::RealTime,
            _ => return None,
        })
    }

    fn read_uexpr_array(&self, r: &mut Rd) -> Option<Vec<UExpr>> {
        let n = r.i32()?;
        if !(0..4096).contains(&n) {
            return None;
        }
        (0..n).map(|_| self.read_uexpr(r)).collect()
    }

    /// Parse the uniform expression arrays of a material resource whose Id starts at
    /// `data[at]`. Returns the set and the offset after the cube texture array.
    pub fn read_uniform_set(&self, data: &[u8], at: usize) -> Option<(UniformSet, usize)> {
        let mut r = Rd::new(data);
        r.pos = at + 16;
        let _num_user_tex_coords = r.i32()?;
        let vectors = self.read_uexpr_array(&mut r)?;
        let scalars = self.read_uexpr_array(&mut r)?;
        let tex2d = self.read_uexpr_array(&mut r)?;
        let cubes = self.read_uexpr_array(&mut r)?;
        Some((UniformSet { vectors, scalars, tex2d, cubes }, r.pos))
    }
}

impl UExpr {
    /// Evaluate a vector/scalar expression (parameters at their stored values).
    pub fn eval(&self, time: f32) -> [f32; 4] {
        use UExpr::*;
        let un = |e: &UExpr, f: &dyn Fn(f32) -> f32| e.eval(time).map(f);
        match self {
            VectorParameter { default, .. } => *default,
            ScalarParameter { default, .. } => [*default; 4],
            Constant(v) => *v,
            // EFoldedMathOperation: Add, Sub, Mul, Div, Dot
            FoldedMath(a, b, op) => {
                let (a, b) = (a.eval(time), b.eval(time));
                match op {
                    0 => [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]],
                    1 => [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]],
                    2 => [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]],
                    3 => [a[0] / b[0], a[1] / b[1], a[2] / b[2], a[3] / b[3]],
                    _ => [a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]; 4],
                }
            }
            AppendVector(a, b, n) => {
                let (a, b) = (a.eval(time), b.eval(time));
                let mut o = [0.0; 4];
                let n = (*n as usize).min(4);
                o[..n].copy_from_slice(&a[..n]);
                for k in n..4 {
                    o[k] = b[k - n];
                }
                o
            }
            Sine(x, cos) => un(x, &|v| if *cos { v.cos() } else { v.sin() }),
            Periodic(x) => un(x, &|v| v - v.floor()),
            Abs(x) => un(x, &|v| v.abs()),
            Ceil(x) => un(x, &|v| v.ceil()),
            Floor(x) => un(x, &|v| v.floor()),
            Frac(x) => un(x, &|v| v - v.floor()),
            Clamp(x, lo, hi) => {
                let (x, lo, hi) = (x.eval(time), lo.eval(time), hi.eval(time));
                [0, 1, 2, 3].map(|k| x[k].max(lo[k]).min(hi[k]))
            }
            Min(a, b) => {
                let (a, b) = (a.eval(time), b.eval(time));
                [0, 1, 2, 3].map(|k| a[k].min(b[k]))
            }
            Max(a, b) => {
                let (a, b) = (a.eval(time), b.eval(time));
                [0, 1, 2, 3].map(|k| a[k].max(b[k]))
            }
            Time | RealTime => [time; 4],
            TextureParameter { .. } | Texture { .. } => [0.0; 4],
        }
    }

    /// Texture object of a texture expression.
    pub fn texture(&self) -> Option<(Option<&str>, i32)> {
        match self {
            UExpr::TextureParameter { name, object } => Some((Some(name.as_str()), *object)),
            UExpr::Texture { object } => Some((None, *object)),
            _ => None,
        }
    }
}

/// Parameter values of a material instance chain (child first wins) and its base Material.
#[derive(Debug, Clone, Default)]
pub struct MaterialParams {
    pub scalars: Vec<(String, f32)>,
    pub vectors: Vec<(String, [f32; 4])>,
    pub textures: Vec<(String, i32)>,
    /// Export index of the base Material.
    pub base: Option<usize>,
}

impl MaterialParams {
    pub fn scalar(&self, name: &str) -> Option<f32> {
        self.scalars.iter().find(|(n, _)| n == name).map(|x| x.1)
    }
    pub fn vector(&self, name: &str) -> Option<[f32; 4]> {
        self.vectors.iter().find(|(n, _)| n == name).map(|x| x.1)
    }
    pub fn texture(&self, name: &str) -> Option<i32> {
        self.textures.iter().find(|(n, _)| n == name).map(|x| x.1)
    }
}

impl Package {
    pub fn material_params(&self, material: i32) -> MaterialParams {
        use crate::props::Value;
        let mut out = MaterialParams::default();
        let mut cur = material;
        for _ in 0..10 {
            if cur <= 0 {
                break;
            }
            let idx = cur as usize - 1;
            if self.export_class(&self.exports[idx]) == "Material" {
                out.base = Some(idx);
                break;
            }
            let Some(p) = self.export_own_props(idx) else { break };
            let structs = |name: &str| p.get(name).map(|v| self.array_structs(v)).unwrap_or_default();
            for s in structs("TextureParameterValues") {
                if let (Some(k), Some(t)) = (s.enum_name("ParameterName"), s.object("ParameterValue")) {
                    if out.texture(k).is_none() {
                        out.textures.push((k.to_string(), t));
                    }
                }
            }
            for s in structs("ScalarParameterValues") {
                if let (Some(k), Some(x)) = (s.enum_name("ParameterName"), s.float("ParameterValue")) {
                    if out.scalar(k).is_none() {
                        out.scalars.push((k.to_string(), x));
                    }
                }
            }
            for s in structs("VectorParameterValues") {
                let v = match s.get("ParameterValue") {
                    Some(Value::LinearColor(c)) => Some(*c),
                    Some(Value::Raw { data, .. }) if data.len() >= 16 => {
                        Some([0, 1, 2, 3].map(|k| f32::from_le_bytes(data[4 * k..4 * k + 4].try_into().unwrap())))
                    }
                    _ => None,
                };
                if let (Some(k), Some(x)) = (s.enum_name("ParameterName"), v) {
                    if out.vector(k).is_none() {
                        out.vectors.push((k.to_string(), x));
                    }
                }
            }
            cur = p.object("Parent").unwrap_or(0);
        }
        out
    }

    /// The material's own cooked FMaterialResource: Id (a shader cache record) and uniform
    /// expressions. `is_resource_id` tells whether a GUID is a record in the shader cache.
    pub fn material_resource(&self, material: i32, is_resource_id: &dyn Fn(&[u8; 16]) -> bool) -> Option<([u8; 16], UniformSet)> {
        if material <= 0 {
            return None;
        }
        let idx = material as usize - 1;
        let mut r = self.export_reader(idx);
        self.read_props(&mut r)?;
        let nat = &r.b[r.pos..];
        for o in 0..nat.len().saturating_sub(16) {
            let mut id = [0u8; 16];
            id.copy_from_slice(&nat[o..o + 16]);
            if is_resource_id(&id) {
                if let Some((u, _)) = self.read_uniform_set(nat, o) {
                    return Some((id, u));
                }
            }
        }
        None
    }
}

impl Package {
    /// The static parameter set a MaterialInstance stores in its native data (first
    /// FStaticParameterSet: BaseMaterialId, then TArray of FName, UBOOL Value, UBOOL bOverride,
    /// FGuid ExpressionGUID): (base id, [(ExpressionGUID, value)]).
    pub fn static_parameter_set(&self, material: i32) -> Option<([u8; 16], Vec<([u8; 16], bool)>)> {
        if material <= 0 {
            return None;
        }
        let mut r = self.export_reader(material as usize - 1);
        self.read_props(&mut r)?;
        let b = r.b;
        let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let mut o = r.pos + 16;
        while o + 40 < b.len() {
            let n = u(o) as usize;
            if (1..64).contains(&n)
                && o + 4 + n * 32 <= b.len()
                && (0..n).all(|k| {
                    let q = o + 4 + k * 32;
                    (u(q) as usize) < self.names.len() && u(q + 4) == 0 && u(q + 8) <= 1 && u(q + 12) <= 1
                })
            {
                let base: [u8; 16] = b[o - 16..o].try_into().ok()?;
                let sw = (0..n)
                    .map(|k| {
                        let q = o + 4 + k * 32;
                        (b[q + 16..q + 32].try_into().unwrap(), u(q + 8) != 0)
                    })
                    .collect();
                return Some((base, sw));
            }
            o += 4;
        }
        None
    }
}

impl UExpr {
    /// WGSL expression (vec4) of a uniform expression; parameters from `params` (falling back
    /// to the stored default), `time` is a WGSL f32 expression for the game time.
    pub fn to_wgsl(&self, params: &MaterialParams, time: &str) -> String {
        use UExpr::*;
        let lit = |v: [f32; 4]| format!("vec4<f32>({}, {}, {}, {})", fl(v[0]), fl(v[1]), fl(v[2]), fl(v[3]));
        match self {
            VectorParameter { name, default } => lit(params.vector(name).unwrap_or(*default)),
            ScalarParameter { name, default } => format!("vec4<f32>({})", fl(params.scalar(name).unwrap_or(*default))),
            Constant(v) => lit(*v),
            FoldedMath(a, b, op) => {
                let (a, b) = (a.to_wgsl(params, time), b.to_wgsl(params, time));
                match op {
                    0 => format!("({a} + {b})"),
                    1 => format!("({a} - {b})"),
                    2 => format!("({a} * {b})"),
                    3 => format!("({a} / {b})"),
                    _ => format!("vec4<f32>(dot({a}, {b}))"),
                }
            }
            AppendVector(a, b, n) => {
                let (a, b) = (a.to_wgsl(params, time), b.to_wgsl(params, time));
                match n {
                    1 => format!("vec4<f32>(({a}).x, ({b}).xyz)"),
                    2 => format!("vec4<f32>(({a}).xy, ({b}).xy)"),
                    3 => format!("vec4<f32>(({a}).xyz, ({b}).x)"),
                    _ => a,
                }
            }
            Sine(x, cos) => format!("{}({})", if *cos { "cos" } else { "sin" }, x.to_wgsl(params, time)),
            Periodic(x) | Frac(x) => format!("fract({})", x.to_wgsl(params, time)),
            Abs(x) => format!("abs({})", x.to_wgsl(params, time)),
            Ceil(x) => format!("ceil({})", x.to_wgsl(params, time)),
            Floor(x) => format!("floor({})", x.to_wgsl(params, time)),
            Clamp(x, lo, hi) => format!("clamp({}, {}, {})", x.to_wgsl(params, time), lo.to_wgsl(params, time), hi.to_wgsl(params, time)),
            Min(a, b) => format!("min({}, {})", a.to_wgsl(params, time), b.to_wgsl(params, time)),
            Max(a, b) => format!("max({}, {})", a.to_wgsl(params, time), b.to_wgsl(params, time)),
            Time | RealTime => format!("vec4<f32>({time})"),
            TextureParameter { .. } | Texture { .. } => "vec4<f32>(0.0)".into(),
        }
    }
}

fn fl(x: f32) -> String {
    if !x.is_finite() {
        return "0.0".into();
    }
    let s = format!("{x:?}");
    if s.contains('.') || s.contains('e') { s } else { format!("{s}.0") }
}

impl UExpr {
    /// Parameter names this expression depends on.
    pub fn param_names(&self, out: &mut Vec<String>) {
        use UExpr::*;
        match self {
            VectorParameter { name, .. } | ScalarParameter { name, .. } | TextureParameter { name, .. } => out.push(name.clone()),
            FoldedMath(a, b, _) | AppendVector(a, b, _) | Min(a, b) | Max(a, b) => {
                a.param_names(out);
                b.param_names(out);
            }
            Clamp(a, b, c) => {
                a.param_names(out);
                b.param_names(out);
                c.param_names(out);
            }
            Sine(x, _) | Periodic(x) | Abs(x) | Ceil(x) | Floor(x) | Frac(x) => x.param_names(out),
            _ => {}
        }
    }
}

impl UniformSet {
    /// Drop expressions that depend on a parameter not in `reachable` (the cooked set of a
    /// static-permutation instance follows the parent's default switches; the shader-cache
    /// permutation drops the expressions of disabled branches, keeping the relative order).
    /// SelectionColor is always present (engine-added).
    pub fn filtered(&self, reachable: &std::collections::HashSet<String>) -> UniformSet {
        let keep = |e: &UExpr| {
            let mut n = Vec::new();
            e.param_names(&mut n);
            n.iter().all(|x| x == "SelectionColor" || reachable.contains(x))
        };
        UniformSet {
            vectors: self.vectors.iter().filter(|e| keep(e)).cloned().collect(),
            scalars: self.scalars.iter().filter(|e| keep(e)).cloned().collect(),
            tex2d: self.tex2d.iter().filter(|e| keep(e)).cloned().collect(),
            cubes: self.cubes.iter().filter(|e| keep(e)).cloned().collect(),
        }
    }
}
