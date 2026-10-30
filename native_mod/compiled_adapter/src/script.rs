use std::ops::Range;

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Atom(String),
    Block(Vec<Entry>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: Option<String>,
    pub op: String,
    pub value: Value,
    pub span: Range<usize>,
}

impl Value {
    pub fn atom(&self) -> Option<&str> {
        match self {
            Self::Atom(s) => Some(s),
            _ => None,
        }
    }
    pub fn entries(&self) -> &[Entry] {
        match self {
            Self::Block(v) => v,
            _ => &[],
        }
    }
    pub fn first(&self, key: &str) -> Option<&Value> {
        self.entries()
            .iter()
            .find(|e| e.key.as_deref() == Some(key))
            .map(|e| &e.value)
    }
    pub fn scalar(&self, key: &str) -> Option<&str> {
        self.first(key).and_then(Value::atom)
    }
    pub fn all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Value> {
        self.entries()
            .iter()
            .filter(move |e| e.key.as_deref() == Some(key))
            .map(|e| &e.value)
    }
}

#[derive(Clone, Debug)]
struct Token {
    text: String,
    start: usize,
    end: usize,
    quoted: bool,
}

pub fn normalise(raw: &str) -> String {
    raw.strip_prefix('\u{feff}')
        .unwrap_or(raw)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

fn tokens(raw: &str, valve: bool) -> Result<Vec<Token>> {
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if bytes[i] == b'#' || (valve && bytes[i..].starts_with(b"//")) {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == 0 {
            return Err(format!("NUL in script at byte {i}"));
        }
        let start = i;
        if bytes[i] == b'"' {
            i += 1;
            let mut text = String::new();
            let mut segment = i;
            loop {
                if i == bytes.len() {
                    return Err(format!("Unclosed quoted string at byte {start}"));
                }
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    if bytes[i + 1] == b'"' || bytes[i + 1] == b'\\' {
                        text.push_str(&raw[segment..i]);
                        text.push(bytes[i + 1] as char);
                        i += 2;
                        segment = i;
                        continue;
                    }
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    text.push_str(&raw[segment..i]);
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push(Token {
                text,
                start,
                end: i,
                quoted: true,
            });
            continue;
        }
        if b"{}=<>!?".contains(&bytes[i]) {
            i += 1;
            if i < bytes.len() && bytes[i] == b'=' && !b"{}".contains(&bytes[start]) {
                i += 1;
            }
        } else {
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && !b"{}=<>!?#\"".contains(&bytes[i])
            {
                i += 1;
            }
        }
        if start == i {
            return Err(format!("Unexpected byte at {i}"));
        }
        out.push(Token {
            text: raw[start..i].into(),
            start,
            end: i,
            quoted: false,
        });
    }
    Ok(out)
}

pub fn parse(raw: &str) -> Result<Value> {
    let ts = tokens(raw, false)?;
    let mut i = 0;
    fn block(ts: &[Token], i: &mut usize, nested: bool, depth: usize) -> Result<Vec<Entry>> {
        if depth > 128 {
            return Err("Script nesting exceeds 128".into());
        }
        let mut entries = Vec::new();
        while *i < ts.len() {
            if !ts[*i].quoted && ts[*i].text == "}" {
                if !nested {
                    return Err(format!("Unexpected closing brace at byte {}", ts[*i].start));
                }
                *i += 1;
                return Ok(entries);
            }
            let first = ts[*i].clone();
            *i += 1;
            if !first.quoted && first.text == "{" {
                let value = Value::Block(block(ts, i, true, depth + 1)?);
                entries.push(Entry {
                    key: None,
                    op: String::new(),
                    span: first.start..ts[*i - 1].end,
                    value,
                });
                continue;
            }
            let is_field = *i < ts.len()
                && !ts[*i].quoted
                && ["=", "<", ">", "<=", ">=", "!=", "?="].contains(&ts[*i].text.as_str());
            if !is_field {
                entries.push(Entry {
                    key: None,
                    op: String::new(),
                    span: first.start..first.end,
                    value: Value::Atom(first.text),
                });
                continue;
            }
            let op = ts[*i].text.clone();
            *i += 1;
            if *i == ts.len() {
                return Err(format!("Missing value for {}", first.text));
            }
            let value = if !ts[*i].quoted && ts[*i].text == "{" {
                *i += 1;
                Value::Block(block(ts, i, true, depth + 1)?)
            } else {
                if !ts[*i].quoted && ts[*i].text == "}" {
                    return Err(format!("Missing value for {}", first.text));
                }
                let v = Value::Atom(ts[*i].text.clone());
                *i += 1;
                v
            };
            entries.push(Entry {
                key: Some(first.text),
                op,
                value,
                span: first.start..ts[*i - 1].end,
            });
        }
        if nested {
            Err("Unclosed script block".into())
        } else {
            Ok(entries)
        }
    }
    Ok(Value::Block(block(&ts, &mut i, false, 0)?))
}

/// Valve KeyValues pairs use no '='; keep them separate from game scripts.
pub fn parse_vdf(raw: &str) -> Result<Value> {
    if raw.len() > 1024 * 1024 {
        return Err("VDF size budget exceeded".into());
    }
    let ts = tokens(raw.trim_start_matches('\u{feff}'), true)?;
    if ts.len() > 20000 {
        return Err("VDF token budget exceeded".into());
    }
    fn block(ts: &[Token], i: &mut usize, depth: usize) -> Result<Vec<Entry>> {
        if depth > 16 {
            return Err("VDF nesting budget exceeded".into());
        }
        let mut out = Vec::new();
        while *i < ts.len() {
            if !ts[*i].quoted && ts[*i].text == "}" {
                if depth == 0 {
                    return Err("Unexpected VDF closing brace".into());
                }
                *i += 1;
                return Ok(out);
            }
            let key = ts[*i].clone();
            *i += 1;
            if !key.quoted && ["{", "=", "<", ">"].contains(&key.text.as_str()) {
                return Err("Invalid VDF key".into());
            }
            let next = ts.get(*i).ok_or("VDF value missing")?;
            *i += 1;
            let value = if !next.quoted && next.text == "{" {
                Value::Block(block(ts, i, depth + 1)?)
            } else {
                if !next.quoted && ["}", "=", "<", ">"].contains(&next.text.as_str()) {
                    return Err("Invalid VDF value".into());
                }
                Value::Atom(next.text.clone())
            };
            out.push(Entry {
                key: Some(key.text),
                op: "=".into(),
                value,
                span: key.start..ts[*i - 1].end,
            });
        }
        if depth > 0 {
            Err("Unclosed VDF block".into())
        } else {
            Ok(out)
        }
    }
    Ok(Value::Block(block(&ts, &mut 0, 0)?))
}

pub fn data(value: &Value) -> serde_json::Value {
    use serde_json::json;
    match value {
        Value::Atom(s) => json!(s),
        Value::Block(entries) => json!({"items":entries.iter().map(|e| match &e.key {
            Some(k)=>json!({"key":k,"operator":e.op,"value":data(&e.value)}),None=>data(&e.value)
        }).collect::<Vec<_>>()}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoted_duplicate_and_comparison() {
        let x = parse(
            "# comment\na={name=\"中文 # { \\\"x\\\"\" modifier={factor=1 factor=2 value>=3}}\n",
        )
        .unwrap();
        let a = x.first("a").unwrap();
        assert_eq!(a.scalar("name"), Some("中文 # { \"x\""));
        let m = a.first("modifier").unwrap();
        assert_eq!(m.all("factor").count(), 2);
        assert_eq!(m.entries()[2].op, ">=");
    }
    #[test]
    fn spans_are_utf8_byte_slices() {
        let raw = "name=\"中文\" tech_x={area=physics}";
        let x = parse(raw).unwrap();
        assert_eq!(&raw[x.entries()[1].span.clone()], "tech_x={area=physics}");
    }
    #[test]
    fn malformed_is_error() {
        for raw in ["a={", "a=", "a=}", "}", "a=\"unterminated", "a={b={}"] {
            assert!(parse(raw).is_err(), "{raw}");
        }
    }
    #[test]
    fn no_evaluation_of_source_text() {
        assert_eq!(
            parse("effect=\"$(touch /tmp/bad)\"")
                .unwrap()
                .scalar("effect"),
            Some("$(touch /tmp/bad)")
        );
    }
    #[test]
    fn anonymous_gui_list_blocks_keep_spans_and_structure() {
        let s = "guiTypes={own={points={{x=1 y=2} {x=3 y=4}}}}";
        let p = parse(s).unwrap();
        let points = p
            .first("guiTypes")
            .unwrap()
            .first("own")
            .unwrap()
            .first("points")
            .unwrap();
        assert_eq!(points.entries().len(), 2);
        assert_eq!(&s[points.entries()[0].span.clone()], "{x=1 y=2}");
        assert_eq!(points.entries()[1].value.scalar("x"), Some("3"));
    }
}
