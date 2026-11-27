//! R55: the plug-in matrix of docs/COMPATIBILITY.md, written from data so it cannot drift from what Milan Pro does.
//!
//! Sources: `docs/compat/vegas15-inventory.csv` (every plug-in of the VEGAS Pro 15 install, read in place),
//! `docs/compat/ofx-vegas15.csv` (the R33c probe of VEGAS's OFX bundles), `docs/compat/tested-plugins.csv` (Milan's
//! own test plug-ins, the third-party plug-ins named so far, and the interfaces Milan does not host), `vegas_map`
//! and the example exports in `tests/vegas/`.
//!
//! Every row gets exactly one of five statuses. A Native, Adapter or Standard interface row must name tests that exist
//! in the source; a Native row's VEGAS plug-in and an Adapter row's name must appear in an example export whose
//! expected file checks the result, so `vegas_examples::every_example_imports_as_expected` really covers them.
//!
//! `cargo test --release compat_matrix` fails when the section in COMPATIBILITY.md is stale;
//! `MILAN_WRITE_COMPAT=1 cargo test --release compat_matrix` rewrites it.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use crate::engine::plugins;
use crate::interchange::{vegas_examples, vegas_map};

const BEGIN: &str = "<!-- compat-matrix:begin: written by interchange::compat_matrix from docs/compat/*.csv, vegas_map and tests/vegas; do not edit by hand -->";
const END: &str = "<!-- compat-matrix:end -->";

/// The tests behind every VEGAS import claim (Native and Adapter rows).
const IMPORT_TESTS: &[&str] = &["interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter", "interchange::vegas_examples::every_example_imports_as_expected"];

/// DirectX audio names found in a VEGAS DLL that may not be plug-ins VEGAS lists, with the question to settle.
const DX_INVESTIGATE: &[(&str, &str)] = &[("VEGAS Dither", "Named in the DLL's string table beside the Track FX; whether VEGAS lists it as an audio FX is to be checked in VEGAS's Plug-In Manager")];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Status {
    Native,
    Standard,
    Adapter,
    Unsupported,
    Investigate,
}

const STATUSES: [(Status, &str); 5] = [
    (Status::Native, "Native equivalent"),
    (Status::Standard, "Standard interface"),
    (Status::Adapter, "Adapter"),
    (Status::Unsupported, "Unsupported"),
    (Status::Investigate, "Needs investigation"),
];

struct Row {
    plugin: String,
    place: String,
    status: Status,
    detail: String,
    tests: Vec<String>,
}

struct Group {
    title: &'static str,
    intro: &'static str,
    rows: Vec<Row>,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A CSV file (quoted fields with `""` escapes; no newlines inside fields) as rows keyed by the header.
fn csv(rel: &str) -> Vec<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    let split = |line: &str| {
        let (mut out, mut cur, mut quoted, mut chars) = (Vec::new(), String::new(), false, line.chars().peekable());
        while let Some(c) = chars.next() {
            match (c, quoted) {
                ('"', true) if chars.peek() == Some(&'"') => {
                    cur.push('"');
                    chars.next();
                }
                ('"', _) => quoted = !quoted,
                (',', false) => out.push(std::mem::take(&mut cur)),
                _ => cur.push(c),
            }
        }
        out.push(cur);
        out
    };
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let head = split(lines.next().expect("a header"));
    lines
        .map(|l| {
            let f = split(l);
            assert_eq!(f.len(), head.len(), "{rel}: {l}");
            head.iter().cloned().zip(f).collect()
        })
        .collect()
}

/// Whether test `path` (`module::...::fn`) is a function in the source file of its module.
fn test_exists(path: &str) -> bool {
    let segs: Vec<&str> = path.split("::").collect();
    let (name, modules) = segs.split_last().expect("a path");
    let src = root().join("src");
    (1..=modules.len()).rev().any(|n| {
        let base = modules[..n].iter().collect::<PathBuf>();
        [src.join(&base).with_extension("rs"), src.join(&base).join("mod.rs")]
            .iter()
            .filter_map(|f| std::fs::read_to_string(f).ok())
            .any(|t| t.contains(&format!("fn {name}(")))
    })
}

