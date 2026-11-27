//! R45a: Project Media as a library. Custom Bins and Smart Bins (`Project::bins`), media tags, and the search and
//! smart-bin query language.
//!
//! A query is the text typed in Project Media's search field or saved in a smart bin. Its terms all have to match:
//! a plain word or "quoted phrase" matches the name, path, codec, tags or frame size (`1920x1080`); `field:value`
//! and `field<op>value` (`=`, `>`, `>=`, `<`, `<=`) test one property; `-` before a term negates it.
//!
//! | Field | Values |
//! |---|---|
//! | `type` | `video`, `audio`, `still` (or `image`); several with commas: `type:video,still` |
//! | `name`, `path`, `codec`, `tag` | text (`:` contains, `=` equals) |
//! | `res` | `1920x1080`, or a height (`1080`, `1080p`); with `>=` both sides are compared |
//! | `width`, `height`, `fps` | numbers (`fps:25` is within 0.01) |
//! | `length` (`dur`) | seconds, or `m:ss` / `h:mm:ss` |
//! | `is` | `used`, `unused`, `offline`, `online`, `tagged` |
use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use crate::project::{MediaItem, MediaType, Project};

/// A Custom Bin (a folder of media; bins nest, and an item may be in several) or a Smart Bin (a saved query).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bin {
    pub id: String,
    pub name: String,
    /// The custom bin it is inside; None at the top (under Custom Bins). Smart bins have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Media ids, in the order they were added (custom bins).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<String>,
    /// Some: a smart bin, its query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

impl Bin {
    pub fn is_smart(&self) -> bool {
        self.query.is_some()
    }
}

// ---- Query ----

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    /// `:`, contains (text) or equals (numbers, types).
    Has,
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
}

impl Op {
    pub fn text(self) -> &'static str {
        match self {
            Op::Has => ":",
            Op::Eq => "=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Lt => "<",
            Op::Le => "<=",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    /// A plain word: name, path, codec, tags or frame size.
    Any,
    Name,
    Path,
    Codec,
    Tag,
    Type,
    Res,
    Width,
    Height,
    Fps,
    Length,
    Is,
}

impl Field {
    fn parse(s: &str) -> Option<Field> {
        Some(match s.to_ascii_lowercase().as_str() {
            "name" => Field::Name,
            "path" | "folder" => Field::Path,
            "codec" => Field::Codec,
            "tag" | "tags" => Field::Tag,
            "type" | "kind" => Field::Type,
            "res" | "resolution" | "size" => Field::Res,
            "width" | "w" => Field::Width,
            "height" | "h" => Field::Height,
            "fps" | "rate" => Field::Fps,
            "length" | "len" | "dur" | "duration" => Field::Length,
            "is" | "status" | "usage" => Field::Is,
            _ => return None,
        })
    }

    pub fn key(&self) -> &'static str {
        match self {
            Field::Any => "",
            Field::Name => "name",
            Field::Path => "path",
            Field::Codec => "codec",
            Field::Tag => "tag",
            Field::Type => "type",
            Field::Res => "res",
            Field::Width => "width",
            Field::Height => "height",
            Field::Fps => "fps",
            Field::Length => "length",
            Field::Is => "is",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Term {
    pub not: bool,
    pub field: Field,
    pub op: Op,
    /// As typed (unquoted).
    pub value: String,
    /// `value` lower-cased, for text matches.
    lower: String,
}

impl Term {
    pub fn new(not: bool, field: Field, op: Op, value: &str) -> Term {
        Term { not, field, op, value: value.to_string(), lower: value.to_lowercase() }
    }

    /// The term as query text (`-tag:"b roll"`).
    pub fn text(&self) -> String {
        let v = if self.value.is_empty() || self.value.contains([' ', '"', ':', '<', '>', '=']) || self.value.starts_with('-') {
            format!("\"{}\"", self.value.replace('"', ""))
        } else {
            self.value.clone()
        };
        let neg = if self.not { "-" } else { "" };
        match self.field {
            Field::Any => format!("{neg}{v}"),
            _ => format!("{neg}{}{}{v}", self.field.key(), self.op.text()),
        }
    }
}

/// A parsed query: every term must match. An empty query matches everything.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query {
    pub terms: Vec<Term>,
}

/// Split `s` into words, keeping "quoted phrases" (quotes dropped) together, and `field:"a b"` as one word.
fn words(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted, mut any) = (Vec::new(), String::new(), false, false);
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

impl Query {
    pub fn parse(s: &str) -> Query {
        let mut terms = Vec::new();
        for w in words(s) {
            let (not, w) = match w.strip_prefix('-') {
                Some(rest) if !rest.is_empty() => (true, rest.to_string()),
                _ => (false, w),
            };
            // `field<op>value` where field is a known name; anything else is a plain word.
            let at = w.find([':', '=', '<', '>']);
            let parsed = at.and_then(|i| {
                let field = Field::parse(&w[..i])?;
                let rest = &w[i..];
                let (op, n) = [(">=", Op::Ge), ("<=", Op::Le), (":", Op::Has), ("=", Op::Eq), (">", Op::Gt), ("<", Op::Lt)].into_iter().find(|(t, _)| rest.starts_with(t)).map(|(t, op)| (op, t.len()))?;
                Some(Term::new(not, field, op, rest[n..].trim()))
            });
            terms.push(parsed.unwrap_or_else(|| Term::new(not, Field::Any, Op::Has, &w)));
        }
        Query { terms }
    }

