//! R29: the small XML layer FCP7 XML and FCPXML share: a document tree read with quick-xml (0.41, MIT) and a
//! writer that indents and escapes. Both formats are element trees with short text leaves, so a tree in memory is
//! the simplest thing that reads them whatever order their elements come in.
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::Event;

/// One element: name, attributes, children in order, and the text directly inside it (joined).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<El>,
    pub text: String,
}

impl El {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The first child element called `name`.
    pub fn child(&self, name: &str) -> Option<&El> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The text of the child `name` (trimmed), if any.
    pub fn text_of(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.trim()).filter(|t| !t.is_empty())
    }

    pub fn f64_of(&self, name: &str) -> Option<f64> {
        self.text_of(name).and_then(|t| t.parse().ok())
    }

    pub fn i64_of(&self, name: &str) -> Option<i64> {
        self.text_of(name).and_then(|t| t.parse().ok())
    }

    /// `TRUE` / `true` / `1`.
    pub fn bool_of(&self, name: &str) -> Option<bool> {
        self.text_of(name).map(|t| t.eq_ignore_ascii_case("true") || t == "1")
    }

    /// The first descendant called `name`, depth first.
    pub fn find(&self, name: &str) -> Option<&El> {
        self.children.iter().find_map(|c| if c.name == name { Some(c) } else { c.find(name) })
    }
}

fn element(e: &quick_xml::events::BytesStart) -> anyhow::Result<El> {
    let mut el = El { name: String::from_utf8_lossy(e.local_name().as_ref()).into_owned(), ..Default::default() };
    for a in e.attributes() {
        let a = a?;
        el.attrs.push((String::from_utf8_lossy(a.key.local_name().as_ref()).into_owned(), a.normalized_value(XmlVersion::Implicit1_0)?.into_owned()));
    }
    Ok(el)
}

/// Parse `text` into its root element.
pub fn parse(text: &str) -> anyhow::Result<El> {
    let text = text.trim_start_matches('\u{feff}');
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<El> = vec![El { name: "#document".into(), ..Default::default() }];
    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                stack.push(element(&e)?);
            }
            // `<x/>`: a child with nothing inside.
            Event::Empty(e) => {
                let el = element(&e)?;
                stack.last_mut().expect("root").children.push(el);
            }
            Event::End(_) => {
                let el = stack.pop().expect("balanced");
                stack.last_mut().expect("root").children.push(el);
            }
            Event::Text(t) => {
                // Whitespace between elements is layout, not text; inside a leaf it stays (`text_of` trims).
                let s = t.xml_content(XmlVersion::Implicit1_0)?;
                if !s.trim().is_empty() {
                    stack.last_mut().expect("root").text.push_str(&s);
                }
            }
            // quick-xml 0.38+ hands `&amp;` and `&#64;` over as events of their own.
            Event::GeneralRef(r) => {
                let text = match r.resolve_char_ref()? {
                    Some(c) => c.to_string(),
                    None => match r.decode()?.as_ref() {
                        "amp" => "&".into(),
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        other => format!("&{other};"),
                    },
                };
                stack.last_mut().expect("root").text.push_str(&text);
            }
            Event::CData(c) => {
                let cur = stack.last_mut().expect("root");
                cur.text.push_str(&String::from_utf8_lossy(&c));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut doc = stack.pop().ok_or_else(|| anyhow::anyhow!("unbalanced XML"))?;
    anyhow::ensure!(stack.is_empty(), "unbalanced XML");
    doc.children.pop().ok_or_else(|| anyhow::anyhow!("no root element"))
}

/// A writer that indents two spaces per level and escapes text and attribute values.
pub struct Xml {
    pub out: String,
    depth: usize,
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c => o.push(c),
        }
    }
    o
}

impl Xml {
    pub fn new(doctype: &str) -> Self {
        Self { out: format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{doctype}\n"), depth: 0 }
    }

    fn indent(&mut self) {
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
    }

    fn attrs(&mut self, attrs: &[(&str, &str)]) {
        for (k, v) in attrs {
            self.out.push_str(&format!(" {k}=\"{}\"", escape(v)));
        }
    }

    /// `<tag attrs>` on its own line; close with `close`.
    pub fn open(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        self.attrs(attrs);
        self.out.push_str(">\n");
        self.depth += 1;
    }

    pub fn close(&mut self, tag: &str) {
        self.depth = self.depth.saturating_sub(1);
        self.indent();
        self.out.push_str(&format!("</{tag}>\n"));
    }

    /// `<tag>text</tag>` on one line.
    pub fn leaf(&mut self, tag: &str, text: &str) {
        self.indent();
        self.out.push_str(&format!("<{tag}>{}</{tag}>\n", escape(text)));
    }

    /// `<tag attrs/>`.
    pub fn empty(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        self.attrs(attrs);
        self.out.push_str("/>\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_writes() {
        let mut x = Xml::new("<!DOCTYPE t>");
        x.open("a", &[("k", "v&w")]);
        x.leaf("b", "1 < 2");
        x.empty("c", &[("n", "\"q\"")]);
        x.open("d", &[]);
        x.close("d");
        x.close("a");
        let el = parse(&x.out).unwrap();
        assert_eq!((el.name.as_str(), el.attr("k"), el.text_of("b"), el.child("c").and_then(|c| c.attr("n"))), ("a", Some("v&w"), Some("1 < 2"), Some("\"q\"")));
        assert_eq!(el.children.len(), 3);
        assert!(el.child("d").unwrap().children.is_empty());
        let nested = parse("<r><x><y>t</y></x><x/><![CDATA[raw <ok>]]></r>").unwrap();
        assert_eq!((nested.find("y").map(|y| y.text.as_str()), nested.children_named("x").count(), nested.text.as_str()), (Some("t"), 2, "raw <ok>"));
    }
}
