//! Tagged-property stream reader (UE3 v576 layout) and a small byte reader.

use crate::{FName, Package};

/// Little-endian cursor over an export's bytes.
pub struct Rd<'a> {
    pub b: &'a [u8],
    pub pos: usize,
}

impl<'a> Rd<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    pub fn left(&self) -> usize {
        self.b.len().saturating_sub(self.pos)
    }
    pub fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.pos + n > self.b.len() {
            return None;
        }
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Some(s)
    }
    pub fn u8(&mut self) -> Option<u8> {
        self.bytes(1).map(|s| s[0])
    }
    pub fn u16(&mut self) -> Option<u16> {
        self.bytes(2).map(|s| u16::from_le_bytes([s[0], s[1]]))
    }
    pub fn i32(&mut self) -> Option<i32> {
        self.bytes(4).map(|s| i32::from_le_bytes(s.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Option<u32> {
        self.i32().map(|v| v as u32)
    }
    pub fn f32(&mut self) -> Option<f32> {
        self.bytes(4).map(|s| f32::from_le_bytes(s.try_into().unwrap()))
    }
    pub fn fname(&mut self) -> Option<FName> {
        Some(FName { index: self.i32()?, number: self.i32()? })
    }
    pub fn fstring(&mut self) -> Option<String> {
        let len = self.i32()?;
        if len >= 0 {
            let s = self.bytes(len as usize)?;
            Some(s.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect())
        } else {
            let n = (-len) as usize;
            let s = self.bytes(n * 2)?;
            let v: Vec<u16> = s.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
            Some(String::from_utf16_lossy(&v))
        }
    }
    pub fn vec3(&mut self) -> Option<[f32; 3]> {
        Some([self.f32()?, self.f32()?, self.f32()?])
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Int(i32),
    Float(f32),
    Bool(bool),
    Byte(u8),
    /// Byte property holding an enum value name.
    Enum(String),
    Name(String),
    Str(String),
    Object(i32),
    /// Struct with a known inline layout.
    Vector([f32; 3]),
    Rotator([i32; 3]),
    Color([u8; 4]),
    LinearColor([f32; 4]),
    /// Anything else: raw bytes (arrays, generic structs, delegates...).
    Raw { type_name: String, struct_name: Option<String>, data: Vec<u8> },
}

#[derive(Debug, Clone)]
pub struct Prop {
    pub name: String,
    pub index: i32,
    pub value: Value,
}

#[derive(Debug, Default, Clone)]
pub struct Props(pub Vec<Prop>);

impl Props {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.iter().find(|p| p.name == name).map(|p| &p.value)
    }
    pub fn int(&self, name: &str) -> Option<i32> {
        match self.get(name)? {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }
    pub fn float(&self, name: &str) -> Option<f32> {
        match self.get(name)? {
            Value::Float(v) => Some(*v),
            _ => None,
        }
    }
    pub fn object(&self, name: &str) -> Option<i32> {
        match self.get(name)? {
            Value::Object(v) => Some(*v),
            _ => None,
        }
    }
    pub fn enum_name(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Value::Enum(v) | Value::Name(v) => Some(v),
            _ => None,
        }
    }
    pub fn vector(&self, name: &str) -> Option<[f32; 3]> {
        match self.get(name)? {
            Value::Vector(v) => Some(*v),
            _ => None,
        }
    }
    pub fn color(&self, name: &str) -> Option<[u8; 4]> {
        match self.get(name)? {
            Value::Color(v) => Some(*v),
            _ => None,
        }
    }
    pub fn linear_color(&self, name: &str) -> Option<[f32; 4]> {
        match self.get(name)? {
            Value::LinearColor(v) => Some(*v),
            _ => None,
        }
    }
    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }
    pub fn rotator(&self, name: &str) -> Option<[i32; 3]> {
        match self.get(name)? {
            Value::Rotator(v) => Some(*v),
            _ => None,
        }
    }
    /// All entries of a static array / repeated property (e.g. `Materials`).
    pub fn all<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s Prop> + 's {
        self.0.iter().filter(move |p| p.name == name)
    }
}

impl Package {
    /// A StructProperty's tagged members.
    pub fn struct_prop(&self, props: &Props, name: &str) -> Option<Props> {
        match props.get(name)? {
            Value::Raw { type_name, data, .. } if type_name == "StructProperty" => self.read_props(&mut Rd::new(data)),
            _ => None,
        }
    }

    /// An ArrayProperty of structs (tagged members per element).
    pub fn struct_array(&self, props: &Props, name: &str) -> Vec<Props> {
        let mut out = Vec::new();
        if let Some(Value::Raw { type_name, data, .. }) = props.get(name) {
            if type_name == "ArrayProperty" && data.len() >= 4 {
                let n = u32::from_le_bytes(data[0..4].try_into().unwrap());
                let mut r = Rd::new(&data[4..]);
                for _ in 0..n {
                    match self.read_props(&mut r) {
                        Some(p) => out.push(p),
                        None => break,
                    }
                }
            }
        }
        out
    }