    pub fn text(&self) -> String {
        self.terms.iter().map(Term::text).collect::<Vec<_>>().join(" ")
    }

    pub fn matches(&self, m: &MediaItem, cx: &Ctx) -> bool {
        self.terms.iter().all(|t| t.matches(m, cx) != t.not)
    }
}

/// What a query needs beyond the item: which media the timeline uses.
#[derive(Default)]
pub struct Ctx {
    pub used: HashSet<String>,
}

impl Ctx {
    pub fn of(project: &Project) -> Ctx {
        let tl = &project.timeline;
        let mut used = HashSet::new();
        // Inactive takes count: their media must stay in the project.
        for c in tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips) {
            used.extend(c.media_id.iter().cloned());
            used.extend(c.takes.iter().map(|t| t.media_id.clone()));
        }
        Ctx { used }
    }
}

fn contains_ci(hay: &str, lower_needle: &str) -> bool {
    if hay.is_ascii() && lower_needle.is_ascii() {
        let (h, n) = (hay.as_bytes(), lower_needle.as_bytes());
        return n.is_empty() || h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n));
    }
    hay.to_lowercase().contains(lower_needle)
}

fn text_match(hay: &str, t: &Term) -> bool {
    match t.op {
        Op::Eq => hay.eq_ignore_ascii_case(&t.value) || hay.to_lowercase() == t.lower,
        _ => contains_ci(hay, &t.lower),
    }
}

fn cmp(v: f64, op: Op, want: f64, tol: f64) -> bool {
    match op {
        Op::Has | Op::Eq => (v - want).abs() <= tol,
        Op::Gt => v > want + tol,
        Op::Ge => v >= want - tol,
        Op::Lt => v < want - tol,
        Op::Le => v <= want + tol,
    }
}

/// "90", "1:30", "1:00:00", "2.5s", "3m" -> seconds.
pub fn parse_seconds(s: &str) -> Option<f64> {
    let s = s.trim().to_ascii_lowercase();
    if let Some(m) = s.strip_suffix('m') {
        return m.parse::<f64>().ok().map(|v| v * 60.0);
    }
    let s = s.strip_suffix('s').unwrap_or(&s);
    s.split(':').try_fold(0.0, |acc, p| p.parse::<f64>().ok().map(|v| acc * 60.0 + v))
}

/// `1920x1080` -> (Some(1920), 1080); `1080p` / `1080` -> (None, 1080).
fn parse_res(s: &str) -> Option<(Option<u32>, u32)> {
    let s = s.trim().to_ascii_lowercase();
    match s.split_once(['x', '×']) {
        Some((w, h)) => Some((Some(w.trim().parse().ok()?), h.trim().parse().ok()?)),
        None => Some((None, s.trim_end_matches(['p', 'i']).parse().ok()?)),
    }
}

