//! Direct3D 9 shader model 3 bytecode -> WGSL translator.
//!
//! Translates the game's compiled vertex and pixel shaders (from the UE3 shader caches)
//! instruction by instruction, keeping D3D9 semantics (rcp/rsq/log on |x|, pow via
//! exp2/log2, cmp >= 0, saturate modifiers, write masks, swizzles, source modifiers).
//! Constants, samplers and stage I/O are named through the constant table (CTAB) so the
//! renderer decides where each value comes from.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Usage {
    Position,
    BlendWeight,
    BlendIndices,
    Normal,
    PSize,
    TexCoord,
    Tangent,
    Binormal,
    TessFactor,
    PositionT,
    Color,
    Fog,
    Depth,
    Sample,
    Other(u32),
}

fn usage(u: u32) -> Usage {
    match u {
        0 => Usage::Position,
        1 => Usage::BlendWeight,
        2 => Usage::BlendIndices,
        3 => Usage::Normal,
        4 => Usage::PSize,
        5 => Usage::TexCoord,
        6 => Usage::Tangent,
        7 => Usage::Binormal,
        8 => Usage::TessFactor,
        9 => Usage::PositionT,
        10 => Usage::Color,
        11 => Usage::Fog,
        12 => Usage::Depth,
        13 => Usage::Sample,
        x => Usage::Other(x),
    }
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub usage: Usage,
    pub index: u32,
    pub reg: u32,
    pub mask: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SamplerKind {
    Tex2D,
    Cube,
    Volume,
}

#[derive(Debug, Clone)]
pub struct Shader {
    pub pixel: bool,
    pub tokens: Vec<u32>,
    /// CTAB: (register set 0=bool 1=int 2=float 3=sampler, register) -> name ("Name" or "Name[k]")
    pub ctab: BTreeMap<(u16, u32), String>,
    pub inputs: Vec<Decl>,
    pub outputs: Vec<Decl>,
    pub samplers: BTreeMap<u32, SamplerKind>,
    pub defs: BTreeMap<u32, [f32; 4]>,
    pub defis: BTreeMap<u32, [i32; 4]>,
    pub defbs: BTreeMap<u32, bool>,
    /// float constant registers read (excluding def'd ones)
    pub consts_used: BTreeSet<u32>,
    pub uses_vpos: bool,
    pub uses_vface: bool,
    pub relative_const: bool,
}

fn ctab(data: &[u8]) -> BTreeMap<(u16, u32), String> {
    let mut out = BTreeMap::new();
    let rd = |o: usize| data.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).unwrap_or(0);
    let rd16 = |o: usize| data.get(o..o + 2).map(|s| u16::from_le_bytes(s.try_into().unwrap())).unwrap_or(0);
    let n = rd(12) as usize;
    let info = rd(16) as usize;
    for i in 0..n {
        let e = info + 20 * i;
        let name_off = rd(e) as usize;
        let set = rd16(e + 4);
        let idx = rd16(e + 6) as u32;
        let cnt = rd16(e + 8) as u32;
        let end = data[name_off.min(data.len())..].iter().position(|&c| c == 0).map(|p| name_off + p).unwrap_or(name_off);
        let name = String::from_utf8_lossy(&data[name_off.min(data.len())..end]).to_string();
        for k in 0..cnt.max(1) {
            out.insert((set, idx + k), if cnt > 1 { format!("{name}[{k}]") } else { name.clone() });
        }
    }
    out
}

