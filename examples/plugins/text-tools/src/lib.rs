//! text-tools: an example folio plugin with text functions (SLUG, WORDCOUNT, REGEXMATCH, with a
//! small regular expression matcher of its own) and an import filter for plain text files.
//!
//! Build it with `plugin.build` (or `cargo build --release` here) and install it with
//! `plugin.publishLocal`; see the SDK's GUIDE.md.

use folio_plugin::prelude::*;

/// SLUG(text, [separator]): "Crème brûlée, 2 pots!" becomes "creme-brulee-2-pots".
fn slug(args: &[Arg]) -> Result<String, ErrorKind> {
    let text = args[0].text()?;
    let sep = match args.get(1) {
        Some(a) => a.text()?,
        None => "-".into(),
    };
    let mut out = String::new();
    let mut pending = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        let folded = fold(c);
        if folded.is_empty() && !c.is_ascii_alphanumeric() {
            pending = !out.is_empty();
            continue;
        }
        if pending {
            out.push_str(&sep);
            pending = false;
        }
        if folded.is_empty() {
            out.push(c);
        } else {
            out.push_str(folded);
        }
    }
    Ok(out)
}

/// The ASCII letters of an accented lowercase letter ("" for anything else).
fn fold(c: char) -> &'static str {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
        'ç' | 'ć' | 'č' => "c",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => "e",
        'ì' | 'í' | 'î' | 'ï' => "i",
        'ñ' | 'ń' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => "o",
        'ù' | 'ú' | 'û' | 'ü' => "u",
        'ý' | 'ÿ' => "y",
        'š' | 'ś' => "s",
        'ž' | 'ź' | 'ż' => "z",
        'ł' => "l",
        'æ' => "ae",
        'œ' => "oe",
        'ß' => "ss",
        _ => "",
    }
}

/// WORDCOUNT(text): words (runs of non-space with a letter or digit) in a value or a range.
fn wordcount(args: &[Arg]) -> Result<usize, ErrorKind> {
    let mut n = 0;
    for v in args[0].values() {
        let t = v.text()?;
        n += t.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count();
    }
    Ok(n)
}

/// REGEXMATCH(text, pattern, [ignore_case]): whether the pattern matches somewhere in the text.
fn regexmatch(args: &[Arg]) -> Result<bool, ErrorKind> {
    let text = args[0].text()?;
    let pattern = args[1].text()?;
    let fold_case = match args.get(2) {
        Some(a) => a.boolean()?,
        None => false,
    };
    let (text, pattern) = if fold_case { (text.to_lowercase(), pattern.to_lowercase()) } else { (text, pattern) };
    let re = Regex::parse(&pattern).ok_or(ErrorKind::Value)?;
    re.is_match(&text).ok_or(ErrorKind::Value)
}

// ---- a small regular expression matcher ------------------------------------------------------
//
// Literals, `.`, `^`, `$`, `*`, `+`, `?`, `{m}`, `{m,}`, `{m,n}`, classes (`[a-z0-9_]`, `[^,]`),
// `\d \w \s \D \W \S`, escapes, and `|` between whole alternatives. No groups. Backtracking,
// with a step budget so a pathological pattern gives #VALUE! instead of hanging a sheet.

#[derive(Debug, Clone)]
enum Atom {
    Char(char),
    Any,
    Class { items: Vec<ClassItem>, negated: bool },
    Start,
    End,
}

#[derive(Debug, Clone)]
enum ClassItem {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
}

impl ClassItem {
    fn matches(&self, c: char) -> bool {
        match *self {
            ClassItem::Range(a, b) => a <= c && c <= b,
            ClassItem::Digit(yes) => c.is_ascii_digit() == yes,
            ClassItem::Word(yes) => (c.is_alphanumeric() || c == '_') == yes,
            ClassItem::Space(yes) => c.is_whitespace() == yes,
        }
    }
}

impl Atom {
    fn matches(&self, c: char) -> bool {
        match self {
            Atom::Char(x) => *x == c,
            Atom::Any => c != '\n',
            Atom::Class { items, negated } => items.iter().any(|i| i.matches(c)) != *negated,
            Atom::Start | Atom::End => false,
        }
    }
}

#[derive(Debug, Clone)]
struct Piece {
    atom: Atom,
    min: usize,
    max: usize,
}