/// The type as Project Media names it.
pub fn type_name(t: MediaType) -> &'static str {
    match t {
        MediaType::Video => "video",
        MediaType::Audio => "audio",
        MediaType::Image => "still",
        MediaType::Unknown => "unknown",
    }
}

impl Term {
    fn matches(&self, m: &MediaItem, cx: &Ctx) -> bool {
        match self.field {
            Field::Any => {
                let size = match (m.width, m.height) {
                    (Some(w), Some(h)) => format!("{w}x{h}"),
                    _ => String::new(),
                };
                contains_ci(&m.name, &self.lower)
                    || contains_ci(&m.path.to_string_lossy(), &self.lower)
                    || m.codec.as_deref().is_some_and(|c| contains_ci(c, &self.lower))
                    || m.tags.iter().any(|t| contains_ci(t, &self.lower))
                    || (!size.is_empty() && contains_ci(&size, &self.lower))
            }
            Field::Name => text_match(&m.name, self),
            Field::Path => text_match(&m.path.to_string_lossy(), self),
            Field::Codec => m.codec.as_deref().is_some_and(|c| text_match(c, self)),
            Field::Tag => m.tags.iter().any(|t| if self.op == Op::Has { contains_ci(t, &self.lower) } else { t.eq_ignore_ascii_case(&self.value) }),
            Field::Type => self.lower.split(',').map(str::trim).any(|v| {
                let v = match v {
                    "image" | "images" | "stills" | "picture" => "still",
                    "videos" => "video",
                    v => v,
                };
                type_name(m.media_type) == v
            }),
            Field::Res => {
                let (Some(w), Some(h)) = (m.width, m.height) else { return false };
                let Some((ww, wh)) = parse_res(&self.value) else { return false };
                match ww {
                    Some(ww) => cmp(w as f64, self.op, ww as f64, 0.0) && cmp(h as f64, self.op, wh as f64, 0.0),
                    None => cmp(h as f64, self.op, wh as f64, 0.0),
                }
            }
            Field::Width => m.width.zip(self.value.parse::<f64>().ok()).is_some_and(|(w, v)| cmp(w as f64, self.op, v, 0.0)),
            Field::Height => m.height.zip(self.value.parse::<f64>().ok()).is_some_and(|(h, v)| cmp(h as f64, self.op, v, 0.0)),
            Field::Fps => m.frame_rate.zip(self.value.parse::<f64>().ok()).is_some_and(|(f, v)| cmp(f, self.op, v, 0.01)),
            Field::Length => parse_seconds(&self.value).is_some_and(|v| m.media_type != MediaType::Image && cmp(m.duration, self.op, v, 1e-6)),
            Field::Is => match self.lower.as_str() {
                "used" => cx.used.contains(&m.id),
                "unused" => !cx.used.contains(&m.id),
                "offline" | "missing" => m.offline(),
                "online" => !m.offline(),
                "tagged" => !m.tags.is_empty(),
                _ => false,
            },
        }
    }
}

// ---- Bins ----

pub fn bin<'a>(project: &'a Project, id: &str) -> Option<&'a Bin> {
    project.bins.iter().find(|b| b.id == id)
}

fn bin_mut<'a>(project: &'a mut Project, id: &str) -> Option<&'a mut Bin> {
    project.bins.iter_mut().find(|b| b.id == id)
}

/// A new custom bin named `name` inside `parent` (None: at the top); its id.
pub fn new_bin(project: &mut Project, name: &str, parent: Option<&str>) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let parent = parent.filter(|p| bin(project, p).is_some_and(|b| !b.is_smart())).map(str::to_string);
    project.bins.push(Bin { id: id.clone(), name: unique_name(project, name, parent.as_deref(), false), parent, ..Default::default() });
    project.mark_modified();
    id
}

/// A new smart bin for `query`; its id.
pub fn new_smart_bin(project: &mut Project, name: &str, query: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    project.bins.push(Bin { id: id.clone(), name: unique_name(project, name, None, true), query: Some(query.trim().to_string()), ..Default::default() });
    project.mark_modified();
    id
}