/// The example exports and their expected files, as text.
fn examples() -> Vec<(String, String, String)> {
    vegas_examples::examples()
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let exp = std::fs::read_to_string(p.to_string_lossy().replace(".milan.json", ".expected.json")).expect("an expected file");
            (name, std::fs::read_to_string(&p).unwrap(), exp)
        })
        .collect()
}

/// The example whose export uses `needle` and whose expected file checks `result`.
fn example_for(ex: &[(String, String, String)], needle: &str, result: &str) -> String {
    ex.iter()
        .find(|(_, text, exp)| text.contains(needle) && exp.contains(result))
        .map(|e| e.0.clone())
        .unwrap_or_else(|| panic!("no example in tests/vegas uses {needle} and checks {result}"))
}

fn short(id: &str) -> &str {
    id.rsplit(':').next().unwrap_or(id)
}

fn vegas_rows(ex: &[(String, String, String)]) -> Vec<Group> {
    let inv = csv("docs/compat/vegas15-inventory.csv");
    let probe = csv("docs/compat/ofx-vegas15.csv");
    let probe_of = |file: &str, index: &str| -> Vec<&BTreeMap<String, String>> {
        probe.iter().filter(|r| r["binary"] == file && (index.is_empty() || r["index"] == index)).collect()
    };
    // Each binary has as many inventory rows as the probe found plug-ins in it.
    for file in inv.iter().filter(|r| r["kind"] == "ofx").map(|r| r["file"].as_str()) {
        let n = inv.iter().filter(|r| r["kind"] == "ofx" && r["file"] == file).count();
        assert_eq!(n, probe_of(file, "").len(), "{file}: inventory and probe disagree");
    }
    let mut groups = vec![
        Group {
            title: "VEGAS Pro 15 OFX plug-ins (installed bundles)",
            intro: "One row per plug-in the 7 bundles export (`docs/compat/ofx-vegas15.csv`). None of them runs in Milan Pro \
                    (R33c): a row is Native when `vegas_map` turns the VEGAS plug-in into a Milan built-in on import. The probe \
                    results are its second pass, with VEGAS's program folder on the DLL path (`Vfx1.ofx`, `Filters.ofx` and \
                    `Stabilize.ofx` do not load at all without it). Indexes the probe did not identify are matched to \
                    identifiers by the order of the identifier strings in the binary, which agrees with all 8 it did identify.",
            rows: Vec::new(),
        },
        Group {
            title: "Named in VEGAS 15's `Vfx1.xml` but not in its binary",
            intro: "`Vfx1.ofx`'s resource file names 96 plug-ins; the binary exports 67. These 29 are in no installed binary; \
                    projects from other VEGAS versions can still name them.",
            rows: Vec::new(),
        },
        Group {
            title: "VEGAS DirectX audio plug-ins (`C:\\Program Files (x86)\\VEGAS\\Shared Plug-Ins\\Audio_x64`)",
            intro: "One row per plug-in name in the 11 DLLs (`ErrorReport.dll` there is not a plug-in). Milan Pro hosts no \
                    DirectX plug-in (`docs/compat/dx-audio.md`); the VEGAS import replaces a known name with a Milan audio FX.",
            rows: Vec::new(),
        },
        Group {
            title: "VEGAS legacy video plug-ins (`Video Plug-Ins\\`)",
            intro: "The pre-OFX generation, one row per DLL (the effects inside are not listed by any public interface).",
            rows: Vec::new(),
        },
    ];
    for r in &inv {
        let (kind, file, index, ident, label) = (r["kind"].as_str(), r["file"].as_str(), r["index"].as_str(), r["identifier"].as_str(), r["label"].as_str());
        let mapped = (!ident.is_empty()).then(|| vegas_map::to_milan(ident)).flatten();
        let (g, place) = match kind {
            "ofx" if index.is_empty() => (0, format!("`{file}`")),
            "ofx" => (0, format!("`{file}` #{index}")),
            "ofx-resource-only" => (1, format!("`{file}` resource only")),
            "dx-audio" => (2, format!("`{file}`")),
            "legacy-video" => (3, format!("`{file}`")),
            k => panic!("unknown kind {k}"),
        };
        let plugin = if ident.is_empty() { label.to_string() } else { format!("{label} (`{}`)", short(ident)) };
        let row = if let Some(m) = mapped {
            let info = plugins::find(m.milan).unwrap_or_else(|| panic!("{} is not a built-in", m.milan));
            let example = example_for(ex, ident, m.milan);
            let n = m.params.len() + m.text.len();
            let carried = match n {
                0 => "no settings to carry".to_string(),
                1 => "1 setting carried".to_string(),
                n => format!("{n} settings carried"),
            };
            let detail = format!("`{}` ({}); vegas_map row `{}` → `{}`, {carried}; checked in `tests/vegas/{example}`", m.milan, info.name, short(m.vegas), m.milan);
            Row { plugin, place, status: Status::Native, detail, tests: IMPORT_TESTS.iter().map(|s| s.to_string()).collect() }
        } else {
            match kind {
                "ofx" => {
                    let rows = probe_of(file, index);
                    let p = rows[0];
                    // The HitFilm rows are not told apart by index: their results must all be the same.
                    assert!(rows.iter().all(|x| x["error"] == p["error"] && x["loads"] == p["loads"] && x["describes"] == p["describes"]), "{file}");
                    let err = &p["error"];
                    let why = if err.contains("OfxVegas") {
                        "it asks for VEGAS's private suites (`OfxVegas*`) and host key"
                    } else if err.contains("OfxOpenCLProgramSuite") {
                        "it asks for VEGAS's OpenCL program suite"
                    } else if file.starts_with("HitFilm") {
                        "the VEGAS edition stops in its describe action after reading the host's name"
                    } else {
                        err.as_str()
                    };
                    let detail = format!(
                        "Runs only in VEGAS: {why} (R33c probe: load {}, describe {}, render {}). An instance in a VEGAS project imports as a disabled `ofx:` placeholder that keeps its settings",
                        p["loads"], p["describes"], p["renders"]
                    );
                    Row { plugin, place, status: Status::Unsupported, detail, tests: Vec::new() }
                }
                "ofx-resource-only" => Row {
                    plugin,
                    place,
                    status: Status::Unsupported,
                    detail: "Not in any VEGAS 15 binary and no Milan built-in; a project naming it imports as a disabled `ofx:` placeholder".into(),
                    tests: Vec::new(),
                },
                "dx-audio" => {
                    if let Some((_, why)) = DX_INVESTIGATE.iter().find(|(n, _)| *n == label) {
                        Row { plugin, place, status: Status::Investigate, detail: why.to_string(), tests: Vec::new() }
                    } else if let Some(id) = vegas_map::audio_to_milan(label) {
                        let info = plugins::find(id).unwrap_or_else(|| panic!("{id} is not a built-in"));
                        let example = example_for(ex, &format!("\"{label}\""), id);
                        let detail = format!("VEGAS import by name → `{id}` ({}); settings not carried (the script API exposes no DirectX parameters), a Milan preset of the same name is used when there is one; checked in `tests/vegas/{example}`", info.name);
                        Row { plugin, place, status: Status::Adapter, detail, tests: IMPORT_TESTS.iter().map(|s| s.to_string()).collect() }
                    } else {
                        let detail = "No DirectX host in Milan Pro and no stand-in in `vegas_map`; the import leaves it out and says so".to_string();
                        Row { plugin, place, status: Status::Unsupported, detail, tests: Vec::new() }
                    }
                }
                _ => Row {
                    plugin,
                    place,
                    status: Status::Unsupported,
                    detail: "VEGAS-private plug-in interface from before OFX, with no SDK; licensed with VEGAS".into(),
                    tests: Vec::new(),
                },
            }
        };
        groups[g].rows.push(row);
    }
    // Every vegas_map row is a Native row of some installed or named VEGAS plug-in.
    for m in vegas_map::MAP {
        let found = groups.iter().flat_map(|g| &g.rows).any(|r| r.status == Status::Native && r.plugin.contains(&format!("(`{}`)", short(m.vegas))));
        assert!(found, "vegas_map row {} has no matrix row", m.vegas);
    }
    groups
}

