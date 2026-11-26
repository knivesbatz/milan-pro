//! Writes the test plug-in's PiPL resources with Milan's own PiPL writer and links them (a `.res` the MSVC linker takes
//! as an input). No resource compiler, PiPL tool or SDK file is involved.
#[path = "../../src/engine/aex/pipl.rs"]
mod pipl;
#[path = "src/effects.rs"]
mod effects;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/effects.rs");
    println!("cargo:rerun-if-changed=../../src/engine/aex/pipl.rs");
    let items: Vec<(&str, u16, Vec<u8>)> = effects::ALL
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let e = pipl::Effect {
                kind: "eFKT".into(),
                name: f.name.into(),
                category: "Milan Test".into(),
                entry: f.entry.into(),
                match_name: f.match_name.into(),
                spec_version: (13, 4),
                version: 1 << 19,
                out_flags: f.out_flags,
                out_flags2: f.out_flags2,
                support_url: String::new(),
            };
            ("PiPL", 16000 + i as u16, pipl::Pipl::of(&e).write())
        })
        .collect();
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("milan-aex-test-pipl.res");
    std::fs::write(&out, pipl::res_file(&items)).expect("writing the .res");
    println!("cargo:rustc-cdylib-link-arg={}", out.display());
}