/// `name`, or `name 2`, `name 3`... when a sibling has it, as Windows Explorer names new folders.
fn unique_name(project: &Project, name: &str, parent: Option<&str>, smart: bool) -> String {
    let taken = |n: &str| project.bins.iter().any(|b| b.is_smart() == smart && b.parent.as_deref() == parent && b.name.eq_ignore_ascii_case(n));
    let base = if name.trim().is_empty() { "New Bin" } else { name.trim() };
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|k| format!("{base} {k}")).find(|n| !taken(n)).expect("a free number")
}

pub fn rename_bin(project: &mut Project, id: &str, name: &str) -> bool {
    let (parent, smart) = match bin(project, id) {
        Some(b) if !name.trim().is_empty() && b.name != name.trim() => (b.parent.clone(), b.is_smart()),
        _ => return false,
    };
    let name = unique_name(project, name, parent.as_deref(), smart);
    if let Some(b) = bin_mut(project, id) {
        b.name = name;
    }
    project.mark_modified();
    true
}

pub fn set_query(project: &mut Project, id: &str, query: &str) -> bool {
    match bin_mut(project, id) {
        Some(b) if b.is_smart() && b.query.as_deref() != Some(query.trim()) => b.query = Some(query.trim().to_string()),
        _ => return false,
    }
    project.mark_modified();
    true
}

/// The bins inside `id` at every depth, `id` first.
pub fn subtree(project: &Project, id: &str) -> Vec<String> {
    let mut out = vec![id.to_string()];
    let mut k = 0;
    while k < out.len() {
        let p = out[k].clone();
        out.extend(project.bins.iter().filter(|b| b.parent.as_deref() == Some(p.as_str())).map(|b| b.id.clone()));
        k += 1;
    }
    out
}

/// Delete bin `id` and the bins inside it (the media stay in the project, as in VEGAS).
pub fn delete_bin(project: &mut Project, id: &str) -> bool {
    if bin(project, id).is_none() {
        return false;
    }
    let gone: HashSet<String> = subtree(project, id).into_iter().collect();
    project.bins.retain(|b| !gone.contains(&b.id));
    project.mark_modified();
    true
}

/// Put media `ids` in custom bin `bin` (those already there are left). True if any was added.
pub fn add_to_bin(project: &mut Project, bin: &str, ids: &[String]) -> bool {
    let known: HashSet<&str> = project.media_pool.iter().map(|m| m.id.as_str()).collect();
    let ids: Vec<String> = ids.iter().filter(|i| known.contains(i.as_str())).cloned().collect();
    let Some(b) = bin_mut(project, bin).filter(|b| !b.is_smart()) else { return false };
    let before = b.media.len();
    for id in ids {
        if !b.media.contains(&id) {
            b.media.push(id);
        }
    }
    let added = b.media.len() > before;
    if added {
        project.mark_modified();
    }
    added
}

/// Remove from Current Bin: take `ids` out of custom bin `bin`. True if any was there.
pub fn remove_from_bin(project: &mut Project, bin: &str, ids: &[String]) -> bool {
    let Some(b) = bin_mut(project, bin) else { return false };
    let before = b.media.len();
    b.media.retain(|m| !ids.contains(m));
    let removed = b.media.len() < before;
    if removed {
        project.mark_modified();
    }
    removed
}

/// Move bin `id` inside `parent` (None: to the top). Refused for a smart bin, onto itself or into its own subtree.
pub fn move_bin(project: &mut Project, id: &str, parent: Option<&str>) -> bool {
    let Some(b) = bin(project, id).filter(|b| !b.is_smart()) else { return false };
    if b.parent.as_deref() == parent || parent.is_some_and(|p| subtree(project, id).iter().any(|s| s == p) || bin(project, p).is_none_or(Bin::is_smart)) {
        return false;
    }
    let name = b.name.clone();
    let name = unique_name(project, &name, parent, false);
    if let Some(b) = bin_mut(project, id) {
        (b.parent, b.name) = (parent.map(str::to_string), name);
    }
    project.mark_modified();
    true
}

/// Media removed from the project leave every bin.
pub fn forget_media(project: &mut Project, ids: &HashSet<String>) {
    for b in &mut project.bins {
        b.media.retain(|m| !ids.contains(m));
    }
}

