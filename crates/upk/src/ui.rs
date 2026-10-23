//! UE3 UI (UIScene) data: localisation (Coalesced_<lang>.bin), fonts, skins/styles and widget trees.
//!
//! Layout semantics from HOMEFRONT.exe (see MODLOG "Menus"):
//! * UIScreenValue_Bounds face values are positions: base + Value * scale (FUN_00ebb610):
//!   EVALPOS 1 PixelViewport (base viewport origin, scale 1), 4 PercentageViewport (scale = viewport
//!   width/height), 3 PixelOwner / 5 PercentageOwner (base = owner's Left/Top for every face,
//!   scale 1 / owner extent), 2 PixelScene / 6 PercentageScene (scene, = viewport here).
//! * Docked faces (UIDockingSet, FUN_00eab0c0): target face position + padding.
//! * Text auto scale (FUN_0074d970): min(vw / 800, vh / 600).

use crate::props::{Props, Rd, Value};
use crate::Package;
use std::collections::HashMap;

// ---------------------------------------------------------------- localisation

/// Strings of all .<lang> files in a Coalesced_<lang>.bin: key "file.section.key" (lowercase).
pub struct Loc {
    pub map: HashMap<String, String>,
    /// .ini entries of the Coalesced file: "file.section.key" (lowercase, file stem) -> values
    /// in order (array entries repeat the key)
    pub ini: HashMap<String, Vec<String>>,
}

impl Loc {
    pub fn load(path: &std::path::Path) -> Option<Loc> {
        let b = std::fs::read(path).ok()?;
        let mut r = Rd::new(&b);
        let fstr = |r: &mut Rd| -> Option<String> {
            let n = r.i32()?;
            if n == 0 {
                return Some(String::new());
            }
            if n > 0 {
                let s = r.bytes(n as usize)?;
                Some(s[..s.len() - 1].iter().map(|&c| c as char).collect())
            } else {
                let s = r.bytes((-n) as usize * 2)?;
                let u: Vec<u16> = s.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                Some(String::from_utf16_lossy(&u[..u.len() - 1]))
            }
        };
        let mut map = HashMap::new();
        let mut ini: HashMap<String, Vec<String>> = HashMap::new();
        let nf = r.i32()?;
        for _ in 0..nf {
            let name = fstr(&mut r)?;
            let file = name.rsplit(['\\', '/']).next().unwrap_or("").to_string();
            let stem = file.rsplit_once('.').map(|x| x.0).unwrap_or(&file).to_ascii_lowercase();
            let is_loc = !name.to_ascii_lowercase().ends_with(".ini");
            let ns = r.i32()?;
            for _ in 0..ns {
                let sec = fstr(&mut r)?;
                let nk = r.i32()?;
                for _ in 0..nk {
                    let k = fstr(&mut r)?;
                    let v = fstr(&mut r)?;
                    if is_loc {
                        let v = v.trim_matches('"').to_string();
                        map.insert(format!("{stem}.{}.{}", sec.to_ascii_lowercase(), k.to_ascii_lowercase()), v);
                    } else {
                        ini.entry(format!("{stem}.{}.{}", sec.to_ascii_lowercase(), k.to_ascii_lowercase())).or_default().push(v);
                    }
                }
            }
        }
        Some(Loc { map, ini })
    }

    /// Localized string by "file.section.key" (PC variant first).
    pub fn get(&self, key: &str) -> Option<&str> {
        let k = key.to_ascii_lowercase();
        self.map.get(&format!("{k}__pc")).or_else(|| self.map.get(&k)).map(|s| s.as_str())
    }

    /// Replace `<Strings:file.section.key>` markup; other markup is left out.
    pub fn resolve(&self, markup: &str) -> String {
        let mut out = String::new();
        let mut rest = markup;
        while let Some(i) = rest.find('<') {
            out.push_str(&rest[..i]);
            let Some(j) = rest[i..].find('>') else {
                out.push_str(&rest[i..]);
                return out;
            };
            let tag = &rest[i + 1..i + j];
            // the prefix is case-insensitive in UE3 (PC_U_KeyBindings' land tab has "<STrings:...>")
            let strings = tag.get(..8).filter(|p| p.eq_ignore_ascii_case("Strings:")).map(|_| &tag[8..]);
            if let Some(k) = strings {
                // PC builds use the "<key>__pc" variant when there is one (trace frame 1000:
                // "ANGEL ISLAND", not the console "¶ANGEL ISLAND")
                let k = k.to_ascii_lowercase();
                if let Some(v) = self.map.get(&format!("{k}__pc")).or_else(|| self.map.get(&k)) {
                    out.push_str(v);
                }
            }
            rest = &rest[i + j + 1..];
        }
        out.push_str(rest);
        out
    }
}

// ---------------------------------------------------------------- fonts

#[derive(Debug, Clone, Copy, Default)]
pub struct FontChar {
    pub u: i32,
    pub v: i32,
    pub w: i32,
    pub h: i32,
    pub tex: u8,
    pub voff: i32,
}

#[derive(Debug, Clone, Default)]
pub struct Font {
    pub chars: Vec<FontChar>,
    /// Texture2D object references (in the font's package)
    pub textures: Vec<i32>,
    pub remap: HashMap<u16, u16>,
    pub is_remapped: bool,
    pub kerning: i32,
}

impl Font {
    pub fn glyph(&self, c: char) -> Option<&FontChar> {
        let code = c as u32;
        let idx = if self.is_remapped { *self.remap.get(&(code as u16))? as usize } else { code as usize };
        self.chars.get(idx)
    }
}

impl Package {
    pub fn read_font(&self, idx: usize) -> Option<Font> {
        let mut r = self.export_reader(idx);
        let pr = self.read_props(&mut r)?;
        let mut f = Font { kerning: pr.int("Kerning").unwrap_or(0), is_remapped: pr.int("IsRemapped").unwrap_or(0) != 0, ..Default::default() };
        if let Some(Value::Raw { data, .. }) = pr.get("Characters") {
            let mut d = Rd::new(data);
            let n = d.i32()?;
            for _ in 0..n {
                let (u, v, w, h) = (d.i32()?, d.i32()?, d.i32()?, d.i32()?);
                let tex = d.u8()?;
                let voff = d.i32()?;
                f.chars.push(FontChar { u, v, w, h, tex, voff });
            }
        }
        if let Some(t) = pr.get("Textures") {
            f.textures = self.array_objects(t);
        }
        // native: TMap<WORD, WORD> CharRemap
        let n = r.i32().unwrap_or(0);
        for _ in 0..n.max(0) {
            let (Some(k), Some(v)) = (r.u16(), r.u16()) else { break };
            f.remap.insert(k, v);
        }
        Some(f)
    }
}

// ---------------------------------------------------------------- property trees across packages

/// Global object reference: (package index, export or import index of that package).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GRef {
    pub pkg: usize,
    pub idx: i32,
}

#[derive(Debug, Clone)]
pub enum PVal {
    Int(i32),
    Float(f32),
    Bool(bool),
    Byte(u8),
    Enum(String),
    Name(String),
    Str(String),
    Obj(Option<GRef>),
    Vector([f32; 3]),
    Rotator([i32; 3]),
    Color([u8; 4]),
    LColor([f32; 4]),
    Struct(Tree),
    /// atomic struct / array / other raw payload (package needed to decode names)
    Raw { pkg: usize, ty: String, sname: Option<String>, data: Vec<u8> },
}

#[derive(Debug, Clone, Default)]
pub struct Tree(pub Vec<(String, i32, PVal)>);

