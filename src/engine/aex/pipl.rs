//! A1: the PiPL resources of an AE-format effect plug-in (`.aex`, a Windows DLL) and the PE resource directory they sit
//! in, read as data: no plug-in code runs. Milan reads these in its own process from files it did not make, so every
//! read is bounds-checked and a damaged file is an error, never a panic. The writer makes the PiPLs of Milan's own test
//! plug-in (its build script links them as a `.res`); `res_file` writes that `.res`.
//!
//! Shared by Milan (`engine/aex`), the plug-in host (`#[path]`) and `tools/aex-test-plugin/build.rs` (`#[path]`), so
//! it uses std only. Provenance of every layout fact: `docs/compat/aex-provenance.md` (PiPL: class P/R; PE and `.res`:
//! Microsoft's documentation).
#![allow(dead_code)]

/// Files larger than this are not read (an effect plug-in is a few MB).
pub const MAX_FILE: u64 = 512 << 20;
/// At most this many properties per PiPL and PiPLs per file.
const MAX_PROPS: usize = 4096;
const MAX_PIPLS: usize = 256;

/// A four-character code as the API writes it ('eFKT'): the first character is the most significant byte.
pub const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

pub const VENDOR: u32 = fourcc(b"8BIM");
pub const KIND: u32 = fourcc(b"kind");
pub const NAME: u32 = fourcc(b"name");
pub const CATEGORY: u32 = fourcc(b"catg");
pub const ENTRY_WIN64: u32 = fourcc(b"8664");
pub const PIPL_VERSION: u32 = fourcc(b"ePVR");
pub const SPEC_VERSION: u32 = fourcc(b"eSVR");
pub const VERSION: u32 = fourcc(b"eVER");
pub const INFO_FLAGS: u32 = fourcc(b"eINF");
pub const OUT_FLAGS: u32 = fourcc(b"eGLO");
pub const OUT_FLAGS2: u32 = fourcc(b"eGL2");
pub const MATCH_NAME: u32 = fourcc(b"eMNA");
pub const RESERVED_INFO: u32 = fourcc(b"aeFL");
pub const SUPPORT_URL: u32 = fourcc(b"eURL");
/// The kind of an After Effects effect.
pub const KIND_EFFECT: u32 = fourcc(b"eFKT");

/// The x64 machine type of a PE file header.
pub const MACHINE_X64: u16 = 0x8664;

/// One property of a PiPL.
#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub vendor: u32,
    pub key: u32,
    pub id: u32,
    pub data: Vec<u8>,
}

/// One PiPL: its properties in file order, and whether it was stored big-endian (old Mac-built resources).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pipl {
    pub props: Vec<Property>,
    pub big_endian: bool,
}

/// What Milan needs from an effect's PiPL.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effect {
    /// The kind code ('eFKT' for an effect) as four characters.
    pub kind: String,
    pub name: String,
    pub category: String,
    /// The exported entry point (Win64); "" when the PiPL has none.
    pub entry: String,
    pub match_name: String,
    /// (major, minor) of the API the plug-in was built for.
    pub spec_version: (u16, u16),
    /// The plug-in's own packed version.
    pub version: u32,
    pub out_flags: u32,
    pub out_flags2: u32,
    pub support_url: String,
}

/// A four-character code as text ("eFKT"); unprintable bytes as '?'.
pub fn fourcc_str(v: u32) -> String {
    v.to_be_bytes().iter().map(|&b| if (32..127).contains(&b) { b as char } else { '?' }).collect()
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
    big: bool,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| format!("the PiPL ends early (needs {n} bytes at {})", self.at))?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn u16(&mut self) -> Result<u16, String> {
        let big = self.big;
        let s = self.take(2)?;
        Ok(if big { u16::from_be_bytes([s[0], s[1]]) } else { u16::from_le_bytes([s[0], s[1]]) })
    }

    fn u32(&mut self) -> Result<u32, String> {
        let big = self.big;
        let s: [u8; 4] = self.take(4)?.try_into().expect("4 bytes");
        Ok(if big { u32::from_be_bytes(s) } else { u32::from_le_bytes(s) })
    }
}

