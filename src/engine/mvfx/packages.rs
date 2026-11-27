//! FX Packages: a named chain of video FX with their settings, applied in one step from the Video FX window
//! ("Filter Packages", as VEGAS calls them) or dropped like any FX. Built-in packages carry the era's signature looks;
//! the user's own (saved from the FX window's chain) live in %APPDATA%\MilanPro\fx_packages.json next to the presets.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use crate::engine::plugins::{self, PluginInfo, UserPreset};
use crate::project::Fx;

/// Words no plug-in, preset or package name may carry: artists, directors, labels and trademarks.
#[cfg(test)]
pub const BANNED: &[&str] = &["vegas", "sony", "magix", "hype w", "meyers", "kanye", "missy", "chemical", "black parade", "fall out", "paramore", "panic!", "lil ", "crank that", "heartbreak", "flashing lights", "g-unit", "dipset", "helena", "movie maker", "windows", "pro-mist", "tiffen", "premiere", "after effects", "final cut"];

/// One FX of a package: a plug-in and its values (as a user preset holds them).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PackageFx {
    pub plugin: String,
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
    #[serde(default)]
    pub text: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    #[serde(default)]
    pub desc: String,
    pub fx: Vec<PackageFx>,
    /// Built in (not saved to, nor deleted from, the user's file).
    #[serde(skip)]
    pub builtin: bool,
}

/// A built-in package: (plug-in id, built-in preset name) in chain order.
struct Builtin {
    name: &'static str,
    desc: &'static str,
    fx: &'static [(&'static str, &'static str)],
}

const BUILTIN: &[Builtin] = &[
    Builtin { name: "Emo Bleach 2005", desc: "Bleach bypass, fine grain and a vignette: the desaturated, crushed look of mid-decade emo videos", fx: &[("milan.fx.bleach_bypass", "Emo 2005 Bleach"), ("milan.fx.film_grain", "Subtle"), ("milan.fx.vignette", "Default")] },
    Builtin { name: "Scene Kid Sepia", desc: "Warm sepia through a diffusion filter with grain", fx: &[("milan.fx.sepia", "Light"), ("milan.fx.soft_contrast", "Warm Vignette"), ("milan.fx.film_grain", "Subtle")] },
    Builtin { name: "Pop-Punk Cross Process", desc: "Cross-processed colour, softened, with 16mm grain", fx: &[("milan.fx.cross_process", "Pop-Punk Cross Process"), ("milan.fx.soft_contrast", "Soft Moderate Contrast"), ("milan.fx.film_grain", "16mm")] },
    Builtin { name: "Silver Smoke Mono", desc: "High-contrast black and white with flicker and a soft glow", fx: &[("milan.fx.black_white", "100% Black and White"), ("milan.fx.bleach_bypass", "Silver Retention"), ("milan.fx.film_effects", "Circa 1980"), ("milan.fx.glow", "White Soft Glow")] },
    Builtin { name: "Super 8 Memory", desc: "Home-movie film: gate weave and dust, heavy grain, a light leak and a dark edge", fx: &[("milan.fx.film_effects", "Super 8 Memory"), ("milan.fx.film_grain", "Super 8"), ("milan.fx.light_leak", "Default"), ("milan.fx.vignette", "Strong")] },
    Builtin { name: "Camcorder 2003", desc: "A consumer camcorder on tape, date stamp and all", fx: &[("milan.fx.vhs", "Camcorder 2003"), ("milan.fx.add_noise", "Default")] },
    Builtin { name: "Webcam 2007", desc: "A bedroom webcam upload: blocky, noisy and choppy", fx: &[("milan.fx.webcam", "Webcam 2007"), ("milan.fx.stutter", "Step Print"), ("milan.fx.add_noise", "Default")] },
    Builtin { name: "Datamosh 2009", desc: "Pictures smearing along the motion of the next, with glitches and split colour", fx: &[("milan.fx.datamosh", "Datamosh 2009"), ("milan.fx.glitch", "Light"), ("milan.fx.rgb_split", "Default")] },
    Builtin { name: "Crunk Bling Glow", desc: "Saturated colour, a gold glow and star glints on every highlight", fx: &[("milan.fx.hsl", "Saturate"), ("milan.fx.glow", "Crunk Bling Glow"), ("milan.fx.starburst", "Bling")] },
    Builtin { name: "Club Strobe", desc: "Strobing white flashes, an intense glow and a shake on the beat", fx: &[("milan.fx.camera_shake", "Beat Hit 120 BPM"), ("milan.fx.glow", "White Intense Glow"), ("milan.fx.strobe", "Club Strobe")] },
    Builtin { name: "Fisheye Widescreen", desc: "A fisheye lens, saturated colour and 2.35 letterbox bars", fx: &[("milan.fx.spherize", "Fisheye"), ("milan.fx.hsl", "Saturate"), ("milan.fx.letterbox", "Default")] },
    Builtin { name: "CRT Broadcast", desc: "Re-shot off a television: aperture grill, interlace and split colour", fx: &[("milan.fx.tv_simulator", "TV Look"), ("milan.fx.rgb_split", "Default")] },
    Builtin { name: "R&B Soft Glow", desc: "Glamour diffusion: a soft, warm, glowing picture", fx: &[("milan.fx.soft_contrast", "R&B Soft Glow"), ("milan.fx.glow", "White Soft Glow")] },
    Builtin { name: "Night Vision Hustle", desc: "Green night-vision with grain and a heavy vignette", fx: &[("milan.fx.night_vision", "Night Vision Hustle"), ("milan.fx.add_noise", "Grainy"), ("milan.fx.vignette", "Strong")] },
    Builtin { name: "Step Print Smear", desc: "Step-printed motion that smears into itself", fx: &[("milan.fx.stutter", "Step Print"), ("milan.fx.echo", "Step Print Smear")] },
];

