//! A0 (docs/AE_PREMIERE_PLUGINS.md §2.6, decision of 2026-11-26): no Adobe SDK material in the tree. The effect
//! plug-in host (`src/bin/milan-plugin-host/aex`) is written from public documentation with its own declarations;
//! this check fails `cargo test` (and so CI) when a file carries the SDK's copyright line, its confidentiality banner,
//! or the name of its main effect header. The banned strings are assembled here from pieces so that this file and the
//! docs that describe the rule do not trip it themselves.
use std::path::{Path, PathBuf};

/// The banned strings, each in two pieces.
const BANNED: [[&str; 2]; 3] = [["Adobe Systems", " Incorporated"], ["ADOBE", " CONFIDENTIAL"], ["AE_Eff", "ect.h"]];

fn banned() -> Vec<String> {
    BANNED.iter().map(|p| p.concat()).collect()
}

/// Folders never searched: build output and git's own data.
fn skipped(dir: &Path) -> bool {
    dir.file_name().is_some_and(|n| n == "target" || n == ".git")
}

/// Every file under `root` that contains a banned string (whitespace runs, line breaks included, count as one space),
/// with the string.
fn hits(root: &Path) -> Vec<(PathBuf, String)> {
    let banned = banned();
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !skipped(&p) {
                    stack.push(p);
                }
            } else if ft.is_file() {
                let Ok(bytes) = std::fs::read(&p) else { continue };
                let text = String::from_utf8_lossy(&bytes);
                let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
                for b in &banned {
                    if flat.contains(b.as_str()) {
                        out.push((p.clone(), b.clone()));
                    }
                }
            }
        }
    }
    out.sort();
    out
}

#[test]
fn the_tree_has_no_adobe_sdk_material() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let found = hits(&root);
    assert!(found.is_empty(), "Adobe SDK material in the tree (see CONTRIBUTING.md, AE-format effects): {found:?}");
}

/// The check fires: a fixture folder with each banned string (one split over two lines, one inside binary bytes, one in
/// a folder that is searched) is caught, and its build output folder is not searched.
#[test]
fn the_check_fires_on_a_fixture() {
    let dir = std::env::temp_dir().join("milan-agent").join("aex").join(format!("banned-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src").join("deep")).unwrap();
    std::fs::create_dir_all(dir.join("target")).unwrap();
    let b = banned();
    std::fs::write(dir.join("header.txt"), format!("// Copyright 2019 {}\n", b[0])).unwrap();
    // The banner broken over two lines, as a wrapped comment would be.
    std::fs::write(dir.join("src").join("deep").join("notes.md"), b[1].replace(' ', "\n   ")).unwrap();
    let mut bin = vec![0u8, 255, 7, 0];
    bin.extend(format!("#include \"{}\"", b[2]).bytes());
    bin.extend([0u8, 1, 2]);
    std::fs::write(dir.join("src").join("blob.bin"), bin).unwrap();
    std::fs::write(dir.join("target").join("ignored.txt"), &b[0]).unwrap();
    std::fs::write(dir.join("clean.rs"), "fn main() {}\n").unwrap();
    let found = hits(&dir);
    let names: Vec<String> = found.iter().map(|(p, s)| format!("{} {s}", p.strip_prefix(&dir).unwrap().display())).collect();
    assert_eq!(found.len(), 3, "{names:?}");
    assert!(found.iter().all(|(p, _)| !p.starts_with(dir.join("target"))), "build output is not searched");
    assert_eq!(found.iter().map(|f| f.1.clone()).collect::<std::collections::BTreeSet<_>>().len(), 3, "each string is caught");
    std::fs::remove_dir_all(&dir).unwrap();
}
