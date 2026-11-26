//! R53: every relative link in the repository's Markdown files points at a file or folder that exists.
//!
//! Checked: inline links and images `[text](target)`, reference definitions `[name]: target`, and `src="..."` /
//! `href="..."` attributes of inline HTML. Skipped: absolute URLs (`http:`, `https:`, `mailto:`), pure anchors
//! (`#section`), anything inside code spans and fenced code blocks, and GitHub site paths that leave the repository
//! (README's `../../releases`). The `#fragment` of a file link is ignored; only the file is checked.
use std::path::{Component, Path, PathBuf};

/// Folders that hold no documentation of ours (build output, fetched binaries, git).
const SKIP_DIRS: &[&str] = &[".git", "target", "release", "ffmpeg", "node_modules"];

fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                markdown_files(&p, out);
            }
        } else if name.to_ascii_lowercase().ends_with(".md") {
            out.push(p);
        }
    }
}

/// The text with fenced code blocks and inline code spans blanked out, so links shown as code are not checked.
fn strip_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            out.push('\n');
            continue;
        }
        if fenced {
            out.push('\n');
            continue;
        }
        let mut in_code = false;
        for c in line.chars() {
            if c == '`' {
                in_code = !in_code;
                out.push(' ');
            } else {
                out.push(if in_code { ' ' } else { c });
            }
        }
        out.push('\n');
    }
    out
}

/// Link targets in one Markdown text.
fn targets(text: &str) -> Vec<String> {
    let text = strip_code(text);
    let mut out = Vec::new();
    // Inline links and images: `](target)` or `](target "title")`, with balanced parentheses inside the target.
    let b = text.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b']' && b[i + 1] == b'(' {
            let start = i + 2;
            let mut depth = 1;
            let mut j = start;
            while j < b.len() && b[j] != b'\n' {
                match b[j] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if depth == 0 {
                let inner = text[start..j].trim();
                let target = inner.split_once(" \"").map_or(inner, |(t, _)| t).trim_matches(|c| c == '<' || c == '>');
                out.push(target.to_string());
            }
            i = j;
        }
        i += 1;
    }
    for line in text.lines() {
        // Reference definitions: `[name]: target`.
        let t = line.trim_start();
        if t.starts_with('[') {
            if let Some(end) = t.find("]:") {
                if let Some(target) = t[end + 2..].split_whitespace().next() {
                    out.push(target.to_string());
                }
            }
        }
        // Inline HTML attributes.
        for attr in ["src=\"", "href=\""] {
            let mut rest = line;
            while let Some(k) = rest.find(attr) {
                rest = &rest[k + attr.len()..];
                if let Some(end) = rest.find('"') {
                    out.push(rest[..end].to_string());
                    rest = &rest[end..];
                }
            }
        }
    }
    out
}

/// `path` with `.` and `..` folded; None when it climbs above the root it started from.
fn normalise(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn every_relative_doc_link_resolves() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    markdown_files(root, &mut files);
    files.sort();
    assert!(files.iter().any(|f| f.ends_with("README.md")), "no Markdown files found under {}", root.display());
    let mut broken = Vec::new();
    let mut checked = 0;
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let rel_file = file.strip_prefix(root).unwrap_or(file);
        let dir = rel_file.parent().unwrap_or(Path::new(""));
        for t in targets(&text) {
            let t = t.trim();
            if t.is_empty() || t.starts_with('#') || t.contains("://") || t.starts_with("mailto:") {
                continue;
            }
            let path_part = percent_decode(t.split('#').next().unwrap_or(t));
            let joined = if let Some(abs) = path_part.strip_prefix('/') { PathBuf::from(abs) } else { dir.join(&path_part) };
            // A GitHub site path (../../releases from README) leaves the repository: not a file of ours.
            let Some(rel) = normalise(&joined) else { continue };
            checked += 1;
            if !root.join(&rel).exists() {
                broken.push(format!("{}: {t}", rel_file.display()));
            }
        }
    }
    assert!(checked > 50, "only {checked} relative links found: is the link parser broken?");
    assert!(broken.is_empty(), "{} broken relative link(s):\n{}", broken.len(), broken.join("\n"));
}

#[test]
fn link_parser_finds_each_kind() {
    let md = "See [a](docs/A.md#x) and ![img](i.png \"t\").\n[ref]: ../B.md\n<img src=\"c.png\">\n`[no](code.md)`\n```\n[no](fenced.md)\n```\n[web](https://x.y)\n";
    let t = targets(md);
    assert_eq!(t, vec!["docs/A.md#x", "i.png", "https://x.y", "../B.md", "c.png"]);
}