struct Regex {
    alternatives: Vec<Vec<Piece>>,
}

const MAX_STEPS: usize = 200_000;

impl Regex {
    fn parse(p: &str) -> Option<Regex> {
        let chars: Vec<char> = p.chars().collect();
        let mut alternatives = vec![vec![]];
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            i += 1;
            let atom = match c {
                '|' => {
                    alternatives.push(vec![]);
                    continue;
                }
                '.' => Atom::Any,
                '^' => Atom::Start,
                '$' => Atom::End,
                '\\' => {
                    let e = *chars.get(i)?;
                    i += 1;
                    match escape_class(e) {
                        Some(item) => Atom::Class { items: vec![item], negated: false },
                        None => Atom::Char(match e {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        }),
                    }
                }
                '[' => {
                    let negated = chars.get(i) == Some(&'^');
                    if negated {
                        i += 1;
                    }
                    let mut items = vec![];
                    let mut first = true;
                    loop {
                        let c = *chars.get(i)?;
                        i += 1;
                        if c == ']' && !first {
                            break;
                        }
                        first = false;
                        let lo = if c == '\\' {
                            let e = *chars.get(i)?;
                            i += 1;
                            if let Some(item) = escape_class(e) {
                                items.push(item);
                                continue;
                            }
                            e
                        } else {
                            c
                        };
                        if chars.get(i) == Some(&'-') && chars.get(i + 1).is_some_and(|c| *c != ']') {
                            let hi = chars[i + 1];
                            i += 2;
                            if hi < lo {
                                return None;
                            }
                            items.push(ClassItem::Range(lo, hi));
                        } else {
                            items.push(ClassItem::Range(lo, lo));
                        }
                    }
                    Atom::Class { items, negated }
                }
                '*' | '+' | '?' | '{' | ')' | '(' => return None,
                c => Atom::Char(c),
            };
            // A quantifier after it.
            let (min, max) = match chars.get(i) {
                Some('*') => (0, usize::MAX),
                Some('+') => (1, usize::MAX),
                Some('?') => (0, 1),
                Some('{') => {
                    let close = chars[i..].iter().position(|c| *c == '}')? + i;
                    let inner: String = chars[i + 1..close].iter().collect();
                    let (a, b) = match inner.split_once(',') {
                        Some((a, "")) => (a.trim().parse().ok()?, usize::MAX),
                        Some((a, b)) => (a.trim().parse().ok()?, b.trim().parse().ok()?),
                        None => {
                            let n = inner.trim().parse().ok()?;
                            (n, n)
                        }
                    };
                    if a > b || a > 1000 {
                        return None;
                    }
                    i = close;
                    (a, b)
                }
                _ => (1, 1),
            };
            if (min, max) != (1, 1) {
                i += 1;
                if matches!(atom, Atom::Start | Atom::End) {
                    return None;
                }
            }
            alternatives.last_mut()?.push(Piece { atom, min, max });
        }
        Some(Regex { alternatives })
    }

    /// Whether it matches anywhere; `None` when the step budget runs out.
    fn is_match(&self, text: &str) -> Option<bool> {
        let t: Vec<char> = text.chars().collect();
        let mut steps = 0;
        for alt in &self.alternatives {
            for start in 0..=t.len() {
                if match_here(alt, &t, start, &mut steps)? {
                    return Some(true);
                }
            }
        }
        Some(false)
    }
}

fn escape_class(e: char) -> Option<ClassItem> {
    Some(match e {
        'd' => ClassItem::Digit(true),
        'D' => ClassItem::Digit(false),
        'w' => ClassItem::Word(true),
        'W' => ClassItem::Word(false),
        's' => ClassItem::Space(true),
        'S' => ClassItem::Space(false),
        _ => return None,
    })
}

fn match_here(pieces: &[Piece], t: &[char], at: usize, steps: &mut usize) -> Option<bool> {
    *steps += 1;
    if *steps > MAX_STEPS {
        return None;
    }
    let Some((p, rest)) = pieces.split_first() else { return Some(true) };
    match p.atom {
        Atom::Start => return if at == 0 { match_here(rest, t, at, steps) } else { Some(false) },
        Atom::End => return if at == t.len() { match_here(rest, t, at, steps) } else { Some(false) },
        _ => {}
    }
    // Greedy: take as many as allowed, then give back.
    let mut n = 0;
    while n < p.max && at + n < t.len() && p.atom.matches(t[at + n]) {
        n += 1;
    }
    if n < p.min {
        return Some(false);
    }
    loop {
        if match_here(rest, t, at + n, steps)? {
            return Some(true);
        }
        if n == p.min {
            return Some(false);
        }
        n -= 1;
    }
}

