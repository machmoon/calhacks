//! KiCad s-expressions: a reader for library files (`.kicad_mod`,
//! `.kicad_sym`) and a writer for the files Wright emits.
//!
//! Format rules from dev-docs.kicad.org/en/file-formats/sexpr-intro: tokens
//! are lowercase bare atoms, strings are double-quoted UTF-8, numbers are mm
//! without exponents. Quoting follows KiCad `OUTPUTFORMATTER::Quotes`
//! (common/richio.cpp): escape `\\`, `"`, `\n`, `\r`. The reader follows the
//! shape of `kiutils_sexpr` (crates/kiutils_sexpr/src/parser.rs, MIT) with a
//! nesting-depth limit, but keeps no layout: Wright regenerates text.

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Sexpr {
    /// A bare token or number, as written.
    Atom(String),
    /// A double-quoted string, unescaped.
    Str(String),
    List(Vec<Sexpr>),
}

const MAX_DEPTH: usize = 256;

impl Sexpr {
    pub fn list(head: &str, rest: Vec<Sexpr>) -> Sexpr {
        let mut v = vec![Sexpr::Atom(head.into())];
        v.extend(rest);
        Sexpr::List(v)
    }
    pub fn atom(s: impl Into<String>) -> Sexpr {
        Sexpr::Atom(s.into())
    }
    pub fn str(s: impl Into<String>) -> Sexpr {
        Sexpr::Str(s.into())
    }
    /// The head token of a list (`footprint` for `(footprint ...)`).
    pub fn head(&self) -> Option<&str> {
        match self {
            Sexpr::List(v) => match v.first() {
                Some(Sexpr::Atom(a)) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }
    pub fn items(&self) -> &[Sexpr] {
        match self {
            Sexpr::List(v) => v,
            _ => &[],
        }
    }
    /// Child lists with head `name`.
    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Sexpr> + 'a {
        self.items().iter().filter(move |c| c.head() == Some(name))
    }
    pub fn child<'a>(&'a self, name: &'a str) -> Option<&'a Sexpr> {
        self.children(name).next()
    }
    /// The text of item `i` (atom or string).
    pub fn text(&self, i: usize) -> Option<&str> {
        match self.items().get(i)? {
            Sexpr::Atom(s) | Sexpr::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn num(&self, i: usize) -> Option<f64> {
        self.text(i)?.parse().ok()
    }

    pub fn parse(text: &str) -> anyhow::Result<Sexpr> {
        let b = text.as_bytes();
        let mut i = 0;
        let v = parse_at(b, &mut i, 0)?;
        skip_ws(b, &mut i);
        anyhow::ensure!(i == b.len(), "trailing text at byte {i}");
        Ok(v)
    }

    /// KiCad-style text: one list per line, tab indent, short lists of
    /// atoms kept on one line.
    pub fn to_kicad(&self) -> String {
        let mut out = String::new();
        write_at(self, 0, &mut out);
        out.push('\n');
        out
    }
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && b[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

fn parse_at(b: &[u8], i: &mut usize, depth: usize) -> anyhow::Result<Sexpr> {
    anyhow::ensure!(depth < MAX_DEPTH, "nesting deeper than {MAX_DEPTH}");
    skip_ws(b, i);
    anyhow::ensure!(*i < b.len(), "unexpected end of input");
    match b[*i] {
        b'(' => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                skip_ws(b, i);
                anyhow::ensure!(*i < b.len(), "unclosed list");
                if b[*i] == b')' {
                    *i += 1;
                    return Ok(Sexpr::List(v));
                }
                v.push(parse_at(b, i, depth + 1)?);
            }
        }
        b')' => anyhow::bail!("unexpected ) at byte {i}"),
        b'"' => {
            *i += 1;
            let mut s = Vec::new();
            while *i < b.len() && b[*i] != b'"' {
                if b[*i] == b'\\' && *i + 1 < b.len() {
                    *i += 1;
                    s.push(match b[*i] {
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        c => c,
                    });
                } else {
                    s.push(b[*i]);
                }
                *i += 1;
            }
            anyhow::ensure!(*i < b.len(), "unclosed string");
            *i += 1;
            Ok(Sexpr::Str(String::from_utf8(s)?))
        }
        _ => {
            let start = *i;
            while *i < b.len() && !b[*i].is_ascii_whitespace() && b[*i] != b'(' && b[*i] != b')' {
                *i += 1;
            }
            Ok(Sexpr::Atom(std::str::from_utf8(&b[start..*i])?.to_string()))
        }
    }
}

/// KiCad `OUTPUTFORMATTER::Quotes`.
pub fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn flat(e: &Sexpr) -> bool {
    e.items().iter().all(|c| !matches!(c, Sexpr::List(_)))
}

fn write_at(e: &Sexpr, depth: usize, out: &mut String) {
    match e {
        Sexpr::Atom(a) => out.push_str(a),
        Sexpr::Str(s) => out.push_str(&quote(s)),
        Sexpr::List(v) => {
            out.push('(');
            for (k, c) in v.iter().enumerate() {
                if let Sexpr::List(_) = c {
                    if !flat(e) {
                        out.push('\n');
                        for _ in 0..=depth {
                            out.push('\t');
                        }
                    }
                } else if k > 0 {
                    out.push(' ');
                }
                write_at(c, depth + 1, out);
            }
            if !flat(e) && v.iter().any(|c| matches!(c, Sexpr::List(_))) {
                out.push('\n');
                for _ in 0..depth {
                    out.push('\t');
                }
            }
            out.push(')');
        }
    }
}

/// Deterministic RFC 4122 v4-shaped UUID from a seed string (FNV-1a 128),
/// so the same design writes the same bytes twice (the Python baseline's `board.py _uuid`,
/// which hashed a seed for the same reason).
pub fn uuid_for(seed: &str) -> String {
    let mut h: u128 = 0x6c62272e07bb014262b821756295c58d;
    for b in seed.bytes() {
        h ^= b as u128;
        h = h.wrapping_mul(0x0000000001000000000000000000013B);
    }
    let mut bytes = h.to_be_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut s = String::with_capacity(36);
    for (i, b) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            s.push('-');
        }
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_library_footprint() {
        let p = std::path::Path::new("/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints/Resistor_SMD.pretty/R_0603_1608Metric.kicad_mod");
        let Ok(text) = std::fs::read_to_string(p) else { return }; // KiCad not installed: skip
        let e = Sexpr::parse(&text).unwrap();
        assert_eq!(e.head(), Some("footprint"));
        assert_eq!(e.children("pad").count(), 2);
        assert_eq!(Sexpr::parse(&e.to_kicad()).unwrap(), e);
    }

    #[test]
    fn strings_escape_like_kicad() {
        let e = Sexpr::list("property", vec![Sexpr::str("Value"), Sexpr::str("a \"b\"\\n")]);
        let t = e.to_kicad();
        assert_eq!(t.trim(), r#"(property "Value" "a \"b\"\\n")"#);
        assert_eq!(Sexpr::parse(&t).unwrap(), e);
    }

    #[test]
    fn uuids_are_stable_and_v4_shaped() {
        let a = uuid_for("U1/pad/1");
        assert_eq!(a, uuid_for("U1/pad/1"));
        assert_ne!(a, uuid_for("U1/pad/2"));
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
    }
}