/// Parse one PiPL resource. Little-endian (Windows) starts `01 00`, big-endian `00 01`; then a 4-byte version and a
/// 4-byte property count; each property is vendor, key, id and length (4 bytes each) and its data padded to 4 bytes.
pub fn parse(bytes: &[u8]) -> Result<Pipl, String> {
    let big = match bytes.get(..2) {
        Some([1, 0]) => false,
        Some([0, 1]) => true,
        Some(_) => return Err("not a PiPL (it does not start with the PiPL marker)".into()),
        None => return Err("the PiPL is empty".into()),
    };
    let mut r = Reader { b: bytes, at: 2, big };
    let _version = r.u32()?;
    let count = r.u32()? as usize;
    if count > MAX_PROPS {
        return Err(format!("the PiPL claims {count} properties"));
    }
    let mut props = Vec::with_capacity(count);
    for i in 0..count {
        let (vendor, key, id, len) = (r.u32()?, r.u32()?, r.u32()?, r.u32()? as usize);
        if len > bytes.len() {
            return Err(format!("property {i} ({}) claims {len} bytes", fourcc_str(key)));
        }
        let data = r.take(len)?.to_vec();
        // Padding to 4 bytes; the last property may end without it.
        let pad = (4 - len % 4) % 4;
        r.at = (r.at + pad).min(bytes.len());
        props.push(Property { vendor, key, id, data });
    }
    Ok(Pipl { props, big_endian: big })
}

/// A string property: a length-prefixed (Pascal) string when the data is one (length byte, the text, zero padding),
/// else a NUL-terminated one. Lossy UTF-8; Windows-1252 text outside ASCII is rare in these names.
fn text(data: &[u8]) -> String {
    let pascal = data.first().map(|&n| n as usize).filter(|&n| n > 0 && n < data.len() && !data[1..1 + n].contains(&0) && data[1 + n..].iter().all(|&b| b == 0) && data.len() - 1 - n < 4);
    let s = match pascal {
        Some(n) => &data[1..1 + n],
        None => data.split(|&b| b == 0).next().unwrap_or_default(),
    };
    String::from_utf8_lossy(s).into_owned()
}

impl Pipl {
    pub fn get(&self, key: u32) -> Option<&[u8]> {
        self.props.iter().find(|p| p.key == key).map(|p| p.data.as_slice())
    }

    fn num(&self, key: u32) -> u32 {
        let Some(d) = self.get(key).and_then(|d| d.get(..4)) else { return 0 };
        let a: [u8; 4] = d.try_into().expect("4 bytes");
        if self.big_endian { u32::from_be_bytes(a) } else { u32::from_le_bytes(a) }
    }

    fn pair(&self, key: u32) -> (u16, u16) {
        let Some(d) = self.get(key).and_then(|d| d.get(..4)) else { return (0, 0) };
        let h = |a: u8, b: u8| if self.big_endian { u16::from_be_bytes([a, b]) } else { u16::from_le_bytes([a, b]) };
        (h(d[0], d[1]), h(d[2], d[3]))
    }

    /// The effect this PiPL describes.
    pub fn effect(&self) -> Effect {
        Effect {
            kind: self.get(KIND).filter(|d| d.len() >= 4).map(|_| fourcc_str(self.num(KIND))).unwrap_or_default(),
            name: self.get(NAME).map(text).unwrap_or_default(),
            category: self.get(CATEGORY).map(text).unwrap_or_default(),
            entry: self.get(ENTRY_WIN64).map(text).unwrap_or_default(),
            match_name: self.get(MATCH_NAME).map(text).unwrap_or_default(),
            spec_version: self.pair(SPEC_VERSION),
            version: self.num(VERSION),
            out_flags: self.num(OUT_FLAGS),
            out_flags2: self.num(OUT_FLAGS2),
            support_url: self.get(SUPPORT_URL).map(text).unwrap_or_default(),
        }
    }

