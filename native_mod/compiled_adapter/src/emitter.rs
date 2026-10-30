//! Native emitter migration. This module is not exposed as a usable game build
//! until planner/UI emission, preservation checks and engine regression pass.
use crate::{
    script::{self, Entry, Result, Value},
    sources::{sha, Sources},
};
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;

fn weight(raw: &str, entries: &[Entry], id: &str, cost: f64) -> Result<String> {
    let gate = format!("tte_vanilla_{id} = yes");
    let mut original = Vec::new();
    let mut additive_seen = false;
    for item in entries {
        if item.key.as_deref() == Some("modifier") && item.value.first("add").is_some() {
            additive_seen = true;
        }
        if additive_seen && matches!(item.key.as_deref(), Some("weight" | "base" | "factor")) {
            return Err(format!(
                "{id}: additive AI modifier before base weight is not safely transformable"
            ));
        }
        match (item.key.as_deref(), &item.value) {
            (Some("weight" | "base" | "factor"), Value::Atom(s)) => original.push(format!(
                "modifier = {{ factor = {} {gate} }}",
                crate::planning::atom(s)
            )),
            (Some("modifier"), Value::Block(_)) => {
                let part = &raw[item.span.clone()];
                original.push(format!("{}\n{gate}\n}}", &part[..part.len() - 1]));
            }
            _ => {
                return Err(format!(
                    "{id}: unsupported AI weight field; no partial patch"
                ))
            }
        }
    }
    let factor = (10000000.0 / cost.max(1.0)).clamp(10.0, 10000.0);
    let ordinary = (factor / 100.0).clamp(10.0, 100.0);
    let extra = (factor / 200.0).clamp(10.0, 100.0);
    Ok(format!("ai_weight = {{\nweight = 1\n{}\nmodifier = {{ weight = {ordinary:.6} tte_filler_{id} = yes NOT = {{ has_country_flag = tte_extra_branch_{id} }} }}\nmodifier = {{ weight = {extra:.6} tte_filler_{id} = yes has_country_flag = tte_extra_branch_{id} }}\nmodifier = {{ weight = 1000 tte_priority_{id} = yes }}\nmodifier = {{ weight = 10000 tte_priority_{id} = yes has_country_flag = tte_goal_{id} }}\nmodifier = {{ factor = 0 is_ai = no has_country_flag = tte_automation NOT = {{ tte_admissible_{id} = yes }} }}\n}}",original.join("\n")))
}

fn without_weight(value: &Value) -> Json {
    let mut copy = value.clone();
    if let Value::Block(entries) = &mut copy {
        entries.retain(|e| e.key.as_deref() != Some("ai_weight"));
    }
    script::data(&copy)
}

fn edit(raw: &str, tech: &Entry, id: &str, cost: f64) -> Result<(usize, usize, String)> {
    let weights: Vec<_> = tech
        .value
        .entries()
        .iter()
        .filter(|e| e.key.as_deref() == Some("ai_weight"))
        .collect();
    if weights.len() > 1 {
        return Err(format!("{id}: duplicate AI weights"));
    }
    if let Some(w) = weights.first() {
        if !matches!(w.value, Value::Block(_)) {
            return Err(format!("{id}: AI weight must be a block"));
        }
        Ok((
            w.span.start,
            w.span.end,
            weight(raw, w.value.entries(), id, cost)?,
        ))
    } else {
        let at = tech.span.end - 1;
        Ok((at, at, format!("\n{}\n", weight(raw, &[], id, cost)?)))
    }
}

pub struct WeightOutput {
    pub files: BTreeMap<String, String>,
    pub proof: Vec<Json>,
}

