//! R34b: the `.vstpreset` file format (VST 3 SDK, `vstpresetfile.cpp`): a 48-byte header ("VST3", version 1, the
//! class id as 32 ASCII hex digits, the offset of the chunk list), the chunks' bytes, then the list ("List", a count,
//! and per chunk its 4-char id, offset and size). Milan reads and writes the "Comp" (component state) and "Cont"
//! (controller state) chunks and writes an "Info" chunk naming the plug-in; other chunks are kept out of the way.
use std::io;

const MAGIC: &[u8; 4] = b"VST3";
const VERSION: i32 = 1;
const HEADER: usize = 4 + 4 + 32 + 8;

/// What a preset holds for Milan: the plug-in's class id and its two state blobs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preset {
    pub class_id: String,
    pub component: Vec<u8>,
    pub controller: Vec<u8>,
}

/// Write a preset for class `class_id` (32 hex digits) with its states; `name` goes into the Info chunk.
pub fn encode(p: &Preset, name: &str) -> Vec<u8> {
    let mut cid = p.class_id.to_ascii_uppercase().into_bytes();
    cid.resize(32, b'0');
    let info = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<MetaInfo>\n\t<Attribute id=\"MediaType\" value=\"VstPreset\" type=\"string\" flags=\"writeProtected\"/>\n\t<Attribute id=\"PlugInCategory\" value=\"Fx\" type=\"string\" flags=\"writeProtected\"/>\n\t<Attribute id=\"PlugInName\" value=\"{}\" type=\"string\" flags=\"writeProtected\"/>\n</MetaInfo>\n",
        name.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    );
    let chunks: Vec<(&[u8; 4], &[u8])> = [(b"Comp", p.component.as_slice()), (b"Cont", p.controller.as_slice()), (b"Info", info.as_bytes())].into_iter().filter(|c| !c.1.is_empty()).collect();
    let mut out = Vec::with_capacity(HEADER + chunks.iter().map(|c| c.1.len()).sum::<usize>() + 8 + chunks.len() * 20);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&cid);
    let list_at = HEADER + chunks.iter().map(|c| c.1.len()).sum::<usize>();
    out.extend_from_slice(&(list_at as i64).to_le_bytes());
    let mut entries = Vec::new();
    for (id, data) in &chunks {
        entries.push((*id, out.len() as i64, data.len() as i64));
        out.extend_from_slice(data);
    }
    out.extend_from_slice(b"List");
    out.extend_from_slice(&(entries.len() as i32).to_le_bytes());
    for (id, off, size) in entries {
        out.extend_from_slice(id);
        out.extend_from_slice(&off.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
    }
    out
}

fn le_i32(b: &[u8], at: usize) -> Option<i32> {
    b.get(at..at + 4).map(|x| i32::from_le_bytes(x.try_into().expect("4 bytes")))
}

fn le_i64(b: &[u8], at: usize) -> Option<i64> {
    b.get(at..at + 8).map(|x| i64::from_le_bytes(x.try_into().expect("8 bytes")))
}

/// Read a preset file's bytes.
pub fn decode(b: &[u8]) -> io::Result<Preset> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidData, format!("not a .vstpreset file: {what}"));
    if b.len() < HEADER || &b[..4] != MAGIC {
        return Err(bad("it does not start with \"VST3\""));
    }
    if le_i32(b, 4) != Some(VERSION) {
        return Err(bad(&format!("version {} is not supported", le_i32(b, 4).unwrap_or(0))));
    }
    let class_id = String::from_utf8_lossy(&b[8..40]).trim_end_matches('\0').to_ascii_uppercase();
    let list_at = le_i64(b, 40).ok_or_else(|| bad("no chunk list offset"))?;
    if list_at < HEADER as i64 || list_at as usize + 8 > b.len() || &b[list_at as usize..list_at as usize + 4] != b"List" {
        return Err(bad("the chunk list is missing"));
    }
    let n = le_i32(b, list_at as usize + 4).unwrap_or(0).clamp(0, 1024) as usize;
    let mut p = Preset { class_id, ..Default::default() };
    for k in 0..n {
        let at = list_at as usize + 8 + k * 20;
        let (Some(id), Some(off), Some(size)) = (b.get(at..at + 4), le_i64(b, at + 4), le_i64(b, at + 12)) else { return Err(bad("a chunk entry is cut short")) };
        if off < 0 || size < 0 || (off + size) as usize > b.len() {
            return Err(bad("a chunk lies outside the file"));
        }
        let data = b[off as usize..(off + size) as usize].to_vec();
        match id {
            b"Comp" => p.component = data,
            b"Cont" => p.controller = data,
            _ => {}
        }
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vstpreset_round_trips_and_rejects_junk() {
        let p = Preset { class_id: "4d694c616e5465737447616" .to_string() + "96e000001", component: vec![1, 2, 3, 4, 5], controller: vec![9] };
        let bytes = encode(&p, "Milan <Test> & Gain");
        assert_eq!(&bytes[..4], b"VST3");
        assert_eq!(&bytes[8..40], b"4D694C616E546573744761696E000001");
        let back = decode(&bytes).unwrap();
        assert_eq!(back, Preset { class_id: p.class_id.to_ascii_uppercase(), ..p.clone() });
        assert!(String::from_utf8_lossy(&bytes).contains("&lt;Test&gt; &amp; Gain"));
        // A preset without a controller chunk; a controller-only one.
        let only = decode(&encode(&Preset { controller: Vec::new(), ..p.clone() }, "x")).unwrap();
        assert!(only.controller.is_empty() && only.component == p.component);
        assert!(decode(b"RIFF....").is_err() && decode(&bytes[..30]).is_err());
        let mut cut = bytes.clone();
        cut.truncate(bytes.len() - 5);
        assert!(decode(&cut).is_err());
    }
}