    /// The PiPL of `e` as Milan's writer lays it out (little-endian; names, category and match name as length-prefixed
    /// strings, the entry point NUL-terminated).
    pub fn of(e: &Effect) -> Pipl {
        let pascal = |s: &str| {
            let b = &s.as_bytes()[..s.len().min(255)];
            let mut v = vec![b.len() as u8];
            v.extend_from_slice(b);
            v
        };
        let cstr = |s: &str| {
            let mut v = s.as_bytes().to_vec();
            v.push(0);
            v
        };
        let num = |n: u32| n.to_le_bytes().to_vec();
        let kind = if e.kind.len() == 4 { fourcc(e.kind.as_bytes().try_into().expect("4 bytes")) } else { KIND_EFFECT };
        let mut props = vec![
            (KIND, num(kind)),
            (NAME, pascal(&e.name)),
            (CATEGORY, pascal(&e.category)),
            (ENTRY_WIN64, cstr(&e.entry)),
            (PIPL_VERSION, [2u16.to_le_bytes(), 0u16.to_le_bytes()].concat()),
            (SPEC_VERSION, [e.spec_version.0.to_le_bytes(), e.spec_version.1.to_le_bytes()].concat()),
            (VERSION, num(e.version)),
            (INFO_FLAGS, num(0)),
            (OUT_FLAGS, num(e.out_flags)),
            (OUT_FLAGS2, num(e.out_flags2)),
            (MATCH_NAME, pascal(&e.match_name)),
            (RESERVED_INFO, num(0)),
        ];
        if !e.support_url.is_empty() {
            props.push((SUPPORT_URL, cstr(&e.support_url)));
        }
        Pipl { props: props.into_iter().map(|(key, data)| Property { vendor: VENDOR, key, id: 0, data }).collect(), big_endian: false }
    }

    /// The resource bytes (little-endian unless `big_endian`).
    pub fn write(&self) -> Vec<u8> {
        let big = self.big_endian;
        let w32 = |v: u32| if big { v.to_be_bytes() } else { v.to_le_bytes() };
        let mut out = if big { vec![0, 1] } else { vec![1, 0] };
        out.extend(w32(0));
        out.extend(w32(self.props.len() as u32));
        for p in &self.props {
            out.extend(w32(p.vendor));
            out.extend(w32(p.key));
            out.extend(w32(p.id));
            out.extend(w32(p.data.len() as u32));
            out.extend(&p.data);
            out.resize(out.len() + (4 - p.data.len() % 4) % 4, 0);
        }
        out
    }
}

// ---------------------------------------------------------------- PE files

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at.checked_add(2).ok_or("offset overflow")?).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| format!("the file ends at {at}"))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at.checked_add(4).ok_or("offset overflow")?).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| format!("the file ends at {at}"))
}

/// A PE file's machine type and its sections (virtual address, virtual size, file offset, file size), and the
/// resource directory's (address, size).
struct Pe {
    machine: u16,
    sections: Vec<(u32, u32, u32, u32)>,
    resources: (u32, u32),
}