impl Shader {
    pub fn parse(code: &[u8]) -> Option<Shader> {
        let tokens: Vec<u32> = code.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let ver = *tokens.first()?;
        let pixel = ver >> 16 == 0xFFFF;
        if ver & 0xFFFF != 0x0300 {
            return None;
        }
        let mut s = Shader {
            pixel,
            tokens: tokens.clone(),
            ctab: BTreeMap::new(),
            inputs: vec![],
            outputs: vec![],
            samplers: BTreeMap::new(),
            defs: BTreeMap::new(),
            defis: BTreeMap::new(),
            defbs: BTreeMap::new(),
            consts_used: BTreeSet::new(),
            uses_vpos: false,
            uses_vface: false,
            relative_const: false,
        };
        let mut i = 1;
        while i < tokens.len() {
            let t = tokens[i];
            if t == 0x0000_FFFF {
                break;
            }
            let op = t & 0xFFFF;
            if op == 0xFFFE {
                let len = ((t >> 16) & 0x7FFF) as usize;
                let payload: Vec<u8> = tokens[i + 1..(i + 1 + len).min(tokens.len())].iter().flat_map(|w| w.to_le_bytes()).collect();
                if payload.len() >= 4 && &payload[..4] == b"CTAB" {
                    s.ctab = ctab(&payload[4..]);
                }
                i += 1 + len;
                continue;
            }
            let len = ((t >> 24) & 0xF) as usize;
            let args = &tokens[i + 1..(i + 1 + len).min(tokens.len())];
            match op {
                31 => {
                    // dcl
                    let d = args[0];
                    let r = args[1];
                    let (rt, rn) = (reg_type(r), r & 0x7FF);
                    let mask = (r >> 16) & 0xF;
                    if rt == 10 {
                        let k = match (d >> 27) & 0xF {
                            3 => SamplerKind::Cube,
                            4 => SamplerKind::Volume,
                            _ => SamplerKind::Tex2D,
                        };
                        s.samplers.insert(rn, k);
                    } else if rt == 17 {
                        if rn == 0 { s.uses_vpos = true } else { s.uses_vface = true }
                    } else {
                        let dd = Decl { usage: usage(d & 0x1F), index: (d >> 16) & 0xF, reg: rn, mask };
                        if rt == 1 { s.inputs.push(dd) } else { s.outputs.push(dd) }
                    }
                }
                81 => {
                    let v = [f32::from_bits(args[1]), f32::from_bits(args[2]), f32::from_bits(args[3]), f32::from_bits(args[4])];
                    s.defs.insert(args[0] & 0x7FF, v);
                }
                48 => {
                    s.defis.insert(args[0] & 0x7FF, [args[1] as i32, args[2] as i32, args[3] as i32, args[4] as i32]);
                }
                47 => {
                    s.defbs.insert(args[0] & 0x7FF, args[1] != 0);
                }
                _ => {
                    // collect constant usage
                    let mut j = 0;
                    while j < args.len() {
                        let a = args[j];
                        let rt = reg_type(a);
                        if matches!(rt, 2 | 11 | 12 | 13) && j > 0 || (j == 0 && matches!(rt, 2) && has_no_dest(op)) {
                            let n = a & 0x7FF;
                            if !s.defs.contains_key(&n) {
                                s.consts_used.insert(n);
                            }
                        }
                        if a & (1 << 13) != 0 && j > 0 {
                            if matches!(rt, 2 | 11 | 12 | 13) {
                                s.relative_const = true;
                            }
                            j += 1; // relative address token
                        }
                        j += 1;
                    }
                }
            }
            i += 1 + len;
        }
        Some(s)
    }
}

fn has_no_dest(op: u32) -> bool {
    matches!(op, 40 | 41 | 45 | 65 | 38 | 27 | 26)
}

fn reg_type(t: u32) -> u32 {
    ((t >> 28) & 7) | ((t >> 8) & 0x18)
}

/// Where translated code finds each external value.
pub trait Bindings {
    /// WGSL vec4 expression for float constant register `reg` (CTAB name if any).
    fn constant(&self, reg: u32, name: Option<&str>) -> String;
    /// (texture, sampler) identifiers for sampler register `reg`.
    fn sampler(&self, reg: u32, name: Option<&str>) -> (String, String);
    /// WGSL expression for the array of float constants used with relative addressing.
    fn const_array(&self) -> String {
        "hf_c".into()
    }
}

struct Gen<'a> {
    s: &'a Shader,
    out: String,
    indent: usize,
    nonuniform_depth: usize,
    stack: Vec<bool>,
}

const SW: [char; 4] = ['x', 'y', 'z', 'w'];

fn fmt_f(x: f32) -> String {
    if x.is_nan() {
        return "0.0".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "3.4e38".into() } else { "-3.4e38".into() };
    }
    let s = format!("{x:?}");
    if s.contains('.') || s.contains('e') { s } else { format!("{s}.0") }
}