// ---- an import filter -------------------------------------------------------------------------

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => {}
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Plain text: paragraphs between blank lines; `# ` and `## ` lines become headings.
fn import_txt(bytes: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "This file isn't UTF-8 text.".to_string())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n");
    let mut title = None;
    let mut blocks = vec![];
    for para in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        let (style, body) = if let Some(h) = para.strip_prefix("## ") {
            ("heading2", h)
        } else if let Some(h) = para.strip_prefix("# ") {
            title.get_or_insert_with(|| h.trim().to_string());
            ("heading1", h)
        } else {
            ("normal", para)
        };
        blocks.push(format!(r#"{{"type":"paragraph","style":"{style}","runs":[{{"text":{}}}]}}"#, json_str(body.trim())));
    }
    if blocks.is_empty() {
        blocks.push(r#"{"type":"paragraph","runs":[]}"#.to_string());
    }
    let title = title.unwrap_or_else(|| "Text".into());
    Ok(format!(r#"{{"title":{},"pages":[{{"name":{},"kind":"doc","blocks":[{}]}}]}}"#, json_str(&title), json_str(&title), blocks.join(",")))
}

export! {
    id: "xyz.lsuite.folio.text-tools",
    name: "Text tools",
    version: env!("CARGO_PKG_VERSION"),
    functions: [
        fn_def!("SLUG", "SLUG(text, [separator])", "Text as a URL slug: lowercase ASCII words joined by dashes.", 1, 2, slug),
        fn_def!("WORDCOUNT", "WORDCOUNT(text)", "How many words a text, or a range of texts, holds.", 1, 1, wordcount),
        fn_def!("REGEXMATCH", "REGEXMATCH(text, pattern, [ignore_case])", "Whether a regular expression matches the text.", 2, 3, regexmatch),
    ],
    filters: [
        filter_def!("Plain text", "txt,text", "Paragraphs from a plain text file; # and ## lines become headings.", import_txt),
    ],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Arg {
        Arg::Value(Value::Text(s.into()))
    }

    fn re(text: &str, p: &str) -> bool {
        regexmatch(&[t(text), t(p)]).unwrap()
    }

    #[test]
    fn slugs_and_words() {
        assert_eq!(slug(&[t("  Crème brûlée, 2 pots! ")]).unwrap(), "creme-brulee-2-pots");
        assert_eq!(slug(&[t("A B"), t("_")]).unwrap(), "a_b");
        assert_eq!(wordcount(&[t("one two  three — four")]).unwrap(), 4);
    }

    #[test]
    fn the_matcher() {
        assert!(re("invoice-2026-10", r"^invoice-\d{4}-\d\d$"));
        assert!(!re("invoice-26-10", r"^invoice-\d{4}-\d\d$"));
        assert!(re("hello world", "wor"));
        assert!(re("colour", "colou?r"));
        assert!(re("a@b.co", r"^[\w.]+@[\w]+\.[a-z]{2,}$"));
        assert!(re("cat", "dog|cat"));
        assert!(!re("abc", "[^abc]"));
        assert!(regexmatch(&[t("ABC"), t("abc"), Arg::Value(Value::Bool(true))]).unwrap());
        assert_eq!(regexmatch(&[t("x"), t("(")]), Err(ErrorKind::Value));
        assert_eq!(regexmatch(&[t(&"a".repeat(40)), t("a*a*a*a*a*a*a*b")]), Err(ErrorKind::Value));
    }

    #[test]
    fn text_files_become_documents() {
        let json = import_txt(b"# Notes\n\nFirst \"para\".\n\n## Part\n\nSecond").unwrap();
        assert!(json.starts_with(r#"{"title":"Notes","#), "{json}");
        assert!(json.contains(r#""style":"heading2","runs":[{"text":"Part"}]"#));
        assert!(json.contains(r#"First \"para\"."#));
    }
}