fn pe(b: &[u8]) -> Result<Pe, String> {
    if b.get(..2) != Some(b"MZ") {
        return Err("not a Windows DLL (no MZ header)".into());
    }
    let nt = u32_at(b, 0x3c)? as usize;
    if b.get(nt..nt + 4) != Some(b"PE\0\0") {
        return Err("not a Windows DLL (no PE header)".into());
    }
    let fh = nt + 4;
    let machine = u16_at(b, fh)?;
    let nsec = u16_at(b, fh + 2)? as usize;
    let opt_size = u16_at(b, fh + 16)? as usize;
    let opt = fh + 20;
    let magic = u16_at(b, opt)?;
    // The data directories start at 96 (PE32) or 112 (PE32+); the resource directory is the third.
    let dirs = match magic {
        0x10b => opt + 96,
        0x20b => opt + 112,
        m => return Err(format!("unknown optional header 0x{m:04x}")),
    };
    let ndirs = u32_at(b, dirs - 4)? as usize;
    let resources = if ndirs > 2 && dirs + 24 <= opt + opt_size { (u32_at(b, dirs + 16)?, u32_at(b, dirs + 20)?) } else { (0, 0) };
    let sec = opt + opt_size;
    if nsec > 96 {
        return Err(format!("the file claims {nsec} sections"));
    }
    let mut sections = Vec::with_capacity(nsec);
    for i in 0..nsec {
        let s = sec + i * 40;
        sections.push((u32_at(b, s + 12)?, u32_at(b, s + 8)?, u32_at(b, s + 20)?, u32_at(b, s + 16)?));
    }
    Ok(Pe { machine, sections, resources })
}

impl Pe {
    /// The file offset of virtual address `rva` and how many bytes of the section's file data follow it.
    fn offset(&self, rva: u32) -> Option<(usize, usize)> {
        self.sections.iter().find_map(|&(va, vsize, raw, rsize)| {
            let d = rva.checked_sub(va)?;
            (d < vsize.max(rsize) && d < rsize).then(|| (raw as usize + d as usize, (rsize - d) as usize))
        })
    }
}

/// The machine type of a PE file (0x8664 for x64).
pub fn machine(b: &[u8]) -> Result<u16, String> {
    pe(b).map(|p| p.machine)
}

/// The data of every resource whose type is the string `type_name` (any case), in directory order: (name, data). A
/// name is its string, or "#<id>".
pub fn resources(b: &[u8], type_name: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let p = pe(b)?;
    if p.resources.0 == 0 {
        return Ok(Vec::new());
    }
    let (base, avail) = p.offset(p.resources.0).filter(|&(at, _)| at < b.len()).ok_or("the resource directory is outside the file")?;
    let sect = &b[base..base + avail.min(b.len() - base)];
    // One directory: its entries (name or id, is-directory, offset), bounds-checked within the resource section.
    let entries = |off: usize| -> Result<Vec<(Result<String, u32>, bool, usize)>, String> {
        let n = u16_at(sect, off + 12)? as usize + u16_at(sect, off + 14)? as usize;
        if n > 4096 {
            return Err(format!("a resource directory claims {n} entries"));
        }
        let mut v = Vec::with_capacity(n);
        for i in 0..n {
            let e = off + 16 + i * 8;
            let (name, data) = (u32_at(sect, e)?, u32_at(sect, e + 4)?);
            let name = if name & 0x8000_0000 != 0 {
                let at = (name & 0x7fff_ffff) as usize;
                let len = u16_at(sect, at)? as usize;
                let raw = sect.get(at + 2..at + 2 + len * 2).ok_or("a resource name is outside the file")?;
                Ok(String::from_utf16_lossy(&raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()))
            } else {
                Err(name)
            };
            v.push((name, data & 0x8000_0000 != 0, (data & 0x7fff_ffff) as usize));
        }
        Ok(v)
    };
    let mut out = Vec::new();
    for (ty, dir, off) in entries(0)? {
        if !dir || !ty.as_ref().is_ok_and(|t| t.eq_ignore_ascii_case(type_name)) {
            continue;
        }
        for (name, dir, off) in entries(off)? {
            let name = match name {
                Ok(s) => s,
                Err(id) => format!("#{id}"),
            };
            // Each language of the name (a leaf directly under the name is tolerated).
            let leaves = if dir { entries(off)?.into_iter().filter(|e| !e.1).map(|e| e.2).collect() } else { vec![off] };
            for leaf in leaves {
                let (rva, size) = (u32_at(sect, leaf)?, u32_at(sect, leaf + 4)? as usize);
                let (at, avail) = p.offset(rva).ok_or_else(|| format!("resource {name} is outside the file"))?;
                if size > avail || at.checked_add(size).is_none_or(|end| end > b.len()) {
                    return Err(format!("resource {name} claims {size} bytes"));
                }
                out.push((name.clone(), b[at..at + size].to_vec()));
                if out.len() > MAX_PIPLS {
                    return Err("too many resources".into());
                }
            }
        }
    }
    Ok(out)
}

