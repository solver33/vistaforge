//! Minimal NBT (Named Binary Tag) model with a big-endian writer and reader.

use std::io::{self, Read, Write};

#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<Tag>),
    Compound(Compound),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

/// A compound keeps insertion order so written files are deterministic.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Compound(pub Vec<(String, Tag)>);

impl Compound {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn put(&mut self, key: &str, tag: impl Into<Tag>) -> &mut Self {
        self.0.push((key.to_string(), tag.into()));
        self
    }

    pub fn with(mut self, key: &str, tag: impl Into<Tag>) -> Self {
        self.put(key, tag);
        self
    }

    pub fn get(&self, key: &str) -> Option<&Tag> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

impl From<Compound> for Tag {
    fn from(c: Compound) -> Self {
        Tag::Compound(c)
    }
}
impl From<i8> for Tag {
    fn from(v: i8) -> Self {
        Tag::Byte(v)
    }
}
impl From<bool> for Tag {
    fn from(v: bool) -> Self {
        Tag::Byte(v as i8)
    }
}
impl From<i32> for Tag {
    fn from(v: i32) -> Self {
        Tag::Int(v)
    }
}
impl From<i64> for Tag {
    fn from(v: i64) -> Self {
        Tag::Long(v)
    }
}
impl From<f32> for Tag {
    fn from(v: f32) -> Self {
        Tag::Float(v)
    }
}
impl From<f64> for Tag {
    fn from(v: f64) -> Self {
        Tag::Double(v)
    }
}
impl From<&str> for Tag {
    fn from(v: &str) -> Self {
        Tag::String(v.to_string())
    }
}
impl From<String> for Tag {
    fn from(v: String) -> Self {
        Tag::String(v)
    }
}

impl Tag {
    fn id(&self) -> u8 {
        match self {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(_) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }

    /// A list of strings, as used for palettes and pack lists.
    pub fn strings<S: AsRef<str>>(items: &[S]) -> Tag {
        Tag::List(
            items
                .iter()
                .map(|s| Tag::String(s.as_ref().to_string()))
                .collect(),
        )
    }
}

/// Writes `root` as a named root compound (the name is usually empty).
pub fn write_root<W: Write>(w: &mut W, name: &str, root: &Compound) -> io::Result<()> {
    w.write_all(&[10])?;
    write_string(w, name)?;
    write_compound(w, root)
}

fn write_string<W: Write>(w: &mut W, s: &str) -> io::Result<()> {
    // Java's modified UTF-8 matches standard UTF-8 for everything Minecraft ids use.
    let bytes = s.as_bytes();
    let len = u16::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "string too long"))?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(bytes)
}

fn write_compound<W: Write>(w: &mut W, c: &Compound) -> io::Result<()> {
    for (key, tag) in &c.0 {
        w.write_all(&[tag.id()])?;
        write_string(w, key)?;
        write_payload(w, tag)?;
    }
    w.write_all(&[0])
}

fn write_payload<W: Write>(w: &mut W, tag: &Tag) -> io::Result<()> {
    match tag {
        Tag::Byte(v) => w.write_all(&v.to_be_bytes()),
        Tag::Short(v) => w.write_all(&v.to_be_bytes()),
        Tag::Int(v) => w.write_all(&v.to_be_bytes()),
        Tag::Long(v) => w.write_all(&v.to_be_bytes()),
        Tag::Float(v) => w.write_all(&v.to_be_bytes()),
        Tag::Double(v) => w.write_all(&v.to_be_bytes()),
        Tag::ByteArray(v) => {
            w.write_all(&(v.len() as i32).to_be_bytes())?;
            let bytes: Vec<u8> = v.iter().map(|b| *b as u8).collect();
            w.write_all(&bytes)
        }
        Tag::String(s) => write_string(w, s),
        Tag::List(items) => {
            // An empty list is written with the End element type, as Minecraft does.
            let element = items.first().map(Tag::id).unwrap_or(0);
            if items.iter().any(|t| t.id() != element) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "mixed list element types",
                ));
            }
            w.write_all(&[element])?;
            w.write_all(&(items.len() as i32).to_be_bytes())?;
            for item in items {
                write_payload(w, item)?;
            }
            Ok(())
        }
        Tag::Compound(c) => write_compound(w, c),
        Tag::IntArray(v) => {
            w.write_all(&(v.len() as i32).to_be_bytes())?;
            for x in v {
                w.write_all(&x.to_be_bytes())?;
            }
            Ok(())
        }
        Tag::LongArray(v) => {
            w.write_all(&(v.len() as i32).to_be_bytes())?;
            let mut buf = Vec::with_capacity(v.len() * 8);
            for x in v {
                buf.extend_from_slice(&x.to_be_bytes());
            }
            w.write_all(&buf)
        }
    }
}