impl Tree {
    pub fn get_i(&self, name: &str, index: i32) -> Option<&PVal> {
        self.0.iter().find(|(n, i, _)| n == name && *i == index).map(|x| &x.2)
    }
    pub fn get(&self, name: &str) -> Option<&PVal> {
        self.get_i(name, 0)
    }
    pub fn f32_i(&self, name: &str, i: i32) -> Option<f32> {
        match self.get_i(name, i)? {
            PVal::Float(v) => Some(*v),
            PVal::Int(v) => Some(*v as f32),
            _ => None,
        }
    }
    pub fn f32(&self, name: &str) -> Option<f32> {
        self.f32_i(name, 0)
    }
    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            PVal::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn enum_i(&self, name: &str, i: i32) -> Option<&str> {
        match self.get_i(name, i)? {
            PVal::Enum(s) | PVal::Name(s) => Some(s),
            _ => None,
        }
    }
    pub fn i32(&self, name: &str) -> Option<i32> {
        match self.get(name)? {
            PVal::Int(v) => Some(*v),
            _ => None,
        }
    }
    pub fn str(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            PVal::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn obj_i(&self, name: &str, i: i32) -> Option<GRef> {
        match self.get_i(name, i)? {
            PVal::Obj(o) => *o,
            _ => None,
        }
    }
    pub fn obj(&self, name: &str) -> Option<GRef> {
        self.obj_i(name, 0)
    }
    pub fn sub_i(&self, name: &str, i: i32) -> Option<&Tree> {
        match self.get_i(name, i)? {
            PVal::Struct(t) => Some(t),
            _ => None,
        }
    }
    pub fn sub(&self, name: &str) -> Option<&Tree> {
        self.sub_i(name, 0)
    }
    pub fn lcolor(&self, name: &str) -> Option<[f32; 4]> {
        match self.get(name)? {
            PVal::LColor(c) => Some(*c),
            _ => None,
        }
    }
    /// Raw atomic struct of n floats (Vector2D etc.)
    pub fn floats(&self, name: &str, n: usize) -> Option<Vec<f32>> {
        match self.get(name)? {
            PVal::Raw { data, .. } if data.len() >= 4 * n => Some(data.chunks_exact(4).take(n).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()),
            _ => None,
        }
    }
    /// Deep merge: entries of `base` missing in self are added, structs merged element-wise.
    pub fn merge(&mut self, base: &Tree) {
        for (n, i, v) in &base.0 {
            match self.0.iter_mut().find(|(m, j, _)| m == n && j == i) {
                Some((_, _, PVal::Struct(own))) => {
                    if let PVal::Struct(b) = v {
                        own.merge(b);
                    }
                }
                Some(_) => {}
                None => self.0.push((n.clone(), *i, v.clone())),
            }
        }
    }
}

/// Packages used by the UI plus a path index over their exports.
pub struct UiWorld {
    pub pkgs: Vec<Package>,
    pub names: Vec<String>,
    by_path: HashMap<String, (usize, usize)>,
    cache: std::cell::RefCell<HashMap<GRef, std::rc::Rc<Tree>>>,
}

impl UiWorld {
    /// `pkgs`: (package stem, package)
    pub fn new(list: Vec<(String, Package)>) -> UiWorld {
        let mut by_path = HashMap::new();
        let mut pkgs = Vec::new();
        let mut names = Vec::new();
        for (pi, (name, p)) in list.into_iter().enumerate() {
            let pre = format!("{}.", name.to_ascii_lowercase());
            for i in 0..p.exports.len() {
                let path = p.object_path(i as i32 + 1).to_ascii_lowercase();
                by_path.entry(path.clone()).or_insert((pi, i));
                if !path.starts_with(&pre) {
                    by_path.entry(format!("{pre}{path}")).or_insert((pi, i));
                }
            }
            names.push(name);
            pkgs.push(p);
        }
        UiWorld { pkgs, names, by_path, cache: Default::default() }
    }

    pub fn find(&self, path: &str) -> Option<GRef> {
        self.by_path.get(&path.to_ascii_lowercase()).map(|&(p, i)| GRef { pkg: p, idx: i as i32 + 1 })
    }

    /// Imports are resolved to exports of loaded packages where possible.
    pub fn gref(&self, pkg: usize, idx: i32) -> Option<GRef> {
        if idx == 0 {
            return None;
        }
        if idx < 0 {
            let path = self.pkgs[pkg].object_path(idx);
            return Some(self.find(&path).unwrap_or(GRef { pkg, idx }));
        }
        Some(GRef { pkg, idx })
    }

    pub fn path(&self, r: GRef) -> String {
        self.pkgs[r.pkg].object_path(r.idx)
    }
    pub fn name(&self, r: GRef) -> String {
        self.pkgs[r.pkg].object_name(r.idx)
    }
    pub fn class(&self, r: GRef) -> String {
        if r.idx > 0 {
            let p = &self.pkgs[r.pkg];
            p.export_class(&p.exports[r.idx as usize - 1])
        } else if r.idx < 0 {
            let p = &self.pkgs[r.pkg];
            p.name(p.imports[(-r.idx) as usize - 1].class_name)
        } else {
            String::new()
        }
    }

    fn convert(&self, pkg: usize, pr: &Props) -> Tree {
        let p = &self.pkgs[pkg];
        let mut t = Tree::default();
        for x in &pr.0 {
            let v = match &x.value {
                Value::Int(v) => PVal::Int(*v),
                Value::Float(v) => PVal::Float(*v),
                Value::Bool(v) => PVal::Bool(*v),
                Value::Byte(v) => PVal::Byte(*v),
                Value::Enum(s) => PVal::Enum(s.clone()),
                Value::Name(s) => PVal::Name(s.clone()),
                Value::Str(s) => PVal::Str(s.clone()),
                Value::Object(o) => PVal::Obj(self.gref(pkg, *o)),
                Value::Vector(v) => PVal::Vector(*v),
                Value::Rotator(v) => PVal::Rotator(*v),
                Value::Color(c) => PVal::Color(*c),
                Value::LinearColor(c) => PVal::LColor(*c),
                Value::Raw { type_name, struct_name, data } => {
                    let mut as_struct = None;
                    if type_name == "StructProperty" {
                        let mut rd = Rd::new(data);
                        if let Some(sp) = p.read_props(&mut rd) {
                            if rd.pos == data.len() {
                                as_struct = Some(self.convert(pkg, &sp));
                            }
                        }
                    }
                    match as_struct {
                        Some(s) => PVal::Struct(s),
                        None => PVal::Raw { pkg, ty: type_name.clone(), sname: struct_name.clone(), data: data.clone() },
                    }
                }
            };
            t.0.push((x.name.clone(), x.index, v));
        }
        t
    }

    /// Class default object of a class (by class object reference).
    fn class_default(&self, obj: GRef) -> Option<GRef> {
        let p = &self.pkgs[obj.pkg];
        let class_ref = if obj.idx > 0 { p.exports[obj.idx as usize - 1].class_index } else { return None };
        if class_ref == 0 {
            return None;
        }
        let cname = p.object_name(class_ref);
        let cpkg = if class_ref < 0 {
            let im = &p.imports[(-class_ref) as usize - 1];
            p.object_name(im.outer_index)
        } else {
            self.names[obj.pkg].clone()
        };
        self.find(&format!("{cpkg}.Default__{cname}")).or_else(|| ["Engine", "GCGame", "Core"].iter().find_map(|k| self.find(&format!("{k}.Default__{cname}"))))
    }

    /// Properties of an object merged with its archetype / class default chain.
    pub fn props(&self, r: GRef) -> std::rc::Rc<Tree> {
        if let Some(t) = self.cache.borrow().get(&r) {
            return t.clone();
        }
        let mut t = Tree::default();
        if r.idx > 0 {
            let p = &self.pkgs[r.pkg];
            let i = r.idx as usize - 1;
            let mut rd = p.export_reader(i);
            if let Some(pr) = p.read_props(&mut rd) {
                t = self.convert(r.pkg, &pr);
            }
            let arch = p.exports[i].archetype_index;
            let base = if arch != 0 { self.gref(r.pkg, arch) } else { self.class_default(r) };
            if let Some(b) = base.filter(|b| *b != r && b.idx > 0) {
                let bt = self.props(b);
                t.merge(&bt);
            }
        }
        let rc = std::rc::Rc::new(t);
        self.cache.borrow_mut().insert(r, rc.clone());
        rc
    }

    /// Elements of an object array property.
    /// Object array (or a single object property, e.g. CommandButton.GCCommand[0]).
    pub fn objects(&self, v: Option<&PVal>) -> Vec<GRef> {
        if let Some(PVal::Obj(o)) = v {
            return o.iter().copied().collect();
        }
        let Some(PVal::Raw { pkg, data, .. }) = v else { return vec![] };
        let mut d = Rd::new(data);
        let n = d.i32().unwrap_or(0).max(0);
        (0..n).filter_map(|_| d.i32()).filter_map(|o| self.gref(*pkg, o)).collect()
    }

    /// Elements of a name / enum array property (count, then an FName per element).
    pub fn name_list(&self, v: Option<&PVal>) -> Vec<String> {
        let Some(PVal::Raw { pkg, data, .. }) = v else { return vec![] };
        let Some(p) = self.pkgs.get(*pkg) else { return vec![] };
        let mut d = Rd::new(data);
        let n = d.i32().unwrap_or(0).max(0);
        (0..n).filter_map(|_| Some(p.name(crate::FName { index: d.i32()?, number: d.i32()? }))).collect()
    }

