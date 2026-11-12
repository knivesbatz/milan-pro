//! `.cube` LUTs (Adobe / Resolve format): parse, apply, a cache keyed by path and modified time, and export of a grade.
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;
use crate::engine::plugins::{FxEval, GRADING};
use crate::engine::vfx::Grade;

#[derive(Debug)]
pub struct Cube {
    pub title: String,
    pub size: usize,
    pub three_d: bool,
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// 3D: red fastest, then green, then blue.
    pub data: Vec<[f32; 3]>,
    /// A 1D shaper applied first (Resolve files that declare both LUT_1D_SIZE and LUT_3D_SIZE).
    pub shaper: Option<Box<Cube>>,
}

fn floats<const N: usize>(words: &[&str]) -> Option<[f32; N]> {
    if words.len() != N {
        return None;
    }
    let v: Vec<f32> = words.iter().map(|w| w.parse().ok()).collect::<Option<_>>()?;
    v.try_into().ok()
}

pub fn parse(text: &str) -> Result<Cube, String> {
    let (mut title, mut size1, mut size3, mut data) = (String::new(), None, None, Vec::new());
    let (mut domain, mut range1, mut range3) = (([0.0; 3], [1.0; 3]), None, None);
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(&word) = words.first() else { continue };
        let bad = || format!("line {}: cannot read \"{}\"", i + 1, line);
        let size = || words.get(1).and_then(|s| s.parse::<usize>().ok()).ok_or_else(bad);
        match word {
            "TITLE" => title = line[5..].trim().trim_matches('"').to_string(),
            "LUT_1D_SIZE" => size1 = Some(size()?),
            "LUT_3D_SIZE" => size3 = Some(size()?),
            "DOMAIN_MIN" => domain.0 = floats(&words[1..]).ok_or_else(bad)?,
            "DOMAIN_MAX" => domain.1 = floats(&words[1..]).ok_or_else(bad)?,
            "LUT_1D_INPUT_RANGE" | "LUT_3D_INPUT_RANGE" => {
                let [lo, hi] = floats(&words[1..]).ok_or_else(bad)?;
                *if word.starts_with("LUT_1D") { &mut range1 } else { &mut range3 } = Some(([lo; 3], [hi; 3]));
            }
            // Other keywords (a later format revision) are skipped; anything else must be a data row.
            w if w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') && w.chars().any(|c| c.is_ascii_uppercase()) => {}
            _ => data.push(floats(&words).ok_or_else(bad)?),
        }
    }
    // The .cube spec: 1D up to 65536 entries (log curves are often 1024 or 4096), 3D up to 256 per side.
    for (s, max) in [(size1, 65536), (size3, 256)] {
        if let Some(s) = s.filter(|s| !(2..=max).contains(s)) {
            return Err(format!("unsupported size {s}"));
        }
    }
    let want = size1.unwrap_or(0) + size3.map_or(0, |s| s.pow(3));
    if want == 0 {
        return Err("no LUT_1D_SIZE or LUT_3D_SIZE".into());
    }
    if data.len() != want {
        return Err(format!("{} entries, expected {want}", data.len()));
    }
    let cube = |size, three_d, (min, max), data| Cube { title: title.clone(), size, three_d, min, max, data, shaper: None };
    Ok(match (size1, size3) {
        (Some(n), Some(s)) => {
            let rest = data.split_off(n);
            Cube { shaper: Some(Box::new(cube(n, false, range1.unwrap_or(domain), data))), ..cube(s, true, range3.unwrap_or(domain), rest) }
        }
        (_, Some(s)) => cube(s, true, range3.or(range1).unwrap_or(domain), data),
        (Some(n), None) => cube(n, false, range1.or(range3).unwrap_or(domain), data),
        (None, None) => unreachable!("want > 0"),
    })
}

impl Cube {
    /// Straight RGB 0..1 through the LUT (trilinear in 3D, linear per channel in 1D).
    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        let c = self.shaper.as_ref().map_or(c, |s| s.apply(c));
        let n = self.size;
        let x: [f32; 3] = std::array::from_fn(|i| ((c[i] - self.min[i]) / (self.max[i] - self.min[i]).max(1e-6)).clamp(0.0, 1.0) * (n - 1) as f32);
        let split = |v: f32| {
            let i = (v.floor() as usize).min(n - 2);
            (i, v - i as f32)
        };
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        if !self.three_d {
            return std::array::from_fn(|ch| {
                let (i, t) = split(x[ch]);
                lerp(self.data[i][ch], self.data[i + 1][ch], t)
            });
        }
        let ((r, fr), (g, fg), (b, fb)) = (split(x[0]), split(x[1]), split(x[2]));
        let at = |dr: usize, dg: usize, db: usize| self.data[(r + dr) + (g + dg) * n + (b + db) * n * n];
        std::array::from_fn(|ch| {
            let e = |dg, db| lerp(at(0, dg, db)[ch], at(1, dg, db)[ch], fr);
            lerp(lerp(e(0, 0), e(1, 0), fg), lerp(e(0, 1), e(1, 1), fg), fb)
        })
    }
}

type Cached = (SystemTime, Result<Arc<Cube>, String>);