pub fn to_bytes(name: &str, root: &Compound) -> Vec<u8> {
    let mut out = Vec::new();
    write_root(&mut out, name, root).expect("writing to a Vec cannot fail");
    out
}

/// Reads a named root compound. Used by tests and the world checker.
pub fn read_root<R: Read>(r: &mut R) -> io::Result<(String, Compound)> {
    let id = read_u8(r)?;
    if id != 10 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("root tag is {id}, not a compound"),
        ));
    }
    let name = read_string(r)?;
    Ok((name, read_compound(r)?))
}

fn read_u8<R: Read>(r: &mut R) -> io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn read_n<R: Read, const N: usize>(r: &mut R) -> io::Result<[u8; N]> {
    let mut b = [0u8; N];
    r.read_exact(&mut b)?;
    Ok(b)
}

fn read_len<R: Read>(r: &mut R) -> io::Result<usize> {
    let n = i32::from_be_bytes(read_n(r)?);
    usize::try_from(n).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "negative length"))
}

fn read_string<R: Read>(r: &mut R) -> io::Result<String> {
    let len = u16::from_be_bytes(read_n(r)?) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn read_compound<R: Read>(r: &mut R) -> io::Result<Compound> {
    let mut c = Compound::new();
    loop {
        let id = read_u8(r)?;
        if id == 0 {
            return Ok(c);
        }
        let key = read_string(r)?;
        let tag = read_payload(r, id)?;
        c.0.push((key, tag));
    }
}

fn read_payload<R: Read>(r: &mut R, id: u8) -> io::Result<Tag> {
    Ok(match id {
        1 => Tag::Byte(i8::from_be_bytes(read_n(r)?)),
        2 => Tag::Short(i16::from_be_bytes(read_n(r)?)),
        3 => Tag::Int(i32::from_be_bytes(read_n(r)?)),
        4 => Tag::Long(i64::from_be_bytes(read_n(r)?)),
        5 => Tag::Float(f32::from_be_bytes(read_n(r)?)),
        6 => Tag::Double(f64::from_be_bytes(read_n(r)?)),
        7 => {
            let n = read_len(r)?;
            let mut buf = vec![0u8; n];
            r.read_exact(&mut buf)?;
            Tag::ByteArray(buf.into_iter().map(|b| b as i8).collect())
        }
        8 => Tag::String(read_string(r)?),
        9 => {
            let element = read_u8(r)?;
            let n = read_len(r)?;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(read_payload(r, element)?);
            }
            Tag::List(items)
        }
        10 => Tag::Compound(read_compound(r)?),
        11 => {
            let n = read_len(r)?;
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push(i32::from_be_bytes(read_n(r)?));
            }
            Tag::IntArray(v)
        }
        12 => {
            let n = read_len(r)?;
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push(i64::from_be_bytes(read_n(r)?));
            }
            Tag::LongArray(v)
        }
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown tag id {other}"),
            ))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let root = Compound::new()
            .with("byte", 5i8)
            .with("int", -7i32)
            .with("long", i64::MIN)
            .with("double", 0.25f64)
            .with("name", "minecraft:stone")
            .with("longs", Tag::LongArray(vec![1, -1, 42]))
            .with("empty", Tag::List(vec![]))
            .with(
                "palette",
                Tag::List(vec![Compound::new().with("Name", "minecraft:air").into()]),
            );
        let bytes = to_bytes("", &root);
        let (name, back) = read_root(&mut bytes.as_slice()).unwrap();
        assert_eq!(name, "");
        assert_eq!(back, root);
    }

    #[test]
    fn known_encoding() {
        // {"": {a: 1b}} -> 0a 0000 01 0001 'a' 01 00
        let bytes = to_bytes("", &Compound::new().with("a", 1i8));
        assert_eq!(bytes, vec![10, 0, 0, 1, 0, 1, b'a', 1, 0]);
    }
}