    /// Parse the tagged property list at the reader's position; stops after `None`.
    pub fn read_props(&self, r: &mut Rd) -> Option<Props> {
        let mut out = Vec::new();
        loop {
            let name = self.name(r.fname()?);
            if name == "None" {
                return Some(Props(out));
            }
            let ty = self.name(r.fname()?);
            let size = r.i32()? as usize;
            let index = r.i32()?;
            let mut struct_name = None;
            if ty == "StructProperty" {
                struct_name = Some(self.name(r.fname()?));
            }
            if ty == "BoolProperty" {
                let v = r.u32()?;
                out.push(Prop { name, index, value: Value::Bool(v != 0) });
                continue;
            }
            let data = r.bytes(size)?;
            let mut d = Rd::new(data);
            let value = match (ty.as_str(), struct_name.as_deref()) {
                ("IntProperty", _) => Value::Int(d.i32()?),
                ("FloatProperty", _) => Value::Float(d.f32()?),
                ("ByteProperty", _) if size == 1 => Value::Byte(d.u8()?),
                ("ByteProperty", _) => Value::Enum(self.name(d.fname()?)),
                ("NameProperty", _) => Value::Name(self.name(d.fname()?)),
                ("StrProperty", _) => Value::Str(d.fstring()?),
                ("ObjectProperty" | "ClassProperty" | "ComponentProperty", _) => Value::Object(d.i32()?),
                ("StructProperty", Some("Vector")) if size == 12 => Value::Vector(d.vec3()?),
                ("StructProperty", Some("Rotator")) if size == 12 => {
                    Value::Rotator([d.i32()?, d.i32()?, d.i32()?])
                }
                ("StructProperty", Some("Color")) if size == 4 => {
                    // FColor is stored B, G, R, A; expose as R, G, B, A.
                    let c = d.bytes(4)?;
                    Value::Color([c[2], c[1], c[0], c[3]])
                }
                ("StructProperty", Some("LinearColor")) if size == 16 => {
                    Value::LinearColor([d.f32()?, d.f32()?, d.f32()?, d.f32()?])
                }
                _ => Value::Raw { type_name: ty.clone(), struct_name, data: data.to_vec() },
            };
            out.push(Prop { name, index, value });
        }
    }

    /// Reader positioned at the start of the export's tagged property stream.
    ///
    /// What precedes the properties depends on the object: plain objects have a
    /// NetIndex; actors with script state (RF_HasStack) have a state frame first;
    /// components carry TemplateOwnerClass (+ TemplateName). Rather than model every
    /// case, try candidate offsets and take the first one whose property list parses.
    pub fn export_reader(&self, export_index: usize) -> Rd<'_> {
        let e = &self.exports[export_index];
        let b = self.export_bytes(e);
        let mut cands: Vec<usize> = vec![4];
        if e.object_flags & 0x0200_0000_0000_0000 != 0 {
            // state frame: Node, StateNode, ProbeMask(u64), LatentAction(u16),
            // StateStack(TArray, assumed empty), Offset (if Node != 0), then NetIndex
            let node = i32::from_le_bytes(b.get(0..4).and_then(|s| s.try_into().ok()).unwrap_or([0; 4]));
            cands.insert(0, 4 + 4 + 8 + 2 + 4 + if node != 0 { 4 } else { 0 } + 4);
        }
        cands.extend([8, 12, 16]);
        for &c in &cands {
            if c >= b.len() {
                continue;
            }
            let mut r = Rd::new(b);
            r.pos = c;
            if self.first_tag_ok(&mut r) {
                let mut r = Rd::new(b);
                r.pos = c;
                return r;
            }
        }
        let mut r = Rd::new(b);
        r.pos = 4.min(b.len());
        r
    }

    fn first_tag_ok(&self, r: &mut Rd) -> bool {
        let Some(n) = r.fname() else { return false };
        // property names carry no instance number (US_AST SKC_MP_US_AST: offset 4 holds
        // (bEnableShadowCasting, 97) by accident, the tags start at 8)
        if n.index < 0 || n.index as usize >= self.names.len() || n.number != 0 {
            return false;
        }
        if self.name(n) == "None" {
            return true;
        }
        let Some(t) = r.fname() else { return false };
        if t.index < 0 || t.index as usize >= self.names.len() || !self.name(t).ends_with("Property") {
            return false;
        }
        matches!(r.i32(), Some(sz) if sz >= 0 && (sz as usize) <= r.b.len())
    }
}