/// The effects of a plug-in file: its machine type and each PiPL's effect (or why that PiPL could not be read).
pub fn effects_of(bytes: &[u8]) -> Result<(u16, Vec<Result<Effect, String>>), String> {
    let m = machine(bytes)?;
    let v = resources(bytes, "PIPL")?.into_iter().map(|(name, data)| parse(&data).map(|p| p.effect()).map_err(|e| format!("PiPL {name}: {e}"))).collect();
    Ok((m, v))
}

/// Read `path` (bounded) and return `effects_of` it.
pub fn effects_of_file(path: &std::path::Path) -> Result<(u16, Vec<Result<Effect, String>>), String> {
    let len = std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?.len();
    if len > MAX_FILE {
        return Err(format!("{} is too large to be an effect plug-in ({len} bytes)", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    effects_of(&bytes)
}

// ---------------------------------------------------------------- .res files (Microsoft's resource file format)

/// A `.res` file holding `items` (resource type name, numeric id, data), language US English, for the linker.
pub fn res_file(items: &[(&str, u16, Vec<u8>)]) -> Vec<u8> {
    fn header(out: &mut Vec<u8>, ty: &str, id: u16, size: usize) {
        let mut h = Vec::new();
        for c in ty.encode_utf16().chain([0]) {
            h.extend(c.to_le_bytes());
        }
        h.extend([0xff, 0xff]);
        h.extend(id.to_le_bytes());
        while (8 + h.len()) % 4 != 0 {
            h.push(0);
        }
        h.extend(0u32.to_le_bytes()); // data version
        h.extend(0x0030u16.to_le_bytes()); // memory flags: moveable, pure
        h.extend(0x0409u16.to_le_bytes()); // language
        h.extend(0u32.to_le_bytes()); // version
        h.extend(0u32.to_le_bytes()); // characteristics
        out.extend((size as u32).to_le_bytes());
        out.extend(((8 + h.len()) as u32).to_le_bytes());
        out.extend(h);
    }
    // The empty first entry that marks a 32-bit resource file.
    let mut out = vec![0u8, 0, 0, 0, 0x20, 0, 0, 0, 0xff, 0xff, 0, 0, 0xff, 0xff, 0, 0];
    out.extend([0u8; 16]);
    for (ty, id, data) in items {
        header(&mut out, ty, *id, data.len());
        out.extend(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}

#[cfg(test)]
pub mod tests_support {
    use super::*;

    /// A minimal PE32+ DLL image holding `pipls` (a resource section laid out as the linker does), for the reader tests.
    pub fn fake_dll_raw(machine: u16, pipls: &[Vec<u8>]) -> Vec<u8> {
        // Resource section at RVA 0x1000, file offset 0x400.
        let mut rs = Vec::new();
        let push16 = |v: &mut Vec<u8>, x: u16| v.extend(x.to_le_bytes());
        let push32 = |v: &mut Vec<u8>, x: u32| v.extend(x.to_le_bytes());
        // Root: one named type entry.
        let dir = |v: &mut Vec<u8>, named: u16, ids: u16| {
            v.extend([0u8; 12]);
            push16(v, named);
            push16(v, ids);
        };
        let n = pipls.len();
        let root = 0usize;
        let type_dir = 16 + 8;
        let lang_dirs = type_dir + 16 + 8 * n;
        let leaves = lang_dirs + n * (16 + 8);
        let name_str = leaves + n * 16;
        let data = name_str + 12;
        dir(&mut rs, 1, 0);
        assert_eq!(rs.len(), root + 16);
        push32(&mut rs, 0x8000_0000 | name_str as u32);
        push32(&mut rs, 0x8000_0000 | type_dir as u32);
        dir(&mut rs, 0, n as u16);
        for i in 0..n {
            push32(&mut rs, 16000 + i as u32);
            push32(&mut rs, 0x8000_0000 | (lang_dirs + i * 24) as u32);
        }
        for i in 0..n {
            dir(&mut rs, 0, 1);
            push32(&mut rs, 0x409);
            push32(&mut rs, (leaves + i * 16) as u32);
        }
        let mut at = data;
        for p in pipls {
            push32(&mut rs, 0x1000 + at as u32);
            push32(&mut rs, p.len() as u32);
            push32(&mut rs, 0);
            push32(&mut rs, 0);
            at += p.len().div_ceil(4) * 4;
        }
        push16(&mut rs, 4);
        for c in "PiPL".encode_utf16() {
            push16(&mut rs, c);
        }
        rs.extend([0u8; 2]);
        assert_eq!(rs.len(), data);
        for p in pipls {
            rs.extend(p);
            rs.resize(rs.len().div_ceil(4) * 4, 0);
        }
        let mut f = vec![0u8; 0x400];
        f[0..2].copy_from_slice(b"MZ");
        f[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        f[0x80..0x84].copy_from_slice(b"PE\0\0");
        let fh = 0x84;
        f[fh..fh + 2].copy_from_slice(&machine.to_le_bytes());
        f[fh + 2..fh + 4].copy_from_slice(&1u16.to_le_bytes());
        f[fh + 16..fh + 18].copy_from_slice(&240u16.to_le_bytes());
        let opt = fh + 20;
        f[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        f[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes());
        f[opt + 128..opt + 132].copy_from_slice(&0x1000u32.to_le_bytes());
        f[opt + 132..opt + 136].copy_from_slice(&(rs.len() as u32).to_le_bytes());
        let s = opt + 240;
        f[s..s + 5].copy_from_slice(b".rsrc");
        f[s + 8..s + 12].copy_from_slice(&(rs.len() as u32).to_le_bytes());
        f[s + 12..s + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        f[s + 16..s + 20].copy_from_slice(&(rs.len() as u32).to_le_bytes());
        f[s + 20..s + 24].copy_from_slice(&0x400u32.to_le_bytes());
        f.extend(rs);
        f
    }

    /// A fake x64 plug-in file with one PiPL per effect.
    pub fn fake_dll(effects: &[Effect]) -> Vec<u8> {
        fake_dll_for(MACHINE_X64, effects)
    }

    pub fn fake_dll_for(machine: u16, effects: &[Effect]) -> Vec<u8> {
        fake_dll_raw(machine, &effects.iter().map(|e| Pipl::of(e).write()).collect::<Vec<_>>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::tests_support::fake_dll_raw as fake_dll;

    fn sample() -> Effect {
        Effect {
            kind: "eFKT".into(),
            name: "Test Invert".into(),
            category: "Milan Test".into(),
            entry: "MilanTestInvert".into(),
            match_name: "MLNT Test Invert".into(),
            spec_version: (13, 28),
            version: 0x0008_0001,
            out_flags: 0x0200_0410,
            out_flags2: 0x0800_0000,
            support_url: "https://example.org/x".into(),
        }
    }

    /// A1 acceptance: the writer's PiPL parses back to the same effect, little- and big-endian, and the PE reader finds
    /// every PiPL of a file (several effects per file) whatever case the type name has.
    #[test]
    fn pipl_round_trips() {
        let e = sample();
        let p = Pipl::of(&e);
        assert_eq!(parse(&p.write()).unwrap().effect(), e);
        let big = Pipl { big_endian: true, ..p.clone() };
        let back = parse(&big.write()).unwrap();
        assert!(back.big_endian);
        assert_eq!(back.props.iter().map(|p| p.key).collect::<Vec<_>>(), p.props.iter().map(|p| p.key).collect::<Vec<_>>());
        assert_eq!(fourcc_str(back.props[0].key), "kind");
        // Fourcc codes are stored with their bytes reversed on Windows: 'kind' is "dnik" in the file.
        assert_eq!(&p.write()[14..18], b"dnik");
        // A NUL-terminated match name (as other writers may store it) reads the same.
        let mut c = p.clone();
        c.props.iter_mut().find(|q| q.key == MATCH_NAME).unwrap().data = b"MLNT Test Invert\0\0\0\0".to_vec();
        assert_eq!(parse(&c.write()).unwrap().effect().match_name, "MLNT Test Invert");
        let two = Effect { name: "Second".into(), match_name: "MLNT Second".into(), entry: "Second".into(), ..e.clone() };
        let dll = fake_dll(MACHINE_X64, &[p.write(), Pipl::of(&two).write()]);
        let (m, effects) = effects_of(&dll).unwrap();
        assert_eq!(m, MACHINE_X64);
        assert_eq!(effects, vec![Ok(e), Ok(two)]);
        assert_eq!(resources(&dll, "pipl").unwrap().iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["#16000", "#16001"]);
        assert!(resources(&dll, "ICON").unwrap().is_empty());
        // The .res writer: an empty first entry, then each resource with its UTF-16 type name, aligned.
        let res = res_file(&[("PiPL", 16000, vec![1, 2, 3])]);
        assert_eq!(&res[..8], &[0, 0, 0, 0, 0x20, 0, 0, 0]);
        assert_eq!(res.len() % 4, 0);
        assert_eq!(u32::from_le_bytes(res[32..36].try_into().unwrap()), 3);
        assert_eq!(String::from_utf16_lossy(&res[40..48].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()), "PiPL");
    }

    /// A1 acceptance: damaged and hostile inputs are errors, never panics (it runs in Milan's process on files it did
    /// not make).
    #[test]
    fn pipl_rejects_truncated_and_oversized() {
        let good = Pipl::of(&sample()).write();
        // Every truncation of a PiPL is an error or a shorter, valid reading; none panics.
        for n in 0..good.len() {
            let _ = parse(&good[..n]);
        }
        assert!(parse(&good[..good.len() - 8]).is_err());
        assert!(parse(&[]).is_err() && parse(&[7, 7, 0, 0]).is_err());
        let mut huge = good.clone();
        huge[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&huge).unwrap_err().contains("properties"));
        let mut long = good.clone();
        long[22..26].copy_from_slice(&0x7fff_fff0u32.to_le_bytes());
        assert!(parse(&long).is_err());
        // Strings that are not padded, empty or all NULs.
        for d in [&b""[..], b"\0\0\0\0", b"\xff", b"\x05ab", b"abc"] {
            let _ = text(d);
        }
        assert_eq!(text(b"\x03abc"), "abc");
        assert_eq!(text(b"abc\0"), "abc");
        // A DLL whose every byte is cut short, or whose offsets point anywhere, never panics.
        let dll = fake_dll(MACHINE_X64, &[good.clone()]);
        for n in (0..dll.len()).step_by(7) {
            let _ = effects_of(&dll[..n]);
        }
        let mut seed = 0x1234_5678u32;
        for _ in 0..3000 {
            let mut d = dll.clone();
            for _ in 0..4 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let at = (seed as usize >> 8) % d.len();
                d[at] = (seed >> 3) as u8;
            }
            let _ = effects_of(&d);
        }
        assert!(effects_of(b"MZ").is_err());
        assert!(effects_of(b"not a dll at all").is_err());
        assert_eq!(machine(&fake_dll(0x14c, &[good])).unwrap(), 0x14c);
    }
}