impl<'a> Gen<'a> {
    fn line(&mut self, l: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(l);
        self.out.push('\n');
    }

    fn reg_name(&self, t: u32, rel: Option<u32>, b: &dyn Bindings) -> String {
        let rt = reg_type(t);
        let n = t & 0x7FF;
        match rt {
            0 => format!("r{n}"),
            1 => format!("v{n}"),
            2 | 11 | 12 | 13 => {
                if let Some(rel) = rel {
                    let comp = SW[((rel >> 16) & 3) as usize];
                    let base = if reg_type(rel) == 15 { "aL".to_string() } else { format!("a{}", rel & 0x7FF) };
                    format!("{}[u32(i32({n}) + i32({base}.{comp}))]", b.const_array())
                } else if let Some(d) = self.s.defs.get(&n) {
                    format!("vec4<f32>({}, {}, {}, {})", fmt_f(d[0]), fmt_f(d[1]), fmt_f(d[2]), fmt_f(d[3]))
                } else {
                    format!("c{n}")
                }
            }
            3 => format!("a{n}"),
            4 => "o_pos".into(),
            5 => format!("o_d{n}"),
            6 => format!("o{n}"),
            7 => {
                let v = self.s.defis.get(&n).copied().unwrap_or([0; 4]);
                format!("vec4<f32>({}.0, {}.0, {}.0, {}.0)", v[0], v[1], v[2], v[3])
            }
            8 => format!("oc{n}"),
            9 => "o_depth".into(),
            14 => format!("{}", self.s.defbs.get(&n).copied().unwrap_or(false)),
            15 => "aL".into(),
            17 => if n == 0 { "vpos".into() } else { "vface".into() },
            19 => format!("p{n}"),
            _ => format!("unk{rt}_{n}"),
        }
    }

    /// Source operand as a vec4 expression.
    fn src(&self, t: u32, rel: Option<u32>, b: &dyn Bindings) -> String {
        let rt = reg_type(t);
        let base = self.reg_name(t, rel, b);
        if rt == 14 {
            return base;
        }
        let sw = (t >> 16) & 0xFF;
        let swz: String = (0..4).map(|i| SW[((sw >> (2 * i)) & 3) as usize]).collect();
        let v = if swz == "xyzw" { base } else { format!("{base}.{swz}") };
        match (t >> 24) & 0xF {
            0 => v,
            1 => format!("(-{v})"),
            2 => format!("({v} - 0.5)"),
            3 => format!("(-({v} - 0.5))"),
            4 => format!("({v} * 2.0 - 1.0)"),
            5 => format!("(-({v} * 2.0 - 1.0))"),
            6 => format!("(1.0 - {v})"),
            7 => format!("({v} * 2.0)"),
            8 => format!("(-({v} * 2.0))"),
            9 => format!("({v} / {v}.z)"),
            10 => format!("({v} / {v}.w)"),
            11 => format!("abs({v})"),
            12 => format!("(-abs({v}))"),
            13 => format!("select(vec4<f32>(0.0), vec4<f32>(1.0), {v} == vec4<f32>(0.0))"),
            _ => v,
        }
    }

    fn write(&mut self, dst: u32, value: &str, b: &dyn Bindings) {
        let name = self.reg_name(dst, None, b);
        let mask = (dst >> 16) & 0xF;
        let sat = (dst >> 20) & 1 != 0;
        let v = if sat { format!("clamp({value}, vec4<f32>(0.0), vec4<f32>(1.0))") } else { value.to_string() };
        if mask == 0xF {
            self.line(&format!("{name} = {v};"));
        } else {
            self.line(&format!("{{ let _t = {v};"));
            for i in 0..4 {
                if mask & (1 << i) != 0 {
                    self.line(&format!("  {name}.{c} = _t.{c};", c = SW[i]));
                }
            }
            self.line("}");
        }
    }
}

pub struct Translated {
    /// WGSL statements forming the body; registers declared by `declarations`.
    pub body: String,
    pub declarations: String,
}

