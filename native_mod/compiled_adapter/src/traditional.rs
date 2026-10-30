//! Saved Chinese-script preference. Dictionary data is OpenCC (Apache-2.0);
//! conversion is compiled, local, and never interprets game substitutions.
use crate::{
    gui, planning,
    script::{self, Result, Value},
    sources::Sources,
    view::Localisation,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};
pub const FLAG: &str = "tte105_traditional_chinese";
pub fn caption_keys<'a>(raws: impl Iterator<Item = &'a str>) -> Result<Vec<String>> {
    fn walk(v: &Value, parent: &str, out: &mut Vec<String>) {
        for e in v.entries() {
            if matches!(
                e.key.as_deref(),
                Some("title" | "desc" | "custom_tooltip" | "text")
            ) || (parent == "option" && e.key.as_deref() == Some("name"))
            {
                if let Some(s) = e.value.atom() {
                    out.extend(refs(s));
                    if crate::view::safe_localisation_key(s) {
                        out.push(s.into());
                    }
                }
            }
            walk(&e.value, e.key.as_deref().unwrap_or(""), out);
        }
    }
    let mut keys = Vec::new();
    for raw in raws {
        walk(&script::parse(raw)?, "", &mut keys);
    }
    Ok(keys)
}

fn refs(raw: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut rest = raw;
    while let Some(a) = rest.find('$') {
        let tail = &rest[a + 1..];
        let Some(b) = tail.find('$') else { break };
        let key = tail[..b].split('|').next().unwrap();
        if crate::view::safe_localisation_key(key) {
            result.push(key.into());
        }
        rest = &tail[b + 1..];
    }
    result
}
fn rewrite_refs(raw: &str, names: &BTreeMap<String, String>) -> String {
    let mut result = String::new();
    let mut rest = raw;
    while let Some(a) = rest.find('$') {
        result.push_str(&rest[..a]);
        let tail = &rest[a + 1..];
        let Some(b) = tail.find('$') else {
            result.push_str(&rest[a..]);
            return result;
        };
        let (key, format) = tail[..b]
            .split_once('|')
            .map(|(k, f)| (k, format!("|{f}")))
            .unwrap_or((&tail[..b], String::new()));
        result.push('$');
        result.push_str(names.get(key).map(String::as_str).unwrap_or(key));
        result.push_str(&format);
        result.push('$');
        rest = &tail[b + 1..];
    }
    result.push_str(rest);
    result
}
pub struct Preference {
    pub names: BTreeMap<String, String>,
    selectors: BTreeMap<String, String>,
}
impl Preference {
    pub fn prepare(
        rows: &mut Localisation,
        sources: &Sources,
        getters: &mut Vec<String>,
        extra_keys: Vec<String>,
    ) -> Result<Self> {
        let originals = rows.clone();
        let native: BTreeMap<_, _> = crate::languages::LANGUAGES
            .into_iter()
            .map(|l| (l, sources.localisation(l).0))
            .collect();
        let mut needed: BTreeSet<String> =
            originals.values().flat_map(|v| v.keys().cloned()).collect();
        let mut pending: Vec<_> = originals
            .values()
            .flat_map(|v| v.values().flat_map(|s| refs(s)))
            .collect();
        pending.extend(extra_keys);
        for raw in getters.iter() {
            fn walk(v: &Value, pending: &mut Vec<String>) {
                for e in v.entries() {
                    if matches!(e.key.as_deref(), Some("localization_key" | "default")) {
                        if let Some(k) = e.value.atom() {
                            pending.push(k.into());
                        }
                    }
                    walk(&e.value, pending);
                }
            }
            walk(&script::parse(raw)?, &mut pending);
        }
        let mut seen = BTreeSet::new();
        while let Some(key) = pending.pop() {
            if !seen.insert(key.clone()) || !crate::view::safe_localisation_key(&key) {
                continue;
            }
            if seen.len() > 100000 {
                return Err("Traditional locale reference budget exceeded".into());
            }
            let mut found = false;
            for language in crate::languages::LANGUAGES {
                if let Some(value) = originals[language]
                    .get(&key)
                    .or_else(|| native[language].get(&key))
                {
                    pending.extend(refs(value));
                    found = true;
                }
            }
            if found {
                needed.insert(key);
            }
        }
        // Only changed Chinese strings, or references reaching one, need a
        // preference alias. This avoids duplicating every getter and locale.
        let chinese = |key: &str| {
            originals
                .get("simp_chinese")
                .and_then(|m| m.get(key))
                .or_else(|| native["simp_chinese"].get(key))
                .or_else(|| originals.get("english").and_then(|m| m.get(key)))
                .or_else(|| native["english"].get(key))
        };
        let mut changed: BTreeSet<String> = needed
            .iter()
            .filter(|k| chinese(k).is_some_and(|s| convert(s) != *s))
            .cloned()
            .collect();
        loop {
            let more: Vec<_> = needed
                .iter()
                .filter(|k| {
                    !changed.contains(*k)
                        && chinese(k).is_some_and(|s| refs(s).iter().any(|r| changed.contains(r)))
                })
                .cloned()
                .collect();
            if more.is_empty() {
                break;
            }
            changed.extend(more);
        }
        let names: BTreeMap<_, _> = changed
            .into_iter()
            .map(|k| {
                let n = format!("tte_hant_{k}");
                (k, n)
            })
            .collect();
        for language in crate::languages::LANGUAGES {
            let texts = rows.get_mut(language).ok_or("Missing native UI locale")?;
            for (key, alias) in &names {
                let value = originals
                    .get(language)
                    .and_then(|m| m.get(key))
                    .or_else(|| native[language].get(key))
                    .or_else(|| originals["english"].get(key))
                    .or_else(|| native["english"].get(key));
                let value = if language == "simp_chinese" {
                    rewrite_refs(&convert(value.map(String::as_str).unwrap_or(key)), &names)
                } else {
                    // Preserve native fallback behaviour, including keys only
                    // supplied by a non-English source. No copied foreign text.
                    format!("${key}$")
                };
                if texts.insert(alias.clone(), value).is_some() {
                    return Err(format!("Traditional locale collision: {alias}"));
                }
            }
        }
        // Existing getters keep their predicates/order. A preference-specific
        // branch precedes each state and a preference-specific default is last.
        for raw in getters.iter_mut() {
            let ast = script::parse(raw)?;
            let mut changes = Vec::new();
            for root in ast
                .entries()
                .iter()
                .filter(|e| e.key.as_deref() == Some("defined_text"))
            {
                for e in root.value.entries() {
                    if e.key.as_deref() == Some("text") {
                        if let Some(alias) = e
                            .value
                            .scalar("localization_key")
                            .and_then(|k| names.get(k))
                        {
                            let trigger = e
                                .value
                                .first("trigger")
                                .map(planning::render)
                                .unwrap_or_default();
                            changes.push((e.span.start,e.span.start,format!("text={{trigger={{has_country_flag={FLAG} {trigger}}} localization_key={alias}}}\n")));
                        }
                    } else if e.key.as_deref() == Some("default") {
                        if let Some(alias) = e.value.atom().and_then(|k| names.get(k)) {
                            changes.push((e.span.start,e.span.start,format!("text={{trigger={{has_country_flag={FLAG}}} localization_key={alias}}}\n")));
                        }
                    }
                }
            }
            *raw = gui::edits(raw, changes)?;
        }
        Ok(Self {
            names,
            selectors: BTreeMap::new(),
        })
    }
    fn selector(&mut self, key: &str, getters: &mut Vec<String>) -> Option<String> {
        let alias = self.names.get(key)?;
        if let Some(name) = self.selectors.get(key) {
            return Some(name.clone());
        }
        let hash = crate::sources::sha(key.as_bytes());
        let name = format!("GetTTEText_{}", &hash[..16]);
        getters.push(format!("defined_text={{name={name} random=no text={{trigger={{has_country_flag={FLAG}}} localization_key={alias}}} default={key}}}"));
        self.selectors.insert(key.into(), name.clone());
        Some(name)
    }
    pub fn gui(
        &mut self,
        raw: &str,
        getters: &mut Vec<String>,
        buttons: &mut Vec<String>,
        owned_subtrees_only: bool,
    ) -> Result<String> {
        fn walk(
            pref: &mut Preference,
            v: &Value,
            raw: &str,
            getters: &mut Vec<String>,
            buttons: &mut Vec<String>,
            changes: &mut Vec<(usize, usize, String)>,
            owned_scope: bool,
        ) -> Result<()> {
            for e in v.entries() {
                let owned = owned_scope
                    || e.value
                        .scalar("name")
                        .is_some_and(|n| n.starts_with("tte_"));
                if owned && e.key.as_deref() == Some("instantTextBoxType") {
                    let label = e.value.scalar("text").and_then(|s| {
                        if s.starts_with("tte_") {
                            Some(s)
                        } else if e
                            .value
                            .scalar("name")
                            .is_some_and(|n| n.starts_with("tte_category_"))
                        {
                            s.strip_prefix('$').and_then(|s| s.strip_suffix('$'))
                        } else {
                            None
                        }
                    });
                    if let Some(key) = label {
                        if let Some(selector) = pref.selector(key, getters) {
                            let name =
                                e.value.scalar("name").ok_or("Owned caption missing name")?;
                            let effect = format!(
                                "tte_label_{}",
                                &crate::sources::sha(name.as_bytes())[..16]
                            );
                            // Repeated caption instances intentionally share a
                            // single no-op effect, independent of widget scope.
                            let definition=format!("{effect}={{potential={{from={{is_ai=no}}}} allow={{always=yes}} effect={{}}}}");
                            if !buttons.contains(&definition) {
                                buttons.push(definition);
                            }
                            let props = [
                                "position",
                                "orientation",
                                "origo",
                                "format",
                                "color",
                                "fixedsize",
                            ]
                            .into_iter()
                            .filter_map(|k| {
                                e.value.first(k).map(|v| {
                                    format!(
                                        "{k}={}",
                                        if matches!(v, Value::Block(_)) {
                                            format!("{{{}}}", planning::render(v))
                                        } else {
                                            planning::render(v)
                                        }
                                    )
                                })
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                            let width = e.value.scalar("maxWidth").unwrap_or("300");
                            let height = e.value.scalar("maxHeight").unwrap_or("24");
                            let font = e.value.scalar("font").unwrap_or("cg_16b");
                            changes.push((e.span.start,e.span.end,format!("effectbuttonType={{name={name} {props} size={{x={width} y={height}}} font={} quadTextureSprite=GFX_tte_caption_blank alwaysTransparent=yes borderSize={{x=0 y=0}} multiline={} buttonText=\"[From.{selector}]\" effect={effect}}}",planning::atom(font),if height.parse::<i64>().unwrap_or(24)>32{"yes"}else{"no"})));
                            continue;
                        }
                    }
                }
                if owned && e.key.as_deref() == Some("buttonText") {
                    if let Some(key) = e.value.atom().filter(|s| s.starts_with("tte_")) {
                        if let Some(selector) = pref.selector(key, getters) {
                            changes.push((
                                e.span.start,
                                e.span.end,
                                format!("buttonText=\"[From.{selector}]\""),
                            ));
                        }
                    }
                }
                walk(pref, &e.value, raw, getters, buttons, changes, owned)?;
            }
            let _ = raw;
            Ok(())
        }
        let mut changes = Vec::new();
        walk(
            self,
            &script::parse(raw)?,
            raw,
            getters,
            buttons,
            &mut changes,
            !owned_subtrees_only,
        )?;
        gui::edits(raw, changes)
    }
    pub fn event_strings(
        &mut self,
        raw: &str,
        rows: &mut Localisation,
        getters: &mut Vec<String>,
    ) -> Result<String> {
        fn walk(
            pref: &mut Preference,
            v: &Value,
            parent: &str,
            rows: &mut Localisation,
            getters: &mut Vec<String>,
            changes: &mut Vec<(usize, usize, String)>,
        ) -> Result<()> {
            for e in v.entries() {
                let field = e.key.as_deref().unwrap_or("");
                if ["title", "desc", "custom_tooltip"].contains(&field)
                    || (parent == "option" && field == "name")
                {
                    if let Some(key) = e.value.atom() {
                        if let Some(selector) = pref.selector(key, getters) {
                            let wrapper = format!(
                                "tte_switch_{}",
                                &crate::sources::sha(key.as_bytes())[..16]
                            );
                            for texts in rows.values_mut() {
                                texts.insert(wrapper.clone(), format!("[Root.{selector}]"));
                            }
                            changes.push((e.span.start, e.span.end, format!("{field}={wrapper}")));
                        }
                    }
                }
                walk(pref, &e.value, field, rows, getters, changes)?;
            }
            Ok(())
        }
        let mut changes = Vec::new();
        walk(self, &script::parse(raw)?, "", rows, getters, &mut changes)?;
        gui::edits(raw, changes)
    }
}

pub fn settings(rows: &mut Localisation, events: &mut [String]) -> Result<()> {
    for (key, en, zh) in [
        ("tte_language_simplified", "Simplified Chinese", "简体中文"),
        (
            "tte_language_traditional",
            "Traditional Chinese",
            "繁體中文",
        ),
    ] {
        for language in crate::languages::LANGUAGES {
            rows.get_mut(language).unwrap().insert(
                key.into(),
                if language == "simp_chinese" {
                    zh.into()
                } else {
                    en.into()
                },
            );
        }
    }
    let mut added = 0;
    for raw in events {
        let ast = script::parse(raw)?;
        if !ast
            .entries()
            .iter()
            .any(|e| e.value.scalar("id") == Some("tte.6"))
        {
            continue;
        }
        // The Settings page is owned generation, not a source-mod definition.
        let at = raw.rfind('}').unwrap();
        raw.insert_str(
            at,
            &format!(
                "\n{}\n{}",
                crate::view::option(
                    "tte_language_simplified",
                    &format!("remove_country_flag={FLAG} country_event={{id=tte.6}}"),
                    ""
                ),
                crate::view::option(
                    "tte_language_traditional",
                    &format!("set_country_flag={FLAG} country_event={{id=tte.6}}"),
                    ""
                )
            ),
        );
        added += 1;
    }
    if added != 1 {
        return Err("Expected one owned Settings event".into());
    }
    Ok(())
}

#[derive(Default)]
struct Node {
    next: BTreeMap<char, usize>,
    value: Option<String>,
}
fn dictionary() -> &'static Vec<Node> {
    static NODES: OnceLock<Vec<Node>> = OnceLock::new();
    NODES.get_or_init(|| {
        let mut nodes = vec![Node::default()];
        for raw in [
            include_str!("../assets/opencc/STCharacters.txt"),
            include_str!("../assets/opencc/STPhrases.txt"),
        ] {
            for line in raw.lines().filter(|s| !s.starts_with('#')) {
                let Some((key, values)) = line.split_once('\t') else {
                    continue;
                };
                let Some(value) = values.split_whitespace().next() else {
                    continue;
                };
                if key.is_empty() {
                    continue;
                }
                let mut at = 0;
                for c in key.chars() {
                    at = if let Some(&next) = nodes[at].next.get(&c) {
                        next
                    } else {
                        let next = nodes.len();
                        nodes.push(Node::default());
                        nodes[at].next.insert(c, next);
                        next
                    };
                }
                nodes[at].value = Some(value.into());
            }
        }
        nodes
    })
}
fn prose(raw: &str, out: &mut String) {
    let nodes = dictionary();
    let mut at = 0;
    while at < raw.len() {
        let mut node = 0;
        let mut found = None;
        for (offset, c) in raw[at..].char_indices() {
            let Some(&next) = nodes[node].next.get(&c) else {
                break;
            };
            node = next;
            if let Some(value) = &nodes[node].value {
                found = Some((at + offset + c.len_utf8(), value));
            }
        }
        if let Some((end, value)) = found {
            out.push_str(value);
            at = end;
        } else {
            let c = raw[at..].chars().next().unwrap();
            out.push(c);
            at += c.len_utf8();
        }
    }
}
pub fn convert(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut at = 0;
    let mut start = 0;
    while at < raw.len() {
        let rest = &raw[at..];
        let c = rest.chars().next().unwrap();
        let end = match c {
            '$' | '£' => rest[c.len_utf8()..]
                .find(c)
                .map(|n| at + c.len_utf8() + n + c.len_utf8()),
            '[' => rest.find(']').map(|n| at + n + 1),
            '§' => rest
                .chars()
                .nth(1)
                .map(|c2| at + c.len_utf8() + c2.len_utf8()),
            '\\' if rest.starts_with("\\n") => Some(at + 2),
            _ => None,
        };
        if let Some(end) = end {
            prose(&raw[start..at], &mut out);
            out.push_str(&raw[at..end]);
            at = end;
            start = end;
        } else {
            at += c.len_utf8();
        }
    }
    prose(&raw[start..], &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phrases_resolve_ambiguous_characters_without_altering_engine_tokens() {
        assert_eq!(
            convert("头发发展，后台研究。§G科技§! $后续科技$ [Root.研发] £能量£\\n"),
            "頭髮發展，後臺研究。§G科技§! $后续科技$ [Root.研发] £能量£\\n"
        );
        assert_eq!(convert("Continuous research"), "Continuous research");
        assert_eq!(convert(""), "");
    }
}