fn other_rows() -> Group {
    let rows = csv("docs/compat/tested-plugins.csv")
        .into_iter()
        .map(|r| {
            let status = match r["status"].as_str() {
                "standard" => Status::Standard,
                "unsupported" => Status::Unsupported,
                "investigate" => Status::Investigate,
                s => panic!("tested-plugins.csv: status {s} (native and adapter rows come from vegas_map)"),
            };
            let detail = format!("{}: {}", r["interface"], r["detail"]);
            Row { plugin: r["plugin"].clone(), place: r["source"].clone(), status, detail, tests: r["tests"].split_whitespace().map(String::from).collect() }
        })
        .collect();
    Group {
        title: "Third-party and test plug-ins, and interfaces",
        intro: "**Only Milan Pro's own test plug-ins have been run.** They are written against Milan Pro's own reading of each \
                interface, so they prove the hosts consistent with themselves, not that every third-party plug-in works. No \
                third-party OFX, VST3, CLAP or AE-format plug-in has been run yet; those named below are candidates. The tests \
                marked `#[ignore]` in the source need the built plug-in host: `cargo build --release`, then \
                `cargo test --release -- --ignored --test-threads=1 ofx:: aplug:: aex::`.",
        rows,
    }
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// The generated section, and the problems found (no test, unknown test, a claim without a test).
fn render() -> (String, Vec<String>) {
    let ex = examples();
    let mut groups = vegas_rows(&ex);
    groups.push(other_rows());
    let mut problems = Vec::new();
    for r in groups.iter().flat_map(|g| &g.rows) {
        let claims = matches!(r.status, Status::Native | Status::Adapter | Status::Standard);
        if claims && r.tests.is_empty() {
            problems.push(format!("{}: {:?} without a test", r.plugin, r.status));
        }
        for t in &r.tests {
            if !test_exists(t) {
                problems.push(format!("{}: test {t} not found in the source", r.plugin));
            }
        }
    }
    let mut s = String::new();
    let _ = writeln!(s, "{BEGIN}\n");
    let _ = writeln!(s, "Written from `docs/compat/vegas15-inventory.csv`, `docs/compat/ofx-vegas15.csv`, `docs/compat/tested-plugins.csv`, \
                        `src/interchange/vegas_map.rs` and `tests/vegas/` by `interchange::compat_matrix`. \
                        `cargo test --release compat_matrix` fails when this section is stale or a row claims support without \
                        a test that exists; `MILAN_WRITE_COMPAT=1 cargo test --release compat_matrix` rewrites it.\n");
    let _ = writeln!(s, "Each row has exactly one status, in its own column:\n");
    let _ = writeln!(s, "- **Native equivalent**: a VEGAS plug-in the VEGAS import turns into a Milan Pro built-in with its settings (the `vegas_map` row).");
    let _ = writeln!(s, "- **Standard interface**: the plug-in runs in Milan Pro's own OFX, VST3, CLAP or AE-format host.");
    let _ = writeln!(s, "- **Adapter**: the VEGAS import replaces it by name with a Milan Pro built-in, without its settings.");
    let _ = writeln!(s, "- **Unsupported**: it does not run in Milan Pro; the reason is given.");
    let _ = writeln!(s, "- **Needs investigation**: what it does in Milan Pro is not known yet; the open question is given.\n");
    let _ = writeln!(s, "| Status | {} | All |", groups.iter().map(|g| g.title.split(" (").next().unwrap_or(g.title).replace('`', "")).collect::<Vec<_>>().join(" | "));
    let _ = writeln!(s, "|---|{}---|", "---|".repeat(groups.len()));
    for (st, name) in STATUSES {
        let counts: Vec<usize> = groups.iter().map(|g| g.rows.iter().filter(|r| r.status == st).count()).collect();
        let _ = writeln!(s, "| {name} | {} | {} |", counts.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" | "), counts.iter().sum::<usize>());
    }
    let totals: Vec<usize> = groups.iter().map(|g| g.rows.len()).collect();
    let _ = writeln!(s, "| **Rows** | {} | {} |", totals.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" | "), totals.iter().sum::<usize>());
    for g in &groups {
        let _ = writeln!(s, "\n#### {} ({})\n\n{}\n", g.title, g.rows.len(), g.intro);
        let _ = writeln!(s, "| Plug-in | Where | Native equivalent | Standard interface | Adapter | Unsupported (reason) | Needs investigation | Tests |");
        let _ = writeln!(s, "|---|---|---|---|---|---|---|---|");
        for r in &g.rows {
            let cols: Vec<String> = STATUSES.iter().map(|(st, _)| if *st == r.status { cell(&r.detail) } else { String::new() }).collect();
            assert_eq!(cols.iter().filter(|c| !c.is_empty()).count(), 1, "{}: one status", r.plugin);
            let tests = r.tests.iter().map(|t| format!("`{t}`")).collect::<Vec<_>>().join("<br>");
            let _ = writeln!(s, "| {} | {} | {} | {} |", cell(&r.plugin), cell(&r.place), cols.join(" | "), tests);
        }
    }
    let _ = write!(s, "\n{END}");
    (s, problems)
}