pub fn technology_weights(sources: &mut Sources, catalog: &Json) -> Result<WeightOutput> {
    let definitions = catalog["technologies"]
        .as_object()
        .ok_or("Missing catalog")?;
    let paths: Vec<_> = sources
        .files
        .keys()
        .filter(|k| {
            k.starts_with("common/technology/") && k.ends_with(".txt") && k.split('/').count() == 3
        })
        .cloned()
        .collect();
    let mut files = BTreeMap::new();
    let mut proof = Vec::new();
    for path in paths {
        let input = sources.files[&path].clone();
        let raw = sources.expand(&input.raw, &[])?;
        let parsed = script::parse(&raw)?;
        let mut edits = Vec::new();
        let mut before = BTreeMap::new();
        for entry in parsed.entries() {
            let Some(id) = &entry.key else { continue };
            let Some(tech) = definitions.get(id) else {
                continue;
            };
            if tech["sourceRelativePath"] != input.relative
                || tech["sourceLayer"] != input.layer
                || tech["definitionSha256"] != sha(raw[entry.span.clone()].as_bytes())
            {
                continue;
            }
            if before
                .insert(id.clone(), without_weight(&entry.value))
                .is_some()
            {
                return Err("Indistinguishable duplicate definitions in one file".into());
            }
            edits.push(edit(
                &raw,
                entry,
                id,
                tech["cost"].as_f64().ok_or("Invalid catalog cost")?,
            )?);
        }
        if edits.is_empty() {
            continue;
        }
        let mut patched = raw;
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        for (start, end, replacement) in edits {
            patched.replace_range(start..end, &replacement);
        }
        let after = script::parse(&patched)?;
        for (id, expected) in before {
            let actual = after
                .entries()
                .iter()
                .rev()
                .find(|e| e.key.as_deref() == Some(&id))
                .ok_or("Patched technology missing")?;
            let actual = without_weight(&actual.value);
            if actual != expected {
                return Err(format!("{id}: non-ai_weight source fields changed"));
            }
            proof.push(json!({"id":id,"file":input.relative,"nonAiWeightSha256":sha(expected.to_string().as_bytes()),"status":"PASS"}));
        }
        files.insert(input.relative, patched);
    }
    if proof.len() != definitions.len() {
        return Err("Every effective technology requires a source-field preservation proof".into());
    }
    Ok(WeightOutput { files, proof })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn patch(raw: &str) -> String {
        let parsed = script::parse(raw).unwrap();
        let t = &parsed.entries()[0];
        let before = without_weight(&t.value);
        let (start, end, replacement) = edit(raw, t, "probe", 4000.0).unwrap();
        let mut after = raw.to_owned();
        after.replace_range(start..end, &replacement);
        assert_eq!(
            without_weight(&script::parse(&after).unwrap().entries()[0].value),
            before
        );
        after
    }
    #[test]
    fn only_ai_weight_is_changed_in_complex_definition() {
        let raw="probe={area=engineering cost=@price prerequisites={OR={a b}} potential={NOT={has_origin=lost_colony}} ai_weight={weight=3 modifier={factor=0 has_country_flag=blocked}} modifier={ship_speed_mult=0.05} effect={country_event={id=other.1}}}";
        let out = patch(raw);
        assert!(out.contains("factor=0 has_country_flag=blocked\ntte_vanilla_probe"));
        assert!(out.contains("weight = 25.000000 tte_filler_probe"));
    }
    #[test]
    fn missing_weight_adds_only_the_ai_field() {
        assert!(patch("probe={area=physics cost=5}").contains("tte_admissible_probe"));
    }
    #[test]
    fn unsupported_weight_never_disappears_silently() {
        let raw = "probe={area=physics ai_weight={unexpected=yes}}";
        let parsed = script::parse(raw).unwrap();
        assert!(edit(raw, &parsed.entries()[0], "probe", 5.0).is_err());
    }
    #[test]
    fn additive_modifier_before_base_is_rejected_without_reordering() {
        let raw = "probe={area=physics ai_weight={modifier={add=2 always=yes} weight=3}}";
        let parsed = script::parse(raw).unwrap();
        assert!(edit(raw, &parsed.entries()[0], "probe", 5.0)
            .unwrap_err()
            .contains("before base weight"));
    }
}