/// Tags of media `ids` set to `tags` (trimmed, duplicates dropped, case kept as first typed). True if any changed.
pub fn set_tags(project: &mut Project, ids: &[String], tags: &[String]) -> bool {
    let mut clean: Vec<String> = Vec::new();
    for t in tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if !clean.iter().any(|c| c.eq_ignore_ascii_case(t)) {
            clean.push(t.to_string());
        }
    }
    let mut changed = false;
    for m in project.media_pool.iter_mut().filter(|m| ids.contains(&m.id)) {
        if m.tags != clean {
            m.tags = clean.clone();
            changed = true;
        }
    }
    if changed {
        project.mark_modified();
    }
    changed
}

/// "interview, b roll ,  Day 1" -> the tags.
pub fn parse_tags(s: &str) -> Vec<String> {
    s.split([',', ';']).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
}

/// Every tag in the pool with how many items carry it, sorted by name (the Tagged Media node's children).
pub fn all_tags(pool: &[MediaItem]) -> Vec<(String, usize)> {
    let mut count: HashMap<String, (String, usize)> = HashMap::new();
    for t in pool.iter().flat_map(|m| &m.tags) {
        count.entry(t.to_lowercase()).or_insert_with(|| (t.clone(), 0)).1 += 1;
    }
    let mut v: Vec<(String, usize)> = count.into_values().collect();
    v.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::History;

    fn item(id: &str, name: &str, t: MediaType) -> MediaItem {
        MediaItem { id: id.into(), name: name.into(), path: format!(r"D:\Footage\{name}").into(), media_type: t, sample_rate: Some(48_000), ..Default::default() }
    }

    fn library() -> (Project, Ctx) {
        let mut p = Project::new("t");
        let mut a = item("a", "interview_A.mov", MediaType::Video);
        (a.width, a.height, a.frame_rate, a.codec, a.duration) = (Some(1920), Some(1080), Some(25.0), Some("prores".into()), 125.0);
        a.tags = vec!["Interview".into(), "Day 1".into()];
        let mut b = item("b", "drone 4k.mp4", MediaType::Video);
        (b.width, b.height, b.frame_rate, b.codec, b.duration) = (Some(3840), Some(2160), Some(29.97), Some("hevc".into()), 40.0);
        b.tags = vec!["b roll".into()];
        let mut c = item("c", "music.wav", MediaType::Audio);
        (c.codec, c.duration) = (Some("pcm_s24le".into()), 200.0);
        let mut d = item("d", "logo.png", MediaType::Image);
        (d.width, d.height, d.codec, d.duration) = (Some(800), Some(600), Some("png".into()), 5.0);
        d.missing = true;
        p.media_pool = vec![a, b, c, d];
        let mut cx = Ctx::default();
        cx.used.insert("a".into());
        (p, cx)
    }

    fn hits(p: &Project, cx: &Ctx, q: &str) -> Vec<String> {
        let q = Query::parse(q);
        p.media_pool.iter().filter(|m| q.matches(m, cx)).map(|m| m.id.clone()).collect()
    }

    #[test]
    fn the_query_parser_reads_fields_ops_quotes_and_negation() {
        let q = Query::parse(r#"interview -tag:"b roll" res>=1920x1080 fps:25 length<2:00 is:unused type:video,still name="logo.png" codec: x:y"#);
        let t = &q.terms;
        assert_eq!(t.len(), 10);
        assert_eq!((t[0].field.clone(), t[0].value.as_str(), t[0].not), (Field::Any, "interview", false));
        assert_eq!((t[1].field.clone(), t[1].op, t[1].value.as_str(), t[1].not), (Field::Tag, Op::Has, "b roll", true));
        assert_eq!((t[2].field.clone(), t[2].op, t[2].value.as_str()), (Field::Res, Op::Ge, "1920x1080"));
        assert_eq!((t[3].field.clone(), t[3].op), (Field::Fps, Op::Has));
        assert_eq!((t[4].field.clone(), t[4].op, t[4].value.as_str()), (Field::Length, Op::Lt, "2:00"));
        assert_eq!((t[5].field.clone(), t[5].value.as_str()), (Field::Is, "unused"));
        assert_eq!((t[7].field.clone(), t[7].op, t[7].value.as_str()), (Field::Name, Op::Eq, "logo.png"));
        assert_eq!((t[8].field.clone(), t[8].value.as_str()), (Field::Codec, ""));
        assert_eq!(t[9].field, Field::Any, "an unknown field is a plain word");
        // The text round-trips.
        assert_eq!(Query::parse(&q.text()), q);
        assert_eq!(Query::parse("  ").terms.len(), 0);
        assert_eq!(Query::parse("-").terms[0].value, "-", "a lone dash is a word");
        assert_eq!((parse_seconds("1:30"), parse_seconds("90s"), parse_seconds("2m"), parse_seconds("1:00:00"), parse_seconds("x")), (Some(90.0), Some(90.0), Some(120.0), Some(3600.0), None));
    }

    #[test]
    fn queries_evaluate_over_every_field() {
        let (p, cx) = library();
        assert_eq!(hits(&p, &cx, ""), ["a", "b", "c", "d"]);
        // Plain words: name, path, codec, tags and frame size, any case.
        assert_eq!(hits(&p, &cx, "INTERVIEW"), ["a"]);
        assert_eq!(hits(&p, &cx, "footage"), ["a", "b", "c", "d"]);
        assert_eq!(hits(&p, &cx, "hevc"), ["b"]);
        assert_eq!(hits(&p, &cx, "\"b roll\""), ["b"]);
        assert_eq!(hits(&p, &cx, "3840x2160"), ["b"]);
        assert_eq!(hits(&p, &cx, "drone 4k"), ["b"], "every word must match");
        assert_eq!(hits(&p, &cx, "type:video"), ["a", "b"]);
        assert_eq!(hits(&p, &cx, "type:audio,still"), ["c", "d"]);
        assert_eq!(hits(&p, &cx, "-type:video"), ["c", "d"]);
        assert_eq!(hits(&p, &cx, "codec:pcm"), ["c"]);
        assert_eq!(hits(&p, &cx, "codec=png"), ["d"]);
        assert_eq!(hits(&p, &cx, "res:1080"), ["a"]);
        assert_eq!(hits(&p, &cx, "res>=1920x1080"), ["a", "b"]);
        assert_eq!(hits(&p, &cx, "res<1920x1080"), ["d"]);
        assert_eq!(hits(&p, &cx, "res>1080p"), ["b"]);
        assert_eq!(hits(&p, &cx, "fps:29.97"), ["b"]);
        assert_eq!(hits(&p, &cx, "fps>=25"), ["a", "b"]);
        assert_eq!(hits(&p, &cx, "width<1000"), ["d"]);
        assert_eq!(hits(&p, &cx, "length>2:00"), ["a", "c"]);
        assert_eq!(hits(&p, &cx, "length<=40"), ["b"], "stills have no length");
        assert_eq!(hits(&p, &cx, "tag:day"), ["a"]);
        assert_eq!(hits(&p, &cx, "tag=\"day 1\""), ["a"]);
        assert_eq!(hits(&p, &cx, "tag=day"), Vec::<String>::new());
        assert_eq!(hits(&p, &cx, "is:used"), ["a"]);
        assert_eq!(hits(&p, &cx, "is:unused"), ["b", "c", "d"]);
        assert_eq!(hits(&p, &cx, "is:offline"), ["d"]);
        assert_eq!(hits(&p, &cx, "is:online type:still"), Vec::<String>::new());
        assert_eq!(hits(&p, &cx, "is:tagged"), ["a", "b"]);
        assert_eq!(hits(&p, &cx, "type:video is:unused res>=3840x2160"), ["b"]);
        // The used set counts takes too.
        let mut p2 = p.clone();
        p2.timeline.add_video_track();
        let mut clip = crate::project::Clip::new("c", 0.0, 1.0);
        clip.media_id = Some("c".into());
        clip.takes = vec![crate::project::Take { media_id: "b".into(), in_point: 0.0, name: "b".into() }];
        p2.timeline.video_tracks[0].clips.push(clip);
        let cx2 = Ctx::of(&p2);
        assert!(cx2.used.contains("b") && cx2.used.contains("c") && !cx2.used.contains("a"));
    }

    #[test]
    fn bin_moves_are_undoable() {
        let (mut p, _) = library();
        let mut h = History::default();
        h.push(p.clone());
        let a = new_bin(&mut p, "Interviews", None);
        let b = new_bin(&mut p, "Interviews", None);
        assert_eq!(bin(&p, &b).unwrap().name, "Interviews 2");
        h.push(p.clone());
        assert!(add_to_bin(&mut p, &a, &["a".into(), "c".into(), "nope".into()]));
        h.push(p.clone());
        // An item may be in several bins.
        assert!(add_to_bin(&mut p, &b, &["a".into()]));
        assert!(!add_to_bin(&mut p, &b, &["a".into()]), "already there");
        h.push(p.clone());
        assert!(move_bin(&mut p, &b, Some(&a)));
        assert!(!move_bin(&mut p, &a, Some(&b)), "not into its own subtree");
        assert!(!move_bin(&mut p, &a, Some(&a)));
        assert_eq!(bin(&p, &b).unwrap().parent.as_deref(), Some(a.as_str()));
        assert_eq!(bin(&p, &b).unwrap().name, "Interviews 2", "a move keeps the name when it is free");
        h.push(p.clone());
        assert!(remove_from_bin(&mut p, &a, &["c".into()]));
        assert_eq!(bin(&p, &a).unwrap().media, ["a"]);
        // Undo walks every step back.
        assert!(h.undo(&mut p));
        assert_eq!(bin(&p, &a).unwrap().media, ["a", "c"]);
        assert!(h.undo(&mut p));
        assert_eq!(bin(&p, &b).unwrap().parent, None);
        assert!(h.undo(&mut p));
        assert!(bin(&p, &b).unwrap().media.is_empty());
        assert!(h.undo(&mut p));
        assert!(bin(&p, &a).unwrap().media.is_empty());
        assert!(h.redo(&mut p) && h.redo(&mut p) && h.redo(&mut p));
        assert_eq!(bin(&p, &b).unwrap().parent.as_deref(), Some(a.as_str()));
        // Deleting a bin deletes the bins inside it, never media.
        assert!(delete_bin(&mut p, &a));
        assert!(p.bins.is_empty() && p.media_pool.len() == 4);
        assert!(h.undo(&mut p));
        assert_eq!(p.bins.len(), 2);
        // Removed media leave their bins.
        forget_media(&mut p, &["a".to_string()].into_iter().collect());
        assert!(bin(&p, &b).unwrap().media.is_empty());
        // Smart bins: a query, renamed and edited; never a parent.
        let s = new_smart_bin(&mut p, "4K", "res>=3840x2160");
        assert!(!add_to_bin(&mut p, &s, &["b".into()]) && !move_bin(&mut p, &b, Some(&s)));
        assert!(set_query(&mut p, &s, "type:video") && rename_bin(&mut p, &s, "Video"));
        assert_eq!((bin(&p, &s).unwrap().name.as_str(), bin(&p, &s).unwrap().query.as_deref()), ("Video", Some("type:video")));
    }

    #[test]
    fn tags_are_set_and_counted() {
        let (mut p, _) = library();
        assert!(set_tags(&mut p, &["c".into(), "d".into()], &parse_tags(" Music ; logo,  music, ")));
        assert_eq!(p.media_pool[2].tags, ["Music", "logo"]);
        assert!(!set_tags(&mut p, &["c".into()], &["Music".into(), "logo".into()]));
        let tags = all_tags(&p.media_pool);
        assert_eq!(tags.iter().map(|(t, n)| format!("{t}:{n}")).collect::<Vec<_>>(), ["b roll:1", "Day 1:1", "Interview:1", "logo:2", "Music:2"]);
    }

    /// Old projects (no `bins`, no `tags`) load unchanged and write neither back.
    #[test]
    fn old_projects_load_without_bins_or_tags() {
        let p = Project::new("old");
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("\"bins\"") && !json.contains("\"tags\""));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert!(back.bins.is_empty());
        let (mut lib, _) = library();
        new_bin(&mut lib, "B", None);
        let back: Project = serde_json::from_str(&serde_json::to_string(&lib).unwrap()).unwrap();
        assert_eq!((back.bins.len(), back.media_pool[0].tags.len()), (1, 2));
        assert!(!back.media_pool[3].missing, "the offline mark is found again on open, not stored");
    }


}