/// Translate the instruction stream. Input registers are named `v{n}` (vec4),
/// outputs `o{n}` / `o_pos` / `oc{n}` / `o_depth`, constants `c{n}` (the caller defines
/// them, see `Bindings::constant`).
pub fn translate(s: &Shader, b: &dyn Bindings) -> Translated {
    let mut g = Gen { s, out: String::new(), indent: 1, nonuniform_depth: 0, stack: Vec::new() };
    let t = &s.tokens;
    let mut temps = BTreeSet::new();
    let mut addr = BTreeSet::new();
    let mut preds = BTreeSet::new();
    // scan temps
    {
        let mut i = 1;
        while i < t.len() {
            let tok = t[i];
            if tok == 0xFFFF {
                break;
            }
            let op = tok & 0xFFFF;
            if op == 0xFFFE {
                i += 1 + ((tok >> 16) & 0x7FFF) as usize;
                continue;
            }
            let len = ((tok >> 24) & 0xF) as usize;
            if op != 31 && op != 81 && op != 47 && op != 48 {
                for &a in &t[i + 1..(i + 1 + len).min(t.len())] {
                    match reg_type(a) {
                        0 => {
                            temps.insert(a & 0x7FF);
                        }
                        3 if !s.pixel => {
                            addr.insert(a & 0x7FF);
                        }
                        19 => {
                            preds.insert(a & 0x7FF);
                        }
                        _ => {}
                    }
                }
            }
            i += 1 + len;
        }
    }
    let mut decl = String::new();
    for r in &temps {
        let _ = writeln!(decl, "    var r{r}: vec4<f32> = vec4<f32>(0.0);");
    }
    for r in &addr {
        let _ = writeln!(decl, "    var a{r}: vec4<f32> = vec4<f32>(0.0);");
    }
    for r in &preds {
        let _ = writeln!(decl, "    var p{r}: vec4<bool> = vec4<bool>(false);");
    }
    let _ = writeln!(decl, "    var aL: vec4<f32> = vec4<f32>(0.0);");
    for &c in &s.consts_used {
        let name = s.ctab.get(&(2, c)).map(|x| x.as_str());
        let _ = writeln!(decl, "    let c{c}: vec4<f32> = {};", b.constant(c, name));
    }

    let mut i = 1;
    while i < t.len() {
        let tok = t[i];
        if tok == 0x0000_FFFF {
            break;
        }
        let op = tok & 0xFFFF;
        if op == 0xFFFE {
            i += 1 + ((tok >> 16) & 0x7FFF) as usize;
            continue;
        }
        let len = ((tok >> 24) & 0xF) as usize;
        let ctrl = (tok >> 16) & 0xFF;
        let args = &t[i + 1..(i + 1 + len).min(t.len())];
        i += 1 + len;
        if matches!(op, 31 | 81 | 47 | 48 | 0) {
            continue;
        }
        // decode operands: dst (if any) + sources (with optional relative tokens)
        let mut ops: Vec<(u32, Option<u32>)> = Vec::new();
        let mut j = 0;
        let predicated = tok & (1 << 28) != 0;
        while j < args.len() {
            let a = args[j];
            let rel = if a & (1 << 13) != 0 && j + 1 < args.len() {
                j += 1;
                Some(args[j])
            } else {
                None
            };
            ops.push((a, rel));
            j += 1;
            if predicated && ops.len() == 1 && !has_no_dest(op) {
                j += 1; // predicate register token follows the destination
            }
        }
        let src = |k: usize| g.src(ops[k].0, ops[k].1, b);
        let dst = ops.first().map(|x| x.0).unwrap_or(0);
        match op {
            1 => {
                let v = src(1);
                g.write(dst, &v, b);
            }
            46 => {
                // mova: round to nearest
                let v = format!("round({})", src(1));
                g.write(dst, &v, b);
            }
            2 => { let v = format!("({} + {})", src(1), src(2)); g.write(dst, &v, b); }
            3 => { let v = format!("({} - {})", src(1), src(2)); g.write(dst, &v, b); }
            4 => { let v = format!("({} * {} + {})", src(1), src(2), src(3)); g.write(dst, &v, b); }
            5 => { let v = format!("({} * {})", src(1), src(2)); g.write(dst, &v, b); }
            6 => { let v = format!("vec4<f32>(1.0 / ({}).x)", src(1)); g.write(dst, &v, b); }
            7 => { let v = format!("vec4<f32>(inverseSqrt(abs(({}).x)))", src(1)); g.write(dst, &v, b); }
            8 => { let v = format!("vec4<f32>(dot(({}).xyz, ({}).xyz))", src(1), src(2)); g.write(dst, &v, b); }
            9 => { let v = format!("vec4<f32>(dot({}, {}))", src(1), src(2)); g.write(dst, &v, b); }
            10 => { let v = format!("min({}, {})", src(1), src(2)); g.write(dst, &v, b); }
            11 => { let v = format!("max({}, {})", src(1), src(2)); g.write(dst, &v, b); }
            12 => { let v = format!("select(vec4<f32>(0.0), vec4<f32>(1.0), {} < {})", src(1), src(2)); g.write(dst, &v, b); }
            13 => { let v = format!("select(vec4<f32>(0.0), vec4<f32>(1.0), {} >= {})", src(1), src(2)); g.write(dst, &v, b); }
            14 => { let v = format!("vec4<f32>(exp2(({}).x))", src(1)); g.write(dst, &v, b); }
            15 => { let v = format!("vec4<f32>(log2(abs(({}).x)))", src(1)); g.write(dst, &v, b); }
            18 => { let v = format!("({0} * ({1} - {2}) + {2})", src(1), src(2), src(3)); g.write(dst, &v, b); }
            19 => { let v = format!("fract({})", src(1)); g.write(dst, &v, b); }
            32 => {
                let v = format!("vec4<f32>(exp2(({}).x * log2(abs(({}).x))))", src(2), src(1));
                g.write(dst, &v, b);
            }
            33 => { let v = format!("vec4<f32>(cross(({}).xyz, ({}).xyz), 0.0)", src(1), src(2)); g.write(dst, &v, b); }
            34 => { let v = format!("sign({})", src(1)); g.write(dst, &v, b); }
            35 => { let v = format!("abs({})", src(1)); g.write(dst, &v, b); }
            36 => {
                let v = format!("vec4<f32>(({0}).xyz * inverseSqrt(max(dot(({0}).xyz, ({0}).xyz), 1e-30)), ({0}).w)", src(1));
                g.write(dst, &v, b);
            }
            37 => {
                let v = format!("vec4<f32>(cos(({0}).w), sin(({0}).w), 0.0, 0.0)", src(1));
                g.write(dst, &v, b);
            }
            88 => { let v = format!("select({2}, {1}, {0} >= vec4<f32>(0.0))", src(1), src(2), src(3)); g.write(dst, &v, b); }
            90 => { let v = format!("vec4<f32>(dot(({}).xy, ({}).xy) + ({}).x)", src(1), src(2), src(3)); g.write(dst, &v, b); }
            91 => { let v = format!("dpdx({})", src(1)); g.write(dst, &v, b); }
            92 => { let v = format!("dpdy({})", src(1)); g.write(dst, &v, b); }
            20 | 21 | 22 | 23 | 24 => {
                // mNxM: dst = src0 dotted with consecutive registers of src1
                let (cols, rows) = match op { 20 => (4, 4), 21 => (4, 3), 22 => (3, 4), 23 => (3, 3), _ => (3, 2) };
                let base = ops[2].0;
                let mut comps = vec![];
                for r in 0..rows {
                    let next = (base & !0x7FF) | ((base & 0x7FF) + r);
                    let m = g.src(next, ops[2].1, b);
                    comps.push(if cols == 4 { format!("dot({}, {})", src(1), m) } else { format!("dot(({}).xyz, ({}).xyz)", src(1), m) });
                }
                while comps.len() < 4 { comps.push("0.0".into()); }
                let v = format!("vec4<f32>({})", comps.join(", "));
                g.write(dst, &v, b);
            }
            66 | 95 | 93 => {
                // texld / texldl / texldd: dst, coord, sampler
                let sreg = ops[2].0 & 0x7FF;
                let name = s.ctab.get(&(3, sreg)).map(|x| x.as_str());
                let (tx, sm) = b.sampler(sreg, name);
                let kind = s.samplers.get(&sreg).cloned().unwrap_or(SamplerKind::Tex2D);
                let coord = src(1);
                let uv = if kind == SamplerKind::Tex2D { format!("({coord}).xy") } else { format!("({coord}).xyz") };
                // binder-provided value instead of a texture (e.g. LightAttenuationTexture computed in place)
                let v = if let Some(expr) = tx.strip_prefix('@') {
                    expr.to_string()
                } else if op == 95 {
                    format!("textureSampleLevel({tx}, {sm}, {uv}, ({coord}).w)")
                } else if op == 93 {
                    let (gx, gy) = (src(3), src(4));
                    if kind == SamplerKind::Tex2D {
                        format!("textureSampleGrad({tx}, {sm}, {uv}, ({gx}).xy, ({gy}).xy)")
                    } else {
                        format!("textureSampleGrad({tx}, {sm}, {uv}, ({gx}).xyz, ({gy}).xyz)")
                    }
                } else if ctrl == 1 {
                    // texldp: projective
                    if kind == SamplerKind::Tex2D {
                        format!("textureSample({tx}, {sm}, ({coord}).xy / ({coord}).w)")
                    } else {
                        format!("textureSample({tx}, {sm}, ({coord}).xyz / ({coord}).w)")
                    }
                } else if ctrl == 2 {
                    format!("textureSampleBias({tx}, {sm}, {uv}, ({coord}).w)")
                } else if g.nonuniform_depth > 0 || !s.pixel {
                    format!("textureSampleLevel({tx}, {sm}, {uv}, 0.0)")
                } else {
                    format!("textureSample({tx}, {sm}, {uv})")
                };
                g.write(dst, &v, b);
            }
            65 => {
                let v = src(0);
                g.line(&format!("if (any(({v}).xyz < vec3<f32>(0.0))) {{ discard; }}"));
            }
            40 => {
                // if b#
                let c = src(0);
                g.line(&format!("if ({c}) {{"));
                g.indent += 1;
                g.stack.push(false);
            }
            41 => {
                let (a, c) = (src(0), src(1));
                let cmp = match ctrl & 7 { 1 => ">", 2 => "==", 3 => ">=", 4 => "<", 5 => "!=", _ => "<=" };
                // uniform if both operands are constants
                let uniform = matches!(reg_type(ops[0].0), 2) && matches!(reg_type(ops[1].0), 2);
                g.line(&format!("if (({a}).x {cmp} ({c}).x) {{"));
                g.indent += 1;
                if !uniform { g.nonuniform_depth += 1; }
                g.stack.push(!uniform);
            }
            42 => {
                g.indent -= 1;
                g.line("} else {");
                g.indent += 1;
            }
            43 => {
                g.indent -= 1;
                g.line("}");
                if g.stack.pop() == Some(true) { g.nonuniform_depth -= 1; }
            }
            38 => {
                // rep i#
                let c = src(0);
                g.line(&format!("for (var _rep: i32 = 0; _rep < i32(({c}).x); _rep++) {{"));
                g.indent += 1;
            }
            39 | 29 => {
                g.indent -= 1;
                g.line("}");
            }
            27 => {
                // loop aL, i#
                let c = src(1);
                g.line(&format!("aL = vec4<f32>(({c}).y);"));
                g.line(&format!("for (var _lp: i32 = 0; _lp < i32(({c}).x); _lp++) {{"));
                g.indent += 1;
                g.line(&format!("let _step = ({c}).z;"));
            }
            44 => g.line("break;"),
            45 => {
                let (a, c) = (src(0), src(1));
                let cmp = match ctrl & 7 { 1 => ">", 2 => "==", 3 => ">=", 4 => "<", 5 => "!=", _ => "<=" };
                g.line(&format!("if (({a}).x {cmp} ({c}).x) {{ break; }}"));
            }
            94 => {
                let (a, c) = (src(1), src(2));
                let cmp = match ctrl & 7 { 1 => ">", 2 => "==", 3 => ">=", 4 => "<", 5 => "!=", _ => "<=" };
                let name = g.reg_name(dst, None, b);
                g.line(&format!("{name} = {a} {cmp} {c};"));
            }
            28 => g.line("// ret"),
            _ => g.line(&format!("// unsupported opcode {op}")),
        }
    }
    Translated { body: g.out, declarations: decl }
}
