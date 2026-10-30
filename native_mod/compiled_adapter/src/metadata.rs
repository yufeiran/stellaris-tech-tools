//! Reverse-index standard unlock prerequisites and purpose tags from effective
//! source objects. These display hints NEVER become eligibility rules.
use crate::{
    catalog, planning,
    script::{self, Result, Value},
    sources::Sources,
};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet};
const KINDS: [(&str, &str); 7] = [
    ("buildings", "building"),
    ("districts", "district"),
    ("starbase_buildings", "building"),
    ("starbase_modules", "module"),
    ("armies", "army"),
    ("megastructures", "megastructure"),
    ("component_templates", "component"),
];
pub fn is_object_folder(path: &str) -> bool {
    KINDS.iter().any(|(p, _)| path == format!("common/{p}"))
}
pub fn vocabulary() -> Json {
    serde_json::from_str(include_str!("../assets/browse-vocabulary.json"))
        .expect("compiled browse vocabulary")
}
fn titles(v: &Value, out: &mut BTreeSet<String>) {
    for e in v.entries() {
        if e.key.as_deref() == Some("title") {
            if let Some(s) = e.value.atom().filter(|s| catalog::identifier(s)) {
                out.insert(s.into());
            }
        }
        titles(&e.value, out);
    }
}
fn effects(block: &Value, values: &BTreeMap<String, String>, vocab: &Json) -> Vec<String> {
    let Some(modifier) = block.first("modifier") else {
        return Vec::new();
    };
    let has_description = modifier.first("description").is_some();
    let mut result = Vec::new();
    for e in modifier.entries() {
        let Some(key) = e.key.as_deref() else {
            continue;
        };
        if key == "show_only_custom_tooltip" {
            continue;
        }
        let Some(raw) = e.value.atom() else { continue };
        if matches!(key, "description" | "custom_tooltip") {
            if catalog::identifier(raw) {
                result.push(format!("${raw}$"));
            }
            continue;
        }
        if has_description {
            continue;
        }
        let candidates = [
            key.to_owned(),
            format!("MOD_{}", key.to_uppercase()),
            format!("mod_{key}"),
            vocab["modifierAliases"][key].as_str().unwrap_or("").into(),
            format!("MOD_COUNTRY_{}", key.to_uppercase()),
        ];
        let label = candidates
            .iter()
            .find(|k| values.contains_key(*k))
            .map(|k| format!("${k}$"))
            .unwrap_or_else(|| key.replace('_', " "));
        let value = if let Ok(n) = raw.parse::<f64>() {
            let percent = key.ends_with("_mult")
                || key.ends_with("_add_perc")
                || vocab["percentModifiers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v.as_str() == Some(key));
            if percent {
                format!(
                    "{}%",
                    format!("{:+.6}", n * 100.0)
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                )
            } else {
                format!("{n:+}")
            }
        } else {
            raw.into()
        };
        result.push(format!("{label} {value}"));
    }
    let mut seen = BTreeSet::new();
    result.retain(|v| seen.insert(v.clone()));
    result
}
pub fn enrich(sources: &mut Sources, report: &mut Json) -> Result<()> {
    let mut objects: BTreeMap<(String, String), Json> = BTreeMap::new();
    let paths: Vec<_> = sources
        .files
        .keys()
        .filter(|p| {
            p.ends_with(".txt")
                && KINDS
                    .iter()
                    .any(|(k, _)| p.starts_with(&format!("common/{k}/")))
        })
        .cloned()
        .collect();
    let mut diagnostics = Vec::new();
    for path in paths {
        let input = sources.files[&path].clone();
        let (namespace, kind) = KINDS
            .iter()
            .find(|(k, _)| path.starts_with(&format!("common/{k}/")))
            .unwrap();
        let raw = match sources.expand(&input.raw, &[]) {
            Ok(s) => s,
            Err(e) => {
                diagnostics.push(
                    json!({"file":input.relative,"issue":"unlock_metadata_unknown","detail":e}),
                );
                continue;
            }
        };
        let ast = match script::parse(&raw) {
            Ok(v) => v,
            Err(e) => {
                diagnostics.push(
                    json!({"file":input.relative,"issue":"unlock_metadata_unknown","detail":e}),
                );
                continue;
            }
        };
        for e in ast
            .entries()
            .iter()
            .filter(|e| matches!(&e.value, Value::Block(_)))
        {
            let Some(key) = e
                .value
                .scalar("key")
                .or(e.key.as_deref())
                .filter(|s| catalog::identifier(s))
            else {
                continue;
            };
            let loc = e
                .value
                .scalar("name")
                .filter(|s| catalog::identifier(s))
                .unwrap_or(key);
            let cond = match catalog::condition(e.value.first("prerequisites"), "all") {
                Ok(c) => c,
                Err(e) => {
                    diagnostics.push(json!({"file":input.relative,"object":key,"issue":"unlock_condition_unknown","detail":e}));
                    continue;
                }
            };
            let item = json!({"key":key,"kind":kind,"localizationKey":loc,"condition":cond,"sourceRelativePath":input.relative,"sourceLayer":input.layer});
            // Object IDs are scoped to their source kind. An army and a
            // building with the same key must not erase each other's unlock.
            if let Some(previous) = objects.insert(((*namespace).into(), key.into()), item) {
                diagnostics.push(json!({"object":key,"issue":"duplicate_unlock_object","selectedFile":input.relative,"previousFile":previous["sourceRelativePath"],"policy":"lexical-last analysis; engine precedence unverified"}));
            }
        }
    }
    let mut reverse: BTreeMap<String, Vec<Json>> = BTreeMap::new();
    for item in objects.values() {
        for id in catalog::ids(&item["condition"]) {
            reverse.entry(id).or_default().push(item.clone());
        }
    }
    let techs = planning::technologies(report)?;
    let vocab = vocabulary();
    let (values, _) = sources.localisation("english");
    let mut rows = BTreeMap::new();
    for tech in techs {
        let mut unlocks = reverse.remove(&tech.id).unwrap_or_default();
        let mut extra = BTreeSet::new();
        if let Some(v) = tech.block.first("prereqfor_desc") {
            titles(v, &mut extra)
        }
        for key in extra {
            if !unlocks.iter().any(|u| u["key"] == key) {
                unlocks.push(json!({"key":key,"kind":"tooltip","localizationKey":key}));
            }
        }
        let mut tokens = vec![
            tech.id.clone(),
            tech.block
                .first("category")
                .map(planning::render)
                .unwrap_or_default(),
        ];
        tokens.extend(
            unlocks
                .iter()
                .filter_map(|u| u["key"].as_str().map(str::to_owned)),
        );
        if let Some(modifier) = tech.block.first("modifier") {
            tokens.extend(modifier.entries().iter().filter_map(|e| e.key.clone()));
        }
        let searchable = tokens.join(" ").to_lowercase();
        let mut groups: BTreeSet<String> = vocab["groups"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|g| {
                g["needles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|n| searchable.contains(n.as_str().unwrap()))
            })
            .map(|g| g["key"].as_str().unwrap().into())
            .collect();
        for (kind, group) in [("megastructure", "megastructures"), ("army", "armies")] {
            if unlocks.iter().any(|u| u["kind"] == kind) {
                groups.insert(group.into());
            }
        }
        if groups.is_empty() {
            groups.insert("other".into());
        }
        rows.insert(tech.id.clone(),json!({"unlocks":unlocks,"effects":effects(&tech.block,&values,&vocab),"groups":groups}));
    }
    report["browseMetadata"] = json!(rows);
    report["browseDiagnostics"] = json!(diagnostics);
    report["browseSemantics"]=json!("Display hints from standard prerequisite fields, not eligibility or guaranteed unlocks; special conditions remain engine-controlled");
    report["inlineScriptsRead"] = json!(sources.inline_reads);
    report["sourceFingerprint"] = json!(sources.fingerprint());
    Ok(())
}