/// The LUT at `path`, parsed once per modified time. The error names the file and, for bad rows, the line.
pub fn load(path: &Path) -> Result<Arc<Cube>, String> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Cached>>> = OnceLock::new();
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).map_err(|e| format!("{name}: {e}"))?;
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, r)) = cache.get(path).filter(|(t, _)| *t == mtime) {
        return r.clone();
    }
    let r = std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|s| parse(&s)).map(Arc::new).map_err(|e| format!("{name}: {e}"));
    cache.insert(path.to_path_buf(), (mtime, r.clone()));
    r
}

/// The LUT files effect `e` reads (only Color Grading reads any).
pub fn paths(e: &FxEval) -> impl Iterator<Item = &Path> {
    ["input_lut", "look_lut"].into_iter().filter(move |_| e.id == GRADING).map(|k| e.t(k).trim()).filter(|p| !p.is_empty()).map(Path::new)
}

/// Why a LUT of `e` cannot be used (it then draws ungraded), for the preview status and the render log.
pub fn error(e: &FxEval) -> Option<String> {
    paths(e).find_map(|p| load(p).err()).map(|m| format!("Color Grading LUT {m}"))
}

/// Bake a Color Grading effect (LUTs, wheels, curves, saturation) into a `size`³ 3D `.cube`.
pub fn export_cube(e: &FxEval, path: &Path, size: usize) -> std::io::Result<()> {
    let (g, n) = (Grade::new(e), size.max(2));
    let mut s = format!("TITLE \"Milan Pro grade\"\nLUT_3D_SIZE {n}\n");
    let v = |i: usize| i as f32 / (n - 1) as f32;
    for b in 0..n {
        for gr in 0..n {
            for r in 0..n {
                let c = g.rgb([v(r), v(gr), v(b)]);
                let _ = writeln!(s, "{:.6} {:.6} {:.6}", c[0], c[1], c[2]);
            }
        }
    }
    std::fs::write(path, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::plugins::{find, preset_eval, GRADING};

    fn identity(n: usize) -> String {
        let mut s = format!("# made by a test\nTITLE \"id\"\nLUT_3D_SIZE {n}\n");
        let v = |i: usize| i as f32 / (n - 1) as f32;
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let _ = writeln!(s, "{} {} {}", v(r), v(g), v(b));
                }
            }
        }
        s
    }

    fn within_lsb(c: &Cube) {
        for r in (0..=255).step_by(15) {
            for g in (0..=255).step_by(17) {
                for b in [0, 77, 255] {
                    let i = [r, g, b].map(|v| v as f32 / 255.0);
                    let o = c.apply(i);
                    for ch in 0..3 {
                        assert!((o[ch] - i[ch]).abs() * 255.0 <= 1.0, "{i:?} -> {o:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn identity_cubes_and_header_variants() {
        for n in [2, 17] {
            let c = parse(&identity(n)).unwrap();
            assert_eq!((c.size, c.three_d, c.title.as_str()), (n, true, "id"));
            within_lsb(&c);
        }
        let c = parse("LUT_1D_SIZE 3\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2 # comment\n\n0 0 0\n0.5 0.5 0.5\n1 1 1\n").unwrap();
        assert!(!c.three_d && c.max == [2.0; 3]);
        assert!((c.apply([1.0, 0.5, 2.0])[0] - 0.5).abs() < 1e-6 && (c.apply([1.0, 0.5, 2.0])[2] - 1.0).abs() < 1e-6);
        assert!(parse("LUT_3D_INPUT_RANGE 0 1\nLUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").is_ok());
        // A 1024-entry 1D identity (Resolve-style log curves are this size or larger).
        let mut s = String::from("LUT_1D_SIZE 1024\n");
        for i in 0..1024 {
            let v = i as f32 / 1023.0;
            let _ = writeln!(s, "{v} {v} {v}");
        }
        let c = parse(&s).unwrap();
        assert!(!c.three_d && c.size == 1024);
        within_lsb(&c);
        assert!(parse("LUT_3D_SIZE 257\n").unwrap_err().contains("unsupported size"));
        // Shaper plus 3D, each with its own input range: x -> x / 2 into a 3D identity over 0..0.5 is the identity.
        let s = "LUT_1D_SIZE 2\nLUT_1D_INPUT_RANGE 0 1\nLUT_3D_SIZE 2\nLUT_3D_INPUT_RANGE 0 0.5\n0 0 0\n0.5 0.5 0.5\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
        let c = parse(s).unwrap();
        assert!(c.three_d && c.shaper.as_ref().is_some_and(|s| s.size == 2 && !s.three_d));
        within_lsb(&c);
    }

    #[test]
    fn garbage_is_an_error_with_a_line_number() {
        let e = parse("LUT_3D_SIZE 2\n0 0 0\nnot a number\n").unwrap_err();
        assert!(e.starts_with("line 3:"), "{e}");
        assert!(parse("LUT_3D_SIZE 2\n0 0 0\n").unwrap_err().contains("expected 8"));
        assert!(parse("").is_err());
    }

    #[test]
    fn exported_default_grade_reads_back_as_identity() {
        let dir = std::env::temp_dir().join(format!("milan-lut-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grade.cube");
        export_cube(&preset_eval(find(GRADING).unwrap(), None), &path, 33).unwrap();
        let c = load(&path).unwrap();
        assert_eq!(c.size, 33);
        within_lsb(&c);
        assert!(Arc::ptr_eq(&c, &load(&path).unwrap())); // cached
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(load(&path).is_err());
    }
}