fn doc() -> PathBuf {
    root().join("docs").join("COMPATIBILITY.md")
}

/// The matrix in COMPATIBILITY.md is the one the data gives; every support claim names a test that exists.
#[test]
fn compatibility_matrix_is_current() {
    let (section, problems) = render();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    let path = doc();
    // Git may check the file out with CRLF line ends: compare without them, write in the file's own.
    let raw = std::fs::read_to_string(&path).expect("docs/COMPATIBILITY.md");
    let crlf = raw.contains("\r\n");
    let text = raw.replace("\r\n", "\n");
    let (a, b) = (text.find(BEGIN).expect("the begin marker"), text.find(END).expect("the end marker") + END.len());
    let (current, fresh) = (&text[a..b], section.as_str());
    if current == fresh {
        return;
    }
    if std::env::var_os("MILAN_WRITE_COMPAT").is_some() {
        let new = format!("{}{fresh}{}", &text[..a], &text[b..]);
        let new = if crlf { new.replace('\n', "\r\n") } else { new };
        std::fs::write(&path, new).expect("write docs/COMPATIBILITY.md");
        return;
    }
    panic!("docs/COMPATIBILITY.md's plug-in matrix is stale: run `MILAN_WRITE_COMPAT=1 cargo test --release compat_matrix` and commit it");
}

/// Test references resolve to functions in their module's file, and a misspelt one does not.
#[test]
fn test_references_resolve_to_functions() {
    assert!(test_exists("interchange::vegas_map::tests::every_row_lands_on_a_built_in_parameter"));
    assert!(test_exists("engine::ofx::tests::describe_through_the_host"));
    assert!(test_exists("interchange::vegas_examples::every_example_imports_as_expected"));
    assert!(!test_exists("interchange::vegas_map::tests::no_such_test"));
    assert!(!test_exists("no::such::module::test"));
}