    /// Elements of a struct array property.
    pub fn structs(&self, v: Option<&PVal>) -> Vec<Tree> {
        let Some(PVal::Raw { pkg, data, .. }) = v else { return vec![] };
        let p = &self.pkgs[*pkg];
        let mut d = Rd::new(data);
        let n = d.i32().unwrap_or(0).max(0);
        let mut out = Vec::new();
        for _ in 0..n {
            match p.read_props(&mut d) {
                Some(pr) => out.push(self.convert(*pkg, &pr)),
                None => break,
            }
        }
        out
    }

    /// Native data after the tagged properties of an export.
    pub fn native_tail(&self, r: GRef) -> Option<&[u8]> {
        if r.idx <= 0 {
            return None;
        }
        let p = &self.pkgs[r.pkg];
        let i = r.idx as usize - 1;
        let mut rd = p.export_reader(i);
        p.read_props(&mut rd)?;
        let b = p.export_bytes(&p.exports[i]);
        Some(&b[rd.pos..])
    }
}

// ---------------------------------------------------------------- widgets and layout

pub const FACE_L: usize = 0;
pub const FACE_T: usize = 1;
pub const FACE_R: usize = 2;
pub const FACE_B: usize = 3;

fn face_index(s: &str) -> Option<usize> {
    match s {
        "UIFACE_Left" => Some(0),
        "UIFACE_Top" => Some(1),
        "UIFACE_Right" => Some(2),
        "UIFACE_Bottom" => Some(3),
        _ => None,
    }
}

fn eval_type(s: &str) -> u8 {
    match s {
        "EVALPOS_PixelViewport" => 1,
        "EVALPOS_PixelScene" => 2,
        "EVALPOS_PixelOwner" => 3,
        "EVALPOS_PercentageViewport" => 4,
        "EVALPOS_PercentageOwner" => 5,
        "EVALPOS_PercentageScene" => 6,
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Dock {
    pub target: Option<usize>,
    pub face: usize,
    pub pad: f32,
    /// UIPADDINGEVAL: 0 Pixels, 1 PercentTarget, 2 PercentOwner, 3 PercentScene, 4 PercentViewport
    pub pad_type: u8,
}

#[derive(Debug, Clone)]
pub struct Widget {
    pub obj: GRef,
    pub class: String,
    pub tag: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub value: [f32; 4],
    pub eval: [u8; 4],
    pub dock: [Option<Dock>; 4],
    pub hidden: bool,
    pub props: std::rc::Rc<Tree>,
    /// resolved face positions (pixels)
    pub rect: [f32; 4],
}

pub struct Scene {
    pub obj: GRef,
    pub widgets: Vec<Widget>,
}

impl UiWorld {
    pub fn load_scene(&self, root: GRef) -> Scene {
        let mut s = Scene { obj: root, widgets: Vec::new() };
        let mut map: HashMap<GRef, usize> = HashMap::new();
        self.add_widget(&mut s, &mut map, root, None);
        // docking targets
        for wi in 0..s.widgets.len() {
            let pr = s.widgets[wi].props.clone();
            let Some(dt) = pr.sub("DockTargets") else { continue };
            let pad = dt.sub("DockPadding");
            for f in 0..4 {
                let Some(face) = dt.enum_i("TargetFace", f as i32).and_then(face_index) else { continue };
                let target = dt.obj_i("TargetWidget", f as i32);
                let t = match target {
                    // docking to itself (or none) = docking to the scene / viewport
                    Some(t) if t == s.widgets[wi].obj => Some(0),
                    Some(t) => map.get(&t).copied(),
                    None => Some(0),
                };
                let (pv, pt) = match pad {
                    Some(p) => (
                        p.f32_i("PaddingValue", f as i32).unwrap_or(0.0),
                        match p.enum_i("PaddingScaleType", f as i32).unwrap_or("UIPADDINGEVAL_Pixels") {
                            "UIPADDINGEVAL_PercentTarget" => 1,
                            "UIPADDINGEVAL_PercentOwner" => 2,
                            "UIPADDINGEVAL_PercentScene" => 3,
                            "UIPADDINGEVAL_PercentViewport" => 4,
                            _ => 0,
                        },
                    ),
                    None => (0.0, 0),
                };
                // a face can only dock to a face of the same orientation (U_HUDPrefs
                // SubtitlesCheckbox: Top -> UIFACE_Right without a target; UE3's editor does not
                // allow it, the entry is stale): such an entry leaves the face undocked
                if f % 2 != face % 2 {
                    continue;
                }
                s.widgets[wi].dock[f] = Some(Dock { target: t, face, pad: pv, pad_type: pt });
            }
        }
        s
    }

    fn add_widget(&self, s: &mut Scene, map: &mut HashMap<GRef, usize>, r: GRef, parent: Option<usize>) -> usize {
        let pr = self.props(r);
        let mut value = [0.0, 0.0, 1.0, 1.0];
        let mut eval = [5u8; 4];
        if let Some(pos) = pr.sub("Position") {
            for f in 0..4 {
                if let Some(v) = pos.f32_i("Value", f as i32) {
                    value[f] = v;
                }
                if let Some(e) = pos.enum_i("ScaleType", f as i32) {
                    eval[f] = eval_type(e);
                }
            }
        }
        let wi = s.widgets.len();
        s.widgets.push(Widget {
            obj: r,
            class: self.class(r),
            tag: match pr.get("WidgetTag").or_else(|| pr.get("SceneTag")) {
                Some(PVal::Name(n)) => n.clone(),
                _ => self.name(r),
            },
            parent,
            children: Vec::new(),
            value,
            eval,
            dock: [None; 4],
            hidden: pr.bool("bHidden").unwrap_or(false),
            props: pr.clone(),
            rect: [0.0; 4],
        });
        map.insert(r, wi);
        for c in self.objects(pr.get("Children")) {
            let ci = self.add_widget(s, map, c, Some(wi));
            s.widgets[wi].children.push(ci);
        }
        wi
    }
}

impl Scene {
    /// Resolve all faces for a viewport (UE3 positions + docking, see module doc).
    pub fn layout(&mut self, vw: f32, vh: f32) {
        let n = self.widgets.len();
        let mut state = vec![[0u8; 4]; n]; // 0 todo, 1 busy, 2 done
        self.widgets[0].rect = [0.0, 0.0, vw, vh];
        state[0] = [2; 4];
        for wi in 1..n {
            for f in 0..4 {
                self.face(wi, f, vw, vh, &mut state);
            }
        }
        // UIComboBox: the button is a square at the right end, the edit box ends at it
        // (trace frame 1000: GameTypeComboBox edit 199.0..412.5, button 412.5..438.1 = height 25.6)
        for wi in 0..n {
            if self.widgets[wi].class != "UIComboBox" {
                continue;
            }
            let find = |name: &str| self.widgets[wi].props.obj(name).and_then(|g| self.widgets.iter().position(|w| w.obj == g));
            if let (Some(b), Some(e)) = (find("ComboButton"), find("ComboEditbox")) {
                let r = self.widgets[b].rect;
                let x0 = r[2] - (r[3] - r[1]);
                self.widgets[b].rect[0] = x0;
                self.widgets[e].rect[2] = x0;
            }
        }
    }

    fn face(&mut self, wi: usize, f: usize, vw: f32, vh: f32, st: &mut Vec<[u8; 4]>) -> f32 {
        if st[wi][f] == 2 || st[wi][f] == 1 {
            return self.widgets[wi].rect[f];
        }
        st[wi][f] = 1;
        let w = self.widgets[wi].clone();
        let horiz = f % 2 == 0;
        let v = if let Some(d) = w.dock[f] {
            let t = d.target.unwrap_or(0);
            let tp = self.face(t, d.face, vw, vh, st);
            let pad = match d.pad_type {
                0 => d.pad,
                4 | 3 => d.pad * if horiz { vw } else { vh },
                1 => {
                    let (a, b) = if horiz { (0, 2) } else { (1, 3) };
                    d.pad * (self.face(t, b, vw, vh, st) - self.face(t, a, vw, vh, st))
                }
                _ => {
                    let o = w.parent.unwrap_or(0);
                    let (a, b) = if horiz { (0, 2) } else { (1, 3) };
                    d.pad * (self.face(o, b, vw, vh, st) - self.face(o, a, vw, vh, st))
                }
            };
            tp + pad
        } else {
            let o = w.parent.unwrap_or(0);
            let (a, b) = if horiz { (0, 2) } else { (1, 3) };
            // FUN_00ebb610: owner/scene types use the owner's (scene's) Left/Top as base for the
            // Left/Top faces but the widget's own Left/Top for Right/Bottom (= extents), scaled by
            // the owner's (scene's) extent for the percentage types
            let own_base = |s: &mut Self, st: &mut Vec<[u8; 4]>| if f >= 2 { Some(s.face(wi, f - 2, vw, vh, st)) } else { None };
            let (base, scale) = match w.eval[f] {
                1 => (0.0, 1.0),
                4 => (0.0, if horiz { vw } else { vh }),
                2 => (own_base(self, st).unwrap_or(0.0), 1.0),
                6 => (own_base(self, st).unwrap_or(0.0), if horiz { vw } else { vh }),
                3 => {
                    let b0 = match own_base(self, st) { Some(v) => v, None => self.face(o, a, vw, vh, st) };
                    (b0, 1.0)
                }
                5 => {
                    let oa = self.face(o, a, vw, vh, st);
                    let ext = self.face(o, b, vw, vh, st) - oa;
                    (own_base(self, st).unwrap_or(oa), ext)
                }
                _ => (0.0, 1.0),
            };
            base + w.value[f] * scale
        };
        self.widgets[wi].rect[f] = v;
        st[wi][f] = 2;
        v
    }
}

// ---------------------------------------------------------------- styles and draw list

/// UIState classes a widget can be in (style lookup key = Default__UIState_<name>).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiState {
    Enabled,
    Focused,
    Active,
    Pressed,
    Disabled,
}

impl UiState {
    fn default_name(self) -> &'static str {
        match self {
            UiState::Enabled => "Default__UIState_Enabled",
            UiState::Focused => "Default__UIState_Focused",
            UiState::Active => "Default__UIState_Active",
            UiState::Pressed => "Default__UIState_Pressed",
            UiState::Disabled => "Default__UIState_Disabled",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Quad {
    /// rotation about the rect centre (radians, clockwise on screen)
    pub rot: f32,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    /// Texture2D (or material) object
    pub tex: Option<GRef>,
    pub color: [f32; 4],
}

#[derive(Debug, Clone)]
pub struct TextStyle {
    pub font: Option<GRef>,
    pub color: [f32; 4],
    pub scale: [f32; 2],
    pub spacing: [f32; 2],
    /// 0 left/top, 1 center, 2 right/bottom
    pub align: [u8; 2],
    pub autoscale: bool,
    /// StylePadding (horizontal, vertical), scaled like the glyphs
    pub padding: [f32; 2],
    /// ClipMode CLIP_Wrap: word wrap inside the rect
    pub wrap: bool,
}

fn align_of(s: &str) -> u8 {
    match s {
        "UIALIGN_Center" => 1,
        "UIALIGN_Right" => 2,
        _ => 0,
    }
}

fn style_id(t: &Tree) -> Option<[i32; 4]> {
    let mut id = [0; 4];
    for (k, n) in ["A", "B", "C", "D"].iter().enumerate() {
        id[k] = match t.get(n)? {
            PVal::Int(v) => *v,
            _ => return None,
        };
    }
    Some(id)
}

pub struct Ui {
    pub world: UiWorld,
    pub loc: Loc,
    styles: HashMap<[i32; 4], GRef>,
    style_tags: HashMap<String, GRef>,
    fonts: std::cell::RefCell<HashMap<GRef, std::rc::Rc<Font>>>,
}

impl Ui {
    pub fn new(world: UiWorld, loc: Loc) -> Ui {
        let mut styles = HashMap::new();
        let mut style_tags = HashMap::new();
        for (pi, p) in world.pkgs.iter().enumerate() {
            for (i, e) in p.exports.iter().enumerate() {
                if p.export_class(e) != "UIStyle" {
                    continue;
                }
                let r = GRef { pkg: pi, idx: i as i32 + 1 };
                let sp = world.props(r);
                if let Some(id) = sp.sub("StyleID").and_then(style_id) {
                    styles.entry(id).or_insert(r);
                }
                if let Some(t) = sp.enum_i("StyleTag", 0) {
                    // the active skin is PC_Skins.PCskin (derived from DefaultUISkin.DefaultSkin):
                    // a tag it redefines wins over the base skin's (ButtonBackground: PCskin has the
                    // grey button art, DefaultSkin EngineResources.DefaultTexture - the camo
                    // checker on the pause menu's "Abstimmung einleiten" / spawn "Trupp-Browser")
                    let base = |g: &GRef| world.pkgs[g.pkg].object_path(g.idx).starts_with("DefaultSkin.");
                    match style_tags.get(t) {
                        Some(old) if base(old) && !base(&r) => {
                            style_tags.insert(t.to_string(), r);
                        }
                        Some(_) => {}
                        None => {
                            style_tags.insert(t.to_string(), r);
                        }
                    }
                }
            }
        }
        Ui { world, loc, styles, style_tags, fonts: Default::default() }
    }

    pub fn font(&self, r: GRef) -> Option<std::rc::Rc<Font>> {
        if let Some(f) = self.fonts.borrow().get(&r) {
            return Some(f.clone());
        }
        // texture references stay package-local (resolved with the font's package)
        let f = self.world.pkgs[r.pkg].read_font(r.idx as usize - 1)?;
        let rc = std::rc::Rc::new(f);
        self.fonts.borrow_mut().insert(r, rc.clone());
        Some(rc)
    }

    /// UIStyleReference -> UIStyle: AssignedStyleID, else DefaultStyleTag.
    pub fn style_ref(&self, r: &Tree) -> Option<GRef> {
        if let Some(id) = r.sub("AssignedStyleID").and_then(style_id) {
            if id != [0; 4] {
                if let Some(s) = self.styles.get(&id) {
                    return Some(*s);
                }
            }
        }
        r.enum_i("DefaultStyleTag", 0).and_then(|t| self.style_tags.get(t).copied())
    }

    /// Style data object of a UIStyle for a state (native StateDataMap), falling back to Enabled.
    fn style_data(&self, style: GRef, state: UiState) -> Option<GRef> {
        let tail = self.world.native_tail(style)?;
        let mut d = Rd::new(tail);
        let n = d.i32()?;
        let mut found = None;
        let mut enabled = None;
        for _ in 0..n {
            let (k, v) = (d.i32()?, d.i32()?);
            let kn = self.world.pkgs[style.pkg].object_name(k);
            if kn == state.default_name() {
                found = self.world.gref(style.pkg, v);
            }
            if kn == UiState::Enabled.default_name() {
                enabled = self.world.gref(style.pkg, v);
            }
        }
        // state data with bEnabled = false is not used (NavButtonBG.UIStyle_Image_5/6)
        let found = found.filter(|f| self.world.props(*f).bool("bEnabled") != Some(false));
        found.or(enabled)
    }

    /// Text style data (UIStyle_Text) from a style reference (Combo -> TextStyle).
    fn text_data(&self, data: GRef, state: UiState, depth: u32) -> Option<GRef> {
        if depth > 4 {
            return None;
        }
        match self.world.class(data).as_str() {
            "UIStyle_Text" => Some(data),
            "UIStyle_Combo" => {
                let p = self.world.props(data);
                let ts = p.sub("TextStyle")?;
                if let Some(c) = ts.obj("CustomStyleData") {
                    return Some(c);
                }
                let src = self.styles.get(&style_id(ts.sub("SourceStyleID")?)?).copied()?;
                let d = self.style_data(src, state)?;
                self.text_data(d, state, depth + 1)
            }
            _ => None,
        }
    }

    /// Effective text style of a UIComp_DrawString.
    pub fn text_style(&self, comp: GRef, state: UiState) -> TextStyle {
        let cp = self.world.props(comp);
        self.text_style_tree(&cp, state)
    }

    /// Text style of a skin style by tag (runtime-created labels such as cookie crumbs).
    pub fn tag_text_style(&self, tag: &str, state: UiState) -> TextStyle {
        let t = Tree(vec![("StringStyle".into(), 0, PVal::Struct(Tree(vec![("DefaultStyleTag".into(), 0, PVal::Name(tag.into()))])))]);
        self.text_style_tree(&t, state)
    }

    pub fn text_style_tree(&self, cp: &Tree, state: UiState) -> TextStyle {
        let mut st = TextStyle { font: None, color: [1.0; 4], scale: [1.0, 1.0], spacing: [0.0, 0.0], align: [0, 1], autoscale: false, padding: [0.0; 2], wrap: false };
        let base = cp
            .sub("StringStyle")
            .and_then(|s| self.style_ref(s))
            .and_then(|s| self.style_data(s, state))
            .and_then(|d| self.text_data(d, state, 0));
        if let Some(b) = base {
            let tp = self.world.props(b);
            if std::env::var("HB_UIDBG2").is_ok() {
                eprintln!("textstyle {} : {:?}\n comp {:?}", self.world.path(b), tp, cp);
            }
            st.font = tp.obj("StyleFont");
            if let Some(c) = tp.lcolor("StyleColor") {
                st.color = c;
            }
            if let Some(s) = tp.floats("Scale", 2) {
                st.scale = [s[0], s[1]];
            }
            if let Some(s) = tp.floats("SpacingAdjust", 2) {
                st.spacing = [s[0], s[1]];
            }
            for k in 0..2 {
                if let Some(a) = tp.enum_i("Alignment", k) {
                    st.align[k as usize] = align_of(a);
                }
            }
            st.wrap = tp.enum_i("ClipMode", 0) == Some("CLIP_Wrap");
            for k in 0..2 {
                if let Some(v) = tp.f32_i("StylePadding", k) {
                    st.padding[k as usize] = v;
                }
            }
            st.autoscale = tp.sub("AutoScaling").and_then(|a| a.enum_i("AutoScaleMode", 0)).is_some_and(|m| m != "UIAUTOSCALE_None");
        }
        if let Some(o) = cp.sub("TextStyleCustomization") {
            if o.bool("bOverrideDrawFont") == Some(true) {
                st.font = o.obj("DrawFont").or(st.font);
            }
            if o.bool("bOverrideDrawColor") == Some(true) {
                if let Some(c) = o.lcolor("DrawColor") {
                    st.color = c;
                }
            }
            if o.bool("bOverrideOpacity") == Some(true) {
                st.color[3] = o.f32("Opacity").unwrap_or(1.0);
            }
            if o.bool("bOverrideScale") == Some(true) {
                st.scale = [o.f32_i("DrawScale", 0).unwrap_or(1.0), o.f32_i("DrawScale", 1).unwrap_or(1.0)];
            }
            if o.bool("bOverrideSpacingAdjust") == Some(true) {
                st.spacing = [o.f32_i("SpacingAdjust", 0).unwrap_or(0.0), o.f32_i("SpacingAdjust", 1).unwrap_or(0.0)];
            }
            if o.bool("bOverrideAlignment") == Some(true) {
                for k in 0..2 {
                    if let Some(a) = o.enum_i("TextAlignment", k) {
                        st.align[k as usize] = align_of(a);
                    }
                }
            }
            if o.bool("bOverrideClipMode") == Some(true) {
                st.wrap = o.enum_i("ClipMode", 0) == Some("CLIP_Wrap");
            }
            if o.bool("bOverridePadding") == Some(true) {
                st.padding = [o.f32_i("Padding", 0).unwrap_or(0.0), o.f32_i("Padding", 1).unwrap_or(0.0)];
            }
            if o.bool("bOverrideAutoScale") == Some(true) {
                st.autoscale = o.sub("AutoScaling").and_then(|a| a.enum_i("AutoScaleMode", 0)).is_some_and(|m| m != "UIAUTOSCALE_None");
            }
        }
        st
    }

    /// Glyph quads of a single-line string in a rect.
    /// Appends the glyph quads; returns the right edge of the last glyph.
    pub fn text_quads(&self, text: &str, st: &TextStyle, rect: [f32; 4], vw: f32, vh: f32, out: &mut Vec<Quad>) -> f32 {
        let Some(fr) = st.font else { return rect[0] };
        let Some(font) = self.font(fr) else { return rect[0] };
        // FUN_0074d970: min(vw / 800, vh / 600)
        let auto = if st.autoscale { (vw / 800.0).min(vh / 600.0) } else { 1.0 };
        let (sx, sy) = (st.scale[0] * auto, st.scale[1] * auto);
        // line height = the font's tallest glyph (trace frame 400: rows on one grid with or
        // without the tall space glyph); glyphs sit at line top + VerticalOffset
        let line_h = font.chars.iter().map(|g| g.h).max().unwrap_or(0) as f32 * sy;
        // UFont Kerning is added to every advance (trace t3 frame 2700: HUDFont_Green, Kerning -6,
        // timer glyphs 16.9 wide advance 10.9 at native size)
        let kern = font.kerning as f32;
        let adv = |c: char| font.glyph(c).map(|g| (g.w as f32 + st.spacing[0] + kern) * sx).unwrap_or(0.0);
        let width_of = |t: &str| t.chars().map(adv).sum::<f32>() - (st.spacing[0] + kern) * sx;
        // StylePadding scales with autoscale (Esc caption 6 * 1.28, version label 5 * 1.28)
        let (px, py) = (st.padding[0] * auto, st.padding[1] * auto);
        // CLIP_Wrap: greedy word wrap; a line keeps its trailing space (trace frame 1000, map
        // description: lines break before the word that would cross rect right - padding)
        let mut lines: Vec<String> = Vec::new();
        for para in text.split('\n') {
            if !st.wrap {
                lines.push(para.to_string());
                continue;
            }
            let avail = rect[2] - rect[0] - 2.0 * px;
            let mut line = String::new();
            for word in para.split_inclusive(' ') {
                let cand = format!("{line}{word}");
                if !line.is_empty() && width_of(cand.trim_end()) > avail {
                    lines.push(std::mem::take(&mut line));
                    line = word.to_string();
                } else {
                    line = cand;
                }
            }
            lines.push(line);
        }
        let total_h = line_h * lines.len() as f32;
        let mut y0 = match st.align[1] {
            1 => rect[1] + (rect[3] - rect[1] - total_h) * 0.5,
            2 => rect[3] - py - total_h,
            _ => rect[1] + py,
        };
        let mut end = rect[0];
        for line in &lines {
            let width = width_of(line);
            let mut x = match st.align[0] {
                1 => rect[0] + (rect[2] - rect[0] - width) * 0.5,
                2 => rect[2] - px - width,
                _ => rect[0] + px,
            };
            for c in line.chars() {
                let Some(g) = font.glyph(c) else { continue };
                if g.w > 0 && g.h > 0 {
                    let tex = font.textures.get(g.tex as usize).and_then(|&t| self.world.gref(fr.pkg, t));
                    let (tw, th) = tex.and_then(|t| self.tex_size(t)).unwrap_or((1024.0, 256.0));
                    out.push(Quad {
                        rot: 0.0,
                        rect: [x, y0 + g.voff as f32 * sy, x + g.w as f32 * sx, y0 + (g.voff + g.h) as f32 * sy],
                        uv: [g.u as f32 / tw, g.v as f32 / th, (g.u + g.w) as f32 / tw, (g.v + g.h) as f32 / th],
                        tex,
                        color: st.color,
                    });
                }
                x += (g.w as f32 + st.spacing[0] + kern) * sx;
            }
            end = x - (st.spacing[0] + kern) * sx;
            y0 += line_h;
        }
        end
    }

    /// SimpleObjects followed by CodeBasedSimpleObjects (both drawn, as the game does).
    pub fn simple_objects(&self, p: &Tree) -> Vec<GRef> {
        let mut v = self.world.objects(p.get("SimpleObjects"));
        v.extend(self.world.objects(p.get("CodeBasedSimpleObjects")));
        v
    }

    /// Width of a single-line string with a style (glyph advances + spacing).
    /// Width of a string in 1280x720 units at a text scale.
    pub fn text_width_1280(&self, text: &str, font: Option<GRef>, scale: f32) -> f32 {
        let st = TextStyle { font, color: [1.0; 4], scale: [scale, scale], spacing: [0.0, 0.0], align: [0, 0], autoscale: false, padding: [0.0, 0.0], wrap: false };
        self.string_width(text, &st)
    }

    pub fn string_width(&self, text: &str, st: &TextStyle) -> f32 {
        let Some(fr) = st.font else { return 0.0 };
        let Some(font) = self.font(fr) else { return 0.0 };
        let n = text.chars().count() as f32;
        text.chars().filter_map(|c| font.glyph(c)).map(|g| g.w as f32 * st.scale[0]).sum::<f32>() + (n - 1.0).max(0.0) * (st.spacing[0] + font.kerning as f32) * st.scale[0]
    }

    pub fn tex_size(&self, t: GRef) -> Option<(f32, f32)> {
        let p = self.world.props(t);
        let w = p.f32("SizeX")?;
        let h = p.f32("SizeY")?;
        Some((w, h))
    }
}

/// Per-widget runtime overrides (script behaviour, state).
#[derive(Debug, Clone, Default)]
pub struct WidgetRt {
    pub state: Option<UiState>,
    pub text: Option<String>,
    pub text_color: Option<[f32; 4]>,
    pub hidden: Option<bool>,
    /// GCUIObject_CookieTrail: CookieName of every scene on the stack, oldest first
    pub crumbs: Vec<String>,
    /// check boxes / radio entries
    pub checked: Option<bool>,
    /// sliders: current value
    pub slider: Option<f32>,
    /// list frames: rows
    pub list: Option<ListRt>,
    /// image widgets: texture shown instead of the component's
    pub image: Option<GRef>,
    /// SimpleObjects overrides by index
    pub simple: HashMap<usize, SimpleRt>,
    /// code-drawn images (e.g. GCUIObject_AmmoCounter bullets): texture, rect in 1280x720
    /// units (x0, y0, x1, y1), colour
    pub extra: Vec<(GRef, [f32; 4], [f32; 4])>,
    /// the same rotated about the rect centre (radians, clockwise), e.g. the full map's player arrow
    pub extra_rot: Vec<(GRef, [f32; 4], [f32; 4], f32)>,
    /// with a texture sub-rectangle (u0, v0, u1, v1): rect, uv, colour
    pub extra_uv: Vec<(GRef, [f32; 4], [f32; 4], [f32; 4])>,
    /// code-drawn text: (text, font, x / top-y in 1280x720 units, scale, colour, centred)
    pub extra_text: Vec<(String, Option<GRef>, [f32; 2], f32, [f32; 4], bool)>,
    /// GCUIObject_Placer: origin (pixels) of its simple objects instead of the widget's rect
    pub origin: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Default)]
pub struct SimpleRt {
    pub text: Option<String>,
    pub image: Option<GRef>,
    pub hidden: Option<bool>,
    pub color: Option<[f32; 4]>,
    /// GCUISimpleProgressBar fill 0..1
    pub progress: Option<f32>,
}

/// Rows of a GCUIObject_MapListFrame.
#[derive(Debug, Clone, Default)]
pub struct ListRt {
    /// (text, enabled)
    pub items: Vec<(String, bool)>,
    pub selected: Option<usize>,
    pub hover: Option<usize>,
    pub top: usize,
    /// GCUIInfo m_UnSelectedItemColor / m_SelectedItemColor
    pub colors: [[f32; 4]; 2],
}

impl Ui {
    /// UIComp_DrawImage: style data (UIStyle_Image: StyleColor, DefaultImage, Coordinates) of the
    /// state, then the component's ImageRef and StyleCustomization overrides.
    fn image_comp(&self, comp: GRef, rect: [f32; 4], state: UiState, out: &mut Vec<Quad>) {
        self.image_comp_tex(comp, rect, state, None, out)
    }

    fn image_comp_tex(&self, comp: GRef, rect: [f32; 4], state: UiState, tex_override: Option<GRef>, out: &mut Vec<Quad>) {
        let cp = self.world.props(comp);
        let mut color = [1.0f32; 4];
        let mut tex = None;
        let mut coords: Option<[f32; 4]> = None;
        let rd = |c: &Tree| [c.f32("U").unwrap_or(0.0), c.f32("V").unwrap_or(0.0), c.f32("UL").unwrap_or(0.0), c.f32("VL").unwrap_or(0.0)];
        if let Some(sd) = cp.sub("ImageStyle").and_then(|s| self.style_ref(s)).and_then(|s| self.style_data(s, state)) {
            let sp = self.world.props(sd);
            let data = match self.world.class(sd).as_str() {
                "UIStyle_Combo" => sp.sub("ImageStyle").and_then(|i| i.obj("CustomStyleData")).map(|d| self.world.props(d)),
                _ => Some(sp),
            };
            if let Some(d) = data {
                if let Some(col) = d.lcolor("StyleColor") {
                    color = col;
                }
                tex = d.obj("DefaultImage");
                coords = d.sub("Coordinates").map(rd);
            }
        }
        if let Some(t) = cp.obj("ImageRef").and_then(|r| self.world.props(r).obj("ImageTexture")) {
            tex = Some(t);
        }
        if let Some(o) = cp.sub("StyleCustomization") {
            if o.bool("bOverrideDrawColor") == Some(true) {
                if let Some(col) = o.lcolor("DrawColor") {
                    color = col;
                }
            }
            if o.bool("bOverrideOpacity") == Some(true) {
                color[3] *= o.f32("Opacity").unwrap_or(1.0);
            }
            if o.bool("bOverrideCoordinates") == Some(true) {
                coords = o.sub("Coordinates").map(rd);
            }
        }
        let Some(tex) = tex_override.or(tex) else { return };
        let mut uv = [0.0, 0.0, 1.0, 1.0];
        // UL/VL 0 = whole texture (NavButtonBG: VL 75 of 128 -> v 0..0.5859, trace frame 400)
        if let (Some([u, v, ul, vl]), Some((tw, th))) = (coords, self.tex_size(tex)) {
            let ul = if ul == 0.0 { tw } else { ul };
            let vl = if vl == 0.0 { th } else { vl };
            uv = [u / tw, v / th, (u + ul) / tw, (v + vl) / th];
        }
        out.push(Quad { rot: 0.0, rect, uv, tex: Some(tex), color });
    }

    fn label_text(&self, w: &Widget, src: &str, rt: Option<&WidgetRt>) -> String {
        if let Some(t) = rt.and_then(|r| r.text.clone()) {
            return t;
        }
        let markup = w.props.sub(src).and_then(|d| d.str("MarkupString")).unwrap_or("");
        self.loc.resolve(markup)
    }

    /// Breadcrumbs: one label per scene (CookieStyle, the newest PrimaryCookieStyle), bottom-aligned
    /// in the trail's rect, separated by CookieSeparator. Measured in trace frame 600 (1366x768):
    /// separator x = text end + CookieToSeparatorPadding * vw/1280, width CookieSeparatorWidth * vw/1280,
    /// vertical = crumb line (trail bottom - font height) grown by SeparatorBorderExpansion and
    /// shifted by SeparatorVerticalOffset (both unscaled).
    fn cookie_trail(&self, w: &Widget, p: &Tree, crumbs: &[String], vw: f32, vh: f32, out: &mut Vec<Quad>) {
        let k = vw / 1280.0;
        let pad = p.f32("CookieToSeparatorPadding").unwrap_or(0.0) * k;
        let sep_w = p.f32("CookieSeparatorWidth").unwrap_or(0.0) * k;
        let exp = p.f32("SeparatorBorderExpansion").unwrap_or(0.0);
        let voff = p.f32("SeparatorVerticalOffset").unwrap_or(0.0);
        let sep = p.obj("CookieSeparator");
        let mut x = w.rect[0];
        for (i, c) in crumbs.iter().enumerate() {
            let primary = i + 1 == crumbs.len();
            let st = self.tag_text_style(if primary { "PrimaryCookieStyle" } else { "CookieStyle" }, UiState::Enabled);
            let text = self.loc.resolve(c);
            let end = self.text_quads(&text, &st, [x, w.rect[1], w.rect[2], w.rect[3]], vw, vh, out);
            if primary {
                break;
            }
            let auto = if st.autoscale { (vw / 800.0).min(vh / 600.0) } else { 1.0 };
            let line_h = st.font.and_then(|f| self.font(f)).map(|f| f.chars.iter().map(|g| g.h).max().unwrap_or(0) as f32).unwrap_or(0.0) * st.scale[1] * auto;
            let (t, b) = (w.rect[3] - line_h, w.rect[3]);
            let x0 = end + pad;
            if let Some(tex) = sep {
                out.push(Quad { rot: 0.0, rect: [x0, t - exp + voff, x0 + sep_w, b + exp + voff], uv: [0.0, 0.0, 1.0, 1.0], tex: Some(tex), color: [1.0; 4] });
            }
            x = x0 + sep_w + pad;
        }
    }

    /// UISlider bar + marker (trace frame 1000, ScoreLimitSlider 1200 in 100..4000, 1366x768):
    /// bar centred, full width, height BarSize * width / texture UL (SliderBar style
    /// slider_rail 256 wide, ADJUST_Justified: 7 * 123.97 / 256 = 3.39); marker width
    /// MarkerWidth * vw (PercentViewport), height = slider height, x = left + t * (width - marker).
    #[allow(clippy::too_many_arguments)]
    fn slider(&self, w: &Widget, p: &Tree, sv: &Tree, value: Option<f32>, state: UiState, vw: f32, _vh: f32, out: &mut Vec<Quad>) {
        let (min, max) = (sv.f32("MinValue").unwrap_or(0.0), sv.f32("MaxValue").unwrap_or(1.0));
        let v = value.or(sv.f32("CurrentValue")).unwrap_or(min);
        let t = if max > min { ((v - min) / (max - min)).clamp(0.0, 1.0) } else { 0.0 };
        let r = w.rect;
        let ww = r[2] - r[0];
        if let Some(c) = p.obj("SliderBarImageComponent") {
            let bar = p.sub("BarSize").and_then(|b| b.f32("Value")).unwrap_or(0.0);
            let texw = self.comp_image_width(c, state).unwrap_or(ww);
            let h = bar * ww / texw;
            let cy = (r[1] + r[3]) * 0.5;
            self.image_comp(c, [r[0], cy - h * 0.5, r[2], cy + h * 0.5], state, out);
        }
        if let Some(c) = p.obj("MarkerImageComponent") {
            let mw = p.sub("MarkerWidth").and_then(|b| b.f32("Value")).unwrap_or(0.02) * vw;
            let x0 = r[0] + t * (ww - mw);
            self.image_comp(c, [x0, r[1], x0 + mw, r[3]], state, out);
        }
    }

    /// Texel width an image component samples (style Coordinates UL, else texture width).
    fn comp_image_width(&self, comp: GRef, state: UiState) -> Option<f32> {
        let cp = self.world.props(comp);
        let sd = cp.sub("ImageStyle").and_then(|s| self.style_ref(s)).and_then(|s| self.style_data(s, state))?;
        let d = self.world.props(sd);
        let tex = cp.obj("ImageRef").and_then(|r| self.world.props(r).obj("ImageTexture")).or(d.obj("DefaultImage"))?;
        let ul = d.sub("Coordinates").and_then(|c| c.f32("UL")).unwrap_or(0.0);
        if ul > 0.0 { Some(ul) } else { self.tex_size(tex).map(|t| t.0) }
    }

    /// GCUIObject_MapListFrame rows (trace frame 1000, 1366x768, k = vh/720): row height
    /// m_RowHeight*k, rows to the right edge minus ScrollbarWidth*k, background m_RowBackgroundImage
    /// in m_RowEvenColor/m_RowOddColor for all m_MaxVisibleRows (mFillOutColors), selected row
    /// m_RowSelectedImage in m_RowSelectedColor, text m_Font at m_FontScale*k, x = column
    /// TextPadding*k, line top = row top + m_CellGridSpacing*k.
    fn list_rows(&self, w: &Widget, p: &Tree, l: &ListRt, vh: f32, out: &mut Vec<Quad>) {
        let k = vh / 720.0;
        let rh = p.f32("m_RowHeight").unwrap_or(21.0) * k;
        let rows = p.f32("m_MaxVisibleRows").unwrap_or(9.0) as usize;
        let x0 = w.rect[0];
        let x1 = w.rect[2] - p.f32("ScrollbarWidth").unwrap_or(0.0) * k;
        let pad = self.world.structs(p.get("m_ArrayColumns")).first().and_then(|c| c.f32("TextPadding")).unwrap_or(0.0) * k;
        let grid = p.f32("m_CellGridSpacing").unwrap_or(0.0) * k;
        let even = p.lcolor("m_RowEvenColor").unwrap_or([0.0; 4]);
        let odd = p.lcolor("m_RowOddColor").unwrap_or([0.0; 4]);
        let selc = p.lcolor("m_RowSelectedColor").unwrap_or([1.0; 4]);
        let disc = p.lcolor("m_DisabledTextColor").unwrap_or([0.3, 0.3, 0.3, 1.0]);
        let hovc = p.lcolor("m_MouseOverItemColor").unwrap_or(l.colors[1]);
        let st = TextStyle {
            font: p.obj("m_Font"),
            color: [1.0; 4],
            scale: [p.f32("m_FontScale").unwrap_or(1.0) * k; 2],
            spacing: [0.0; 2],
            align: [0, 0],
            autoscale: false,
            padding: [0.0; 2],
            wrap: false,
        };
        for i in 0..rows {
            let y = w.rect[1] + i as f32 * rh;
            let rect = [x0, y, x1, y + rh];
            if let Some(t) = p.obj("m_RowBackgroundImage") {
                out.push(Quad { rot: 0.0, rect, uv: [0.0, 0.0, 1.0, 1.0], tex: Some(t), color: if i % 2 == 0 { even } else { odd } });
            }
            let idx = l.top + i;
            if l.selected == Some(idx) {
                if let Some(t) = p.obj("m_RowSelectedImage") {
                    out.push(Quad { rot: 0.0, rect, uv: [0.0, 0.0, 1.0, 1.0], tex: Some(t), color: selc });
                }
            }
            if let Some((text, enabled)) = l.items.get(idx) {
                let mut s2 = st.clone();
                s2.color = if !enabled {
                    disc
                } else if l.selected == Some(idx) {
                    l.colors[1]
                } else if l.hover == Some(idx) {
                    hovc
                } else {
                    l.colors[0]
                };
                self.text_quads(text, &s2, [x0 + pad, y + grid, x1, y + rh], 0.0, 0.0, out);
            }
        }
    }

    /// Code-drawn text at a pixel position, `scale` in pixels per font pixel (overlays drawn
    /// after a scene, e.g. an open combo box list).
    #[allow(clippy::too_many_arguments)]
    pub fn text_px(&self, text: &str, font: Option<GRef>, at: [f32; 2], scale: f32, color: [f32; 4], vw: f32, vh: f32, out: &mut Vec<Quad>) {
        let st = TextStyle { font, color, scale: [scale, scale], spacing: [0.0, 0.0], align: [0, 0], autoscale: false, padding: [0.0, 0.0], wrap: false };
        self.text_quads(text, &st, [at[0], at[1], vw + 4096.0, vh + 4096.0], vw, vh, out);
    }

    /// Draw list of a laid-out scene.
    pub fn draw_scene(&self, s: &Scene, vw: f32, vh: f32, rt: &HashMap<usize, WidgetRt>, out: &mut Vec<Quad>) {
        self.draw_widget(s, 0, vw, vh, rt, out);
    }

    fn draw_widget(&self, s: &Scene, wi: usize, vw: f32, vh: f32, rt: &HashMap<usize, WidgetRt>, out: &mut Vec<Quad>) {
        let w = &s.widgets[wi];
        let r = rt.get(&wi);
        if r.and_then(|r| r.hidden).unwrap_or(w.hidden) {
            return;
        }
        let state = r.and_then(|r| r.state).unwrap_or(UiState::Enabled);
        let p = w.props.clone();
        let class = w.class.as_str();
        // console button prompts are not drawn on PC (trace frame 400: no 360_A quad)
        if class.starts_with("GCUIImage_ConsoleButton") || class.starts_with("GCUIImage_ConsoleDPad") {
            return;
        }
        if class == "GCUIObject_CookieTrail" {
            if let Some(r) = r {
                self.cookie_trail(w, &p, &r.crumbs, vw, vh, out);
            }
            return;
        }
        // GCUISimpleImage / GCUISimpleLabel objects (placers, loadout/ability widgets): positions in
        // 1280x720 space; a label's Size is its text scale (trace t3 frame 1500: spawn-selection title
        // at Position 110,17.5 Size 0.75 -> x 117.4 at 1366 wide)
        let simple = self.simple_objects(&p);
        if !simple.is_empty() {
            let (sx, sy) = (vw / 1280.0, vh / 720.0);
            for (si, o) in simple.into_iter().enumerate() {
                let op = self.world.props(o);
                let srt = r.and_then(|r| r.simple.get(&si));
                if srt.and_then(|x| x.hidden).unwrap_or(op.bool("bHidden") == Some(true)) {
                    continue;
                }
                let pos = op.floats("Position", 2).unwrap_or(vec![0.0, 0.0]);
                let color = op.lcolor("DrawColor").unwrap_or([1.0; 4]);
                let oclass = self.world.class(o);
                if oclass == "GCUISimpleLabel" || oclass == "GCUISimpleLabel_Integer" || oclass == "GCUISimpleLabel_Cascading" {
                    let mut text = srt.and_then(|x| x.text.clone()).unwrap_or_else(|| self.loc.resolve(op.str("Text").unwrap_or("")));
                    if text.is_empty() {
                        continue;
                    }
                    let size = op.floats("Size", 2).unwrap_or(vec![1.0, 1.0]);
                    let mut st = TextStyle {
                        font: op.obj("Font"),
                        color: srt.and_then(|x| x.color).unwrap_or(color),
                        scale: [size[0] * sx, size[1] * sy],
                        spacing: [op.i32("HorizontalTextSpacing").unwrap_or(0) as f32, 0.0],
                        align: [0, 0],
                        autoscale: false,
                        padding: [0.0, 0.0],
                        wrap: false,
                    };
                    let just = op.enum_i("Justification", 0).map(align_of).unwrap_or(0);
                    let x0 = pos[0] * sx;
                    let y0 = pos[1] * sy;
                    // GCUISimpleLabel_Integer: value padded to m_nMaxDigitAllowed digits, the padding
                    // digits drawn at m_fDigitPaddingAlphaModifier (trace t3 frame 2700: dim "0" + "30")
                    let mut pad = String::new();
                    if oclass == "GCUISimpleLabel_Integer" {
                        let n = op.i32("m_nMaxDigitAllowed").unwrap_or(0).max(0) as usize;
                        if text.len() < n {
                            pad = "0".repeat(n - text.len());
                        }
                    }
                    let full = format!("{pad}{text}");
                    let w = self.string_width(&full, &st);
                    let left = match just {
                        1 => x0 - w * 0.5,
                        2 => x0 - w,
                        _ => x0,
                    };
                    if !pad.is_empty() {
                        let mut ps = st.clone();
                        ps.color[3] *= op.f32("m_fDigitPaddingAlphaModifier").unwrap_or(1.0);
                        let e = self.text_quads(&pad, &ps, [left, y0, vw + 4096.0, vh + 4096.0], vw, vh, out);
                        text = std::mem::take(&mut text);
                        st.align = [0, 0];
                        self.text_quads(&text, &st, [e + st.spacing[0] * st.scale[0], y0, vw + 4096.0, vh + 4096.0], vw, vh, out);
                    } else {
                        self.text_quads(&text, &st, [left, y0, vw + 4096.0, vh + 4096.0], vw, vh, out);
                    }
                    continue;
                }
                if oclass == "GCUISimpleProgressBar" {
                    // Bar texture at Position / Size (1280x720), filled left to right
                    let Some(p) = srt.and_then(|x| x.progress) else { continue };
                    let Some(tex) = op.obj("Bar") else { continue };
                    let size = op.floats("Size", 2).unwrap_or(vec![100.0, 4.0]);
                    let (x0, y0) = (pos[0] * sx, pos[1] * sy);
                    let w = size[0] * sx * p.clamp(0.0, 1.0);
                    if w > 0.0 {
                        out.push(Quad { rot: 0.0, rect: [x0, y0, x0 + w, y0 + size[1] * sy], uv: [0.0, 0.0, 1.0, 1.0], tex: Some(tex), color });
                    }
                    continue;
                }
                let Some(tex) = srt.and_then(|x| x.image).or_else(|| op.obj("Image")) else { continue };
                let size = op.floats("Size", 2).unwrap_or(vec![64.0, 64.0]);
                let c = op.sub("ImageCoordinates");
                let (u, v, ul, vl) = c.map(|c| (c.f32("U").unwrap_or(0.0), c.f32("V").unwrap_or(0.0), c.f32("UL").unwrap_or(1.0), c.f32("VL").unwrap_or(1.0))).unwrap_or((0.0, 0.0, 1.0, 1.0));
                let org = r.and_then(|r| r.origin).unwrap_or([w.rect[0], w.rect[1]]);
                let mut x0 = org[0] + pos[0] * sx;
                let mut x1 = x0 + size[0] * sx;
                let mut y0 = org[1] + pos[1] * sy;
                let mut y1 = y0 + size[1] * sy;
                if class != "GCUIObject_Placer" {
                    x0 = pos[0] * sx;
                    x1 = x0 + size[0] * sx;
                    y0 = pos[1] * sy;
                    y1 = y0 + size[1] * sy;
                }
                let mut uv = [u, v, u + ul, v + vl];
                if x1 < x0 {
                    std::mem::swap(&mut x0, &mut x1);
                    uv.swap(0, 2);
                }
                if y1 < y0 {
                    std::mem::swap(&mut y0, &mut y1);
                    uv.swap(1, 3);
                }
                out.push(Quad { rot: 0.0, rect: [x0, y0, x1, y1], uv, tex: Some(tex), color: srt.and_then(|x| x.color).unwrap_or(color) });
            }
        }
        if let Some(r) = r {
            let (sx, sy) = (vw / 1280.0, vh / 720.0);
            // sub-rectangle images first (the full map), the icons over them
            for (tex, rc, uv, color) in &r.extra_uv {
                out.push(Quad { rot: 0.0, rect: [rc[0] * sx, rc[1] * sy, rc[2] * sx, rc[3] * sy], uv: *uv, tex: Some(*tex), color: *color });
            }
            for (tex, rc, color) in &r.extra {
                out.push(Quad { rot: 0.0, rect: [rc[0] * sx, rc[1] * sy, rc[2] * sx, rc[3] * sy], uv: [0.0, 0.0, 1.0, 1.0], tex: Some(*tex), color: *color });
            }
            for (tex, rc, color, rot) in &r.extra_rot {
                out.push(Quad { rot: *rot, rect: [rc[0] * sx, rc[1] * sy, rc[2] * sx, rc[3] * sy], uv: [0.0, 0.0, 1.0, 1.0], tex: Some(*tex), color: *color });
            }
            for (text, font, pos, scale, color, centred) in &r.extra_text {
                let st = TextStyle { font: *font, color: *color, scale: [scale * sx, scale * sy], spacing: [0.0, 0.0], align: [0, 0], autoscale: false, padding: [0.0, 0.0], wrap: false };
                let w = if *centred { self.string_width(text, &st) } else { 0.0 };
                self.text_quads(text, &st, [pos[0] * sx - w * 0.5, pos[1] * sy, vw + 4096.0, vh + 4096.0], vw, vh, out);
            }
        }
        if let Some(c) = p.obj("ImageComponent") {
            self.image_comp_tex(c, w.rect, state, r.and_then(|r| r.image), out);
        }
        if let Some(c) = p.obj("BackgroundImageComponent") {
            self.image_comp(c, w.rect, state, out);
        }
        if r.and_then(|r| r.checked) == Some(true) {
            if let Some(c) = p.obj("CheckedImageComponent") {
                self.image_comp(c, w.rect, state, out);
            }
        }
        if let Some(sv) = p.sub("SliderValue") {
            self.slider(w, &p, sv, r.and_then(|r| r.slider), state, vw, vh, out);
        }
        if let Some(l) = r.and_then(|r| r.list.as_ref()) {
            self.list_rows(w, &p, l, vh, out);
        }
        if let Some(c) = p.obj("LabelBackground") {
            self.image_comp_tex(c, w.rect, state, r.and_then(|r| r.image), out);
        }
        if let Some(c) = p.obj("StringRenderComponent") {
            let src = if p.get("CaptionDataSource").is_some() { "CaptionDataSource" } else { "DataSource" };
            let text = self.label_text(w, src, r);
            if !text.is_empty() {
                let mut st = self.text_style(c, state);
                // UITabButton: the owning UITabControl's TabButtonCaptionStyle (UE3 applies the
                // tab control's styles to its buttons)
                if class == "UITabButton" {
                    if let Some(cap) = w.parent.and_then(|pi| s.widgets.get(pi)).filter(|pw| pw.class == "UITabControl").and_then(|pw| pw.props.sub("TabButtonCaptionStyle").cloned()) {
                        let mut t = (*self.world.props(c)).clone();
                        t.0.retain(|x| x.0 != "StringStyle");
                        t.0.push(("StringStyle".into(), 0, PVal::Struct(cap)));
                        st = self.text_style_tree(&t, state);
                        // its StylePadding is -25 (text left of the button); the control's
                        // TabButtonPadding (5.0, UIEXTENTEVAL_PercentViewport) that should offset it
                        // is not understood: no negative padding here (NOT from data)
                        st.padding[0] = st.padding[0].max(0.0);
                    }
                }
                if std::env::var("HB_UIDBG").is_ok() {
                    eprintln!("label {} rect {:?} text {:?} style {:?}", w.tag, w.rect, text, st);
                }
                if let Some(col) = r.and_then(|r| r.text_color) {
                    st.color = col;
                }
                self.text_quads(&text, &st, w.rect, vw, vh, out);
            }
        }
        for &c in &w.children {
            self.draw_widget(s, c, vw, vh, rt, out);
        }
    }
}