/// The built-in packages, resolved to values. A row naming a plug-in or preset this build lacks is left out (the
/// `every_builtin_package_loads` test keeps that from happening).
pub fn builtins() -> Vec<Package> {
    BUILTIN
        .iter()
        .filter_map(|b| {
            let fx = b
                .fx
                .iter()
                .map(|&(id, preset)| {
                    let info = plugins::find(id)?;
                    let pr = info.presets.iter().find(|p| p.name == preset)?;
                    Some(PackageFx { plugin: id.into(), params: pr.vals.iter().map(|&(k, v)| (k.to_string(), v)).collect(), text: pr.text.iter().map(|&(k, v)| (k.to_string(), v.to_string())).collect() })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Package { name: b.name.into(), desc: b.desc.into(), fx, builtin: true })
        })
        .collect()
}

/// Built-in packages, then the user's.
pub fn all() -> Vec<Package> {
    let mut v = builtins();
    v.extend(load_user());
    v
}

/// The package's FX as new instances (fresh ids, single keys at 0); FX this build does not know stay, bypassed.
pub fn instantiate(p: &Package) -> Vec<Fx> {
    p.fx
        .iter()
        .map(|f| {
            let u = UserPreset { plugin: f.plugin.clone(), name: String::new(), params: f.params.clone(), text: f.text.clone() };
            match plugins::find(&f.plugin) {
                Some(info) => plugins::new_user_fx(info, &u),
                None => Fx { id: uuid::Uuid::new_v4().to_string(), plugin: f.plugin.clone(), enabled: false, params: Default::default(), text: f.text.clone(), auto: Default::default() },
            }
        })
        .collect()
}

/// A chain's enabled FX at time `t` as a package named `name`.
pub fn of_chain(chain: &[Fx], name: &str, t: f64) -> Package {
    let fx = chain
        .iter()
        .filter(|f| f.enabled)
        .map(|f| {
            let u = plugins::user_preset_of(f, name, t);
            PackageFx { plugin: u.plugin, params: u.params, text: u.text }
        })
        .collect();
    Package { name: name.into(), desc: String::new(), fx, builtin: false }
}

/// A package's chooser tile: its chain on the sample picture.
pub fn thumb(p: &Package) -> eframe::egui::ColorImage {
    let mut img = crate::engine::thumbs::sample();
    for f in instantiate(p) {
        if let Some(mut e) = plugins::eval(&f, 0.0) {
            super::frame_vals(&mut e, 0.0, 30.0);
            crate::engine::vfx::apply(&mut img, &e, 1);
        }
    }
    img
}

/// What a package's chain is called in a tile's hover: its plug-ins.
pub fn summary(p: &Package) -> String {
    p.fx.iter().map(|f| plugins::find(&f.plugin).map_or(f.plugin.as_str(), |i: &'static PluginInfo| i.name)).collect::<Vec<_>>().join(" + ")
}

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Bumped on every save and delete: the chooser lists the packages again.
pub fn generation() -> u64 {
    GENERATION.load(Relaxed)
}

fn path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("MilanPro").join("fx_packages.json"))
}

/// The user's packages; none when the file is missing or unreadable.
pub fn load_user() -> Vec<Package> {
    path().and_then(|p| read(&p).ok()).unwrap_or_default()
}

/// Add `p`, replacing a user package of the same name.
pub fn save_user(p: Package) -> io::Result<()> {
    let r = write(&path().ok_or_else(|| io::Error::other("APPDATA is not set"))?, p);
    GENERATION.fetch_add(1, Relaxed);
    r
}

pub fn delete_user(name: &str) -> io::Result<()> {
    let r = rewrite(&path().ok_or_else(|| io::Error::other("APPDATA is not set"))?, |all| all.retain(|q| q.name != name));
    GENERATION.fetch_add(1, Relaxed);
    r
}

/// A missing file is empty; a damaged one is an error, so a save never overwrites packages it could not read.
fn read(path: &Path) -> io::Result<Vec<Package>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(serde_json::from_str(&s)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

fn write(path: &Path, p: Package) -> io::Result<()> {
    rewrite(path, |all| {
        all.retain(|q| q.name != p.name);
        all.push(p);
    })
}

fn rewrite(path: &Path, f: impl FnOnce(&mut Vec<Package>)) -> io::Result<()> {
    let mut all = read(path)?;
    f(&mut all);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&all)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::plugins::{eval, find, new_fx};

    /// Every built-in package loads: each FX and preset it names exists, the chain renders, and no name carries an
    /// artist or trademark.
    #[test]
    fn every_builtin_package_loads() {
        let b = builtins();
        assert_eq!(b.len(), BUILTIN.len(), "a built-in package names a plug-in or preset that is missing");
        for p in &b {
            assert!(!p.fx.is_empty() && !p.desc.is_empty(), "{}", p.name);
            assert!(BANNED.iter().all(|w| !p.name.to_lowercase().contains(w)), "{}", p.name);
            let chain = instantiate(p);
            assert_eq!(chain.len(), p.fx.len());
            let mut img = crate::engine::thumbs::sample();
            for f in &chain {
                let mut e = eval(f, 0.5).unwrap_or_else(|| panic!("{}: {} runs", p.name, f.plugin));
                crate::engine::mvfx::frame_vals(&mut e, 0.5, 30.0);
                crate::engine::vfx::apply(&mut img, &e, 1);
            }
            // A chain of FX that only read earlier frames (Stutter, Echo) leaves a still as it is.
            let timeless = p.fx.iter().all(|f| crate::engine::mvfx::temporal::reads_sources(&f.plugin));
            assert!(timeless || img.pixels != crate::engine::thumbs::sample().pixels, "{} changes the picture", p.name);
        }
    }

    /// A chain saved as a package, written, read back and applied gives the same FX values; saving again under the
    /// name replaces it; a damaged file is not overwritten.
    #[test]
    fn package_save_apply_round_trip() {
        let dir = std::env::temp_dir().join("milan-agent").join("mvfx").join(format!("packages-{}", std::process::id()));
        let file = dir.join("fx_packages.json");
        let _ = std::fs::remove_dir_all(&dir);
        let glow = find("milan.fx.glow").unwrap();
        let mut chain = vec![new_fx(glow, glow.presets.iter().find(|p| p.name == "Crunk Bling Glow")), new_fx(find("milan.fx.gradient_map").unwrap(), find("milan.fx.gradient_map").unwrap().presets.get(1))];
        let mut off = new_fx(find("milan.fx.invert").unwrap(), None);
        off.enabled = false;
        chain.push(off);
        let pkg = of_chain(&chain, "My Look", 0.0);
        assert_eq!(pkg.fx.len(), 2, "bypassed FX stay out");
        write(&file, pkg.clone()).unwrap();
        write(&file, Package { name: "Other".into(), ..pkg.clone() }).unwrap();
        write(&file, pkg.clone()).unwrap();
        let back = read(&file).unwrap();
        assert_eq!(back.len(), 2);
        let mine = back.iter().find(|p| p.name == "My Look").unwrap();
        assert_eq!(mine, &pkg);
        let applied = instantiate(mine);
        for (a, b) in applied.iter().zip(&chain) {
            assert_eq!(eval(a, 0.0), eval(b, 0.0));
            assert_ne!(a.id, b.id, "fresh instances");
        }
        rewrite(&file, |all| all.retain(|p| p.name != "Other")).unwrap();
        assert_eq!(read(&file).unwrap().len(), 1);
        std::fs::write(&file, "{ damaged").unwrap();
        assert!(write(&file, pkg).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ damaged");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
