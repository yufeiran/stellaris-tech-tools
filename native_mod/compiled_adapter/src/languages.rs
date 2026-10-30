//! Translate owned UI only, keeping engine references intact. Technology
//! captions are rebuilt from each source locale, never machine-translated.
use crate::{
    script::Result,
    sources::Sources,
    view::{self, Localisation},
};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};
pub const LANGUAGES: [&str; 10] = [
    "english",
    "simp_chinese",
    "japanese",
    "korean",
    "russian",
    "french",
    "german",
    "spanish",
    "polish",
    "braz_por",
];
fn boundary(s: &str, at: usize) -> bool {
    s.get(at..)
        .and_then(|s| s.chars().next())
        .is_none_or(|c| !c.is_ascii_alphabetic())
}
fn fragment(text: &str, col: usize, phrases: &BTreeMap<String, Vec<String>>) -> Result<String> {
    let prefix = "The goal route needs a higher tier. Research one T";
    if let Some(rest) = text.strip_prefix(prefix) {
        if let Some((tier, rest)) = rest.split_once(" technology toward unlocking T") {
            if let Some(next) = rest.strip_suffix('.') {
                if tier.chars().all(|c| c.is_ascii_digit())
                    && next.chars().all(|c| c.is_ascii_digit())
                {
                    return Ok(phrases["The goal route needs a higher tier. Research one T{tier} technology toward unlocking T{next}."][col].replace("{tier}",tier).replace("{next}",next));
                }
            }
        }
    }
    let mut keys: Vec<_> = phrases.keys().collect();
    keys.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut out = String::new();
    let mut unknown = String::new();
    let mut at = 0;
    fn check(s: &str) -> Result<()> {
        let mut run = 0;
        for c in s.chars() {
            if c.is_ascii_alphabetic() {
                run += 1;
                if run >= 2 {
                    return Err(format!("Untranslated owned UI fragment: {s}"));
                }
            } else {
                run = 0
            }
        }
        Ok(())
    }
    while at < text.len() {
        let before = at == 0
            || text[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphabetic());
        let found = keys
            .iter()
            .find(|k| before && text[at..].starts_with(k.as_str()) && boundary(text, at + k.len()));
        if let Some(k) = found {
            check(&unknown)?;
            unknown.clear();
            out.push_str(&phrases[*k][col]);
            at += k.len();
        } else {
            let c = text[at..].chars().next().unwrap();
            unknown.push(c);
            out.push(c);
            at += c.len_utf8();
        }
    }
    check(&unknown)?;
    Ok(out)
}
fn cached_fragment(
    text: &str,
    col: usize,
    phrases: &BTreeMap<String, Vec<String>>,
    cache: &mut BTreeMap<String, String>,
) -> Result<String> {
    if let Some(value) = cache.get(text) {
        return Ok(value.clone());
    }
    let value = fragment(text, col, phrases)?;
    cache.insert(text.into(), value.clone());
    Ok(value)
}
fn translate(
    text: &str,
    col: usize,
    phrases: &BTreeMap<String, Vec<String>>,
    cache: &mut BTreeMap<String, String>,
) -> Result<String> {
    // The only templates containing engine references across multiple prose
    // fragments are filled without translating any reference contents.
    if text.starts_with("Fill T[") {
        if let Some((tier, rest)) = text[6..].split_once(" toward T") {
            if let Some((next, rest)) = rest.split_once("; counted ") {
                if let Some(rest) = rest.strip_suffix('.') {
                    if let Some((count, limit)) = rest.split_once('/') {
                        let key = "Fill T{tier} toward T{next}; counted {count}/{limit}.";
                        return Ok(phrases[key][col]
                            .replace("{tier}", tier)
                            .replace("{next}", next)
                            .replace("{count}", count)
                            .replace("{limit}", limit));
                    }
                }
            }
        }
    }
    let mut out = String::new();
    let mut at = 0;
    let mut start = 0;
    while at < text.len() {
        let rest = &text[at..];
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
            out.push_str(&cached_fragment(&text[start..at], col, phrases, cache)?);
            out.push_str(&text[at..end]);
            at = end;
            start = end;
        } else {
            at += c.len_utf8();
        }
    }
    out.push_str(&cached_fragment(&text[start..], col, phrases, cache)?);
    Ok(out)
}
pub fn expand(rows: &mut Localisation, report: &Json, sources: &Sources) -> Result<()> {
    let asset: Json = serde_json::from_str(include_str!("../assets/ui-phrases.json"))
        .map_err(|e| e.to_string())?;
    if asset["languages"] != serde_json::json!(&LANGUAGES[2..]) {
        return Err("Owned language phrase columns do not match runtime language order".into());
    }
    let phrases: BTreeMap<String, Vec<String>> =
        serde_json::from_value(asset["phrases"].clone()).map_err(|e| e.to_string())?;
    if phrases
        .values()
        .any(|v| v.len() != 8 || v.iter().any(String::is_empty))
    {
        return Err("Invalid owned language phrase columns".into());
    }
    let english = rows["english"].clone();
    let (base, _) = sources.localisation("english");
    let techs = crate::planning::technologies(report)?;
    let categories: BTreeSet<String> = techs
        .iter()
        .filter_map(|t| t.block.first("category"))
        .flat_map(|v| {
            v.entries()
                .iter()
                .filter_map(|e| e.value.atom())
                .filter(|key| view::safe_localisation_key(key))
                .map(str::to_owned)
        })
        .collect();
    for (col, language) in LANGUAGES[2..].iter().enumerate() {
        // Explicit project-owned translations (e.g. the shared goal editor)
        // take precedence over phrase reconstruction, never over source keys.
        let mut texts = rows.get(*language).cloned().unwrap_or_default();
        let mut cache = BTreeMap::new();
        for (key, value) in &english {
            if texts.contains_key(key) {
                continue;
            }
            if key.starts_with("tte_caption_") && !key.starts_with("tte_caption_full_") {
                continue;
            }
            if key.starts_with("tte_source_") {
                texts.insert(key.clone(), value.clone());
                continue;
            }
            texts.insert(
                key.clone(),
                translate(value, col, &phrases, &mut cache)
                    .map_err(|e| format!("{language}/{key}: {e}"))?,
            );
        }
        rows.insert((*language).into(), texts);
    }
    for language in LANGUAGES {
        let (native, _) = sources.localisation(language);
        let texts = rows.get_mut(language).unwrap();
        for key in &categories {
            if !native.contains_key(key) {
                texts.insert(
                    key.clone(),
                    base.get(key)
                        .cloned()
                        .unwrap_or_else(|| key.replace('_', " ")),
                );
            }
        }
        for id in report["technologies"].as_object().unwrap().keys() {
            let caption = native
                .get(id)
                .or_else(|| base.get(id))
                .map(String::as_str)
                .unwrap_or(id);
            // Compact captions follow the source's current locale, not English.
            if language != "english" {
                let caption = report["localisation"][language][id]["name"]
                    .as_str()
                    .filter(|s| *s != id)
                    .unwrap_or(caption);
                for (state, color) in [
                    ("done", "G"),
                    ("busy", "B"),
                    ("offered", "Y"),
                    ("locked", "L"),
                ] {
                    texts.insert(
                        format!("tte_caption_{state}_{id}"),
                        format!("§{color}{}§!", view::compact_name(caption, 15, 2)),
                    );
                }
            }
            for key in [id.clone(), format!("{id}_desc")] {
                if !native.contains_key(&key) {
                    texts.insert(
                        key.clone(),
                        base.get(&key).cloned().unwrap_or_else(|| {
                            if key.ends_with("_desc") {
                                String::new()
                            } else {
                                key.replace('_', " ")
                            }
                        }),
                    );
                }
            }
        }
        // Import only absent source strings referenced by our UI. Follow their
        // static $keys$ recursively; do not copy the entire source locale or
        // overwrite an upstream translation. Unknown engine substitutions stay.
        fn references(s: &str) -> Vec<String> {
            let mut r = Vec::new();
            let mut rest = s;
            while let Some(a) = rest.find('$') {
                let tail = &rest[a + 1..];
                let Some(b) = tail.find('$') else { break };
                let key = tail[..b].split('|').next().unwrap();
                if crate::catalog::identifier(key) {
                    r.push(key.into())
                }
                rest = &tail[b + 1..];
            }
            r
        }
        let mut pending: Vec<_> = texts.values().flat_map(|s| references(s)).collect();
        let mut seen = BTreeSet::new();
        while let Some(key) = pending.pop() {
            if !seen.insert(key.clone()) {
                continue;
            }
            if seen.len() > 100000 {
                return Err("Source locale reference budget exceeded".into());
            }
            if native.contains_key(&key) || texts.contains_key(&key) {
                continue;
            }
            if let Some(value) = base.get(&key) {
                pending.extend(references(value));
                texts.insert(key, value.clone());
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protected_engine_values_are_never_translated() {
        let p = BTreeMap::from([("Goal".into(), vec!["目標".into(); 8])]);
        assert_eq!(
            translate(
                "§HGoal§! $source_key$ [Root.UntrustedGetter] £physics£\\n",
                0,
                &p,
                &mut BTreeMap::new()
            )
            .unwrap(),
            "§H目標§! $source_key$ [Root.UntrustedGetter] £physics£\\n"
        );
        assert!(translate("Forgotten prose", 0, &p, &mut BTreeMap::new()).is_err());
    }
}
