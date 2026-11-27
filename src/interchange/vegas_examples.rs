//! R55: the example VEGAS exports in `tests/vegas/`. Each `<name>.milan.json` is written by hand to the schema of
//! `tools/vegas/Export to Milan Pro.cs` (format "milan-vegas", version 2); none was exported from VEGAS. Beside each is
//! `<name>.expected.json`, what the import must give:
//!
//! - `equals`: JSON pointer into the imported project (serialized as Milan saves it) → value (numbers within 1e-4);
//! - `plugins`: pointer to an FX chain → the `plugin` of each FX in order;
//! - `len`: pointer to an array → its length;
//! - `same`: pairs of pointers that must hold the same value (a group id shared by two events);
//! - `warnings`: text each of which some import warning contains; `warning_count`: how many warnings in all.
//!
//! Media paths are never opened: `.wav` paths probe as 60 s stereo audio, paths with "offline" in them fail, and
//! anything else is a 600 s 1920x1080 video file with one stereo audio stream.
//! `MILAN_VEGAS_EXAMPLES_DUMP=<folder>` writes each imported project there, to help write a new expected file.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde_json::Value;
use crate::interchange::vegas;
use crate::project::{MediaItem, MediaType};

/// The example folder.
pub fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("vegas")
}

/// Every example export, by file name, sorted.
pub fn examples() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir()).expect("tests/vegas").flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".milan.json")).collect();
    v.sort();
    v
}

fn probe(paths: &[String]) -> HashMap<String, Result<MediaItem, String>> {
    paths
        .iter()
        .map(|p| {
            let name = Path::new(p).file_name().map_or(p.clone(), |n| n.to_string_lossy().into_owned());
            let item = |media_type, duration, audio_streams, size: Option<u32>| MediaItem {
                id: uuid::Uuid::new_v4().to_string(),
                path: p.into(),
                name: name.clone(),
                media_type,
                duration,
                audio_streams,
                width: size.map(|_| 1920),
                height: size.map(|_| 1080),
                ..Default::default()
            };
            let r = if p.contains("offline") {
                Err("file not found".to_string())
            } else if p.to_ascii_lowercase().ends_with(".wav") {
                Ok(item(MediaType::Audio, 60.0, 1, None))
            } else {
                Ok(item(MediaType::Video, 600.0, 1, Some(1)))
            };
            (p.clone(), r)
        })
        .collect()
}

/// The import of one example: the project as JSON and the warnings.
pub fn import(path: &Path) -> (Value, Vec<String>) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let e = vegas::read_vegas_json(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(e.version, 2, "{}: the examples follow version 2 of the script", path.display());
    let (p, w) = vegas::to_project(&e, &probe(&e.media_paths()));
    (serde_json::to_value(&p).expect("a project serializes"), w)
}

fn at<'a>(v: &'a Value, ptr: &str, file: &str) -> &'a Value {
    v.pointer(ptr).unwrap_or_else(|| panic!("{file}: nothing at {ptr}"))
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => (x.as_f64().unwrap_or(f64::NAN) - y.as_f64().unwrap_or(f64::NAN)).abs() < 1e-4,
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same_value(x, y)),
        _ => a == b,
    }
}

/// What failed in `file`'s import against its expected results (empty: it passed).
pub fn check(file: &Path) -> Vec<String> {
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    let exp_path = PathBuf::from(file.to_string_lossy().replace(".milan.json", ".expected.json"));
    let exp: Value = serde_json::from_str(&std::fs::read_to_string(&exp_path).unwrap_or_else(|e| panic!("{}: {e}", exp_path.display()))).expect("expected JSON");
    let (got, warnings) = import(file);
    if let Some(d) = std::env::var_os("MILAN_VEGAS_EXAMPLES_DUMP") {
        let out = PathBuf::from(d).join(name.replace(".milan.json", ".imported.json"));
        let dump = serde_json::json!({ "project": got, "warnings": warnings });
        std::fs::write(out, serde_json::to_string_pretty(&dump).unwrap()).expect("dump folder");
    }
    let mut fails = Vec::new();
    let obj = |k: &str| exp.get(k).and_then(Value::as_object).cloned().unwrap_or_default();
    for (ptr, want) in obj("equals") {
        let v = at(&got, &ptr, &name);
        if !same_value(v, &want) {
            fails.push(format!("{name}: {ptr} is {v}, expected {want}"));
        }
    }
    for (ptr, want) in obj("plugins") {
        let chain: Vec<&str> = at(&got, &ptr, &name).as_array().map(|a| a.iter().filter_map(|f| f["plugin"].as_str()).collect()).unwrap_or_default();
        let want: Vec<&str> = want.as_array().expect("plugins lists").iter().filter_map(Value::as_str).collect();
        if chain != want {
            fails.push(format!("{name}: {ptr} is {chain:?}, expected {want:?}"));
        }
    }
    for (ptr, want) in obj("len") {
        let n = at(&got, &ptr, &name).as_array().map_or(0, Vec::len);
        if Some(n as u64) != want.as_u64() {
            fails.push(format!("{name}: {ptr} has {n} items, expected {want}"));
        }
    }
    for pair in exp.get("same").and_then(Value::as_array).cloned().unwrap_or_default() {
        let (a, b) = (pair[0].as_str().unwrap(), pair[1].as_str().unwrap());
        let (x, y) = (at(&got, a, &name), at(&got, b, &name));
        if x != y || x.is_null() {
            fails.push(format!("{name}: {a} ({x}) and {b} ({y}) differ"));
        }
    }
    for w in exp.get("warnings").and_then(Value::as_array).cloned().unwrap_or_default() {
        let w = w.as_str().expect("warning text");
        if !warnings.iter().any(|x| x.contains(w)) {
            fails.push(format!("{name}: no warning contains {w:?} in {warnings:?}"));
        }
    }
    if let Some(n) = exp.get("warning_count").and_then(Value::as_u64)
        && warnings.len() as u64 != n
    {
        fails.push(format!("{name}: {} warnings, expected {n}: {warnings:?}", warnings.len()));
    }
    fails
}

/// Every example imports as its expected file says.
#[test]
fn every_example_imports_as_expected() {
    let files = examples();
    assert!(files.len() >= 11, "{files:?}");
    let fails: Vec<String> = files.iter().flat_map(|f| check(f)).collect();
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// Each example says in its own file that it was written by hand, not exported from VEGAS.
#[test]
fn examples_say_they_are_hand_written() {
    for f in examples() {
        let text = std::fs::read_to_string(&f).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let note = v["_note"].as_str().unwrap_or_default();
        assert!(note.contains("hand") && note.contains("not exported from VEGAS"), "{}: {note:?}", f.display());
    }
}
