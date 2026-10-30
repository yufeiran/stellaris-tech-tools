use crate::{
    script::{self, Result, Value},
    sources::{sha, Sources},
};
use serde_json::{json, Value as Json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone)]
struct Tech {
    record: Json,
    block: Value,
    start_chars: usize,
    end_chars: usize,
}

pub fn identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(),Some(c) if c.is_ascii_alphabetic()||c=='_')
        && chars.all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
}
fn used_variables(v: &Value, all: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    fn walk(v: &Value, needed: &mut BTreeSet<String>) {
        match v {
            Value::Atom(s) if s.starts_with('@') => {
                needed.insert(s.clone());
            }
            Value::Block(es) => {
                for e in es {
                    walk(&e.value, needed)
                }
            }
            _ => {}
        }
    }
    let mut needed = BTreeSet::new();
    walk(v, &mut needed);
    let mut todo: Vec<_> = needed.into_iter().collect();
    let mut used = BTreeMap::new();
    while let Some(key) = todo.pop() {
        if used.contains_key(&key) {
            continue;
        }
        if let Some(value) = all.get(&key) {
            used.insert(key, value.clone());
            if value.starts_with('@') {
                todo.push(value.clone());
            }
        }
    }
    used
}
fn combine(op: &str, children: Vec<Json>) -> Json {
    let identity = op == "all";
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for child in children {
        if child == json!(!identity) {
            return json!(!identity);
        }
        if child == json!(identity) {
            continue;
        }
        let list = if child.get(0) == Some(&json!(op)) {
            child.as_array().unwrap()[1..].to_vec()
        } else {
            vec![child]
        };
        for child in list {
            if seen.insert(child.to_string()) {
                result.push(child);
            }
        }
    }
    if result.is_empty() {
        json!(identity)
    } else if result.len() == 1 {
        result.remove(0)
    } else {
        let mut out = vec![json!(op)];
        out.extend(result);
        Json::Array(out)
    }
}
pub fn condition(value: Option<&Value>, op: &str) -> Result<Json> {
    let Some(value) = value else {
        return Ok(json!(true));
    };
    if !matches!(value, Value::Block(_)) {
        return Err("Prerequisites must be a block".into());
    }
    let mut children = Vec::new();
    for e in value.entries() {
        match (&e.key, &e.value) {
            (None, Value::Atom(id)) if identifier(id) => children.push(json!(["tech", id])),
            (Some(key), block)
                if e.op == "="
                    && (key.eq_ignore_ascii_case("OR") || key.eq_ignore_ascii_case("AND")) =>
            {
                children.push(condition(
                    Some(block),
                    if key.eq_ignore_ascii_case("OR") {
                        "any"
                    } else {
                        "all"
                    },
                )?)
            }
            _ => return Err("Unsupported prerequisite expression; no guessing".into()),
        }
    }
    Ok(combine(op, children))
}
pub fn ids(expr: &Json) -> Vec<String> {
    let mut out = Vec::new();
    fn visit(e: &Json, out: &mut Vec<String>) {
        if let Some(list) = e.as_array() {
            if list.first().and_then(Json::as_str) == Some("tech") {
                if let Some(id) = list.get(1).and_then(Json::as_str) {
                    if !out.iter().any(|s| s == id) {
                        out.push(id.to_owned());
                    }
                }
            } else {
                for child in &list[1..] {
                    visit(child, out);
                }
            }
        }
    }
    visit(expr, &mut out);
    out
}
fn resolve(raw: &str, variables: &BTreeMap<String, String>) -> Result<f64> {
    let mut current = raw.trim().to_owned();
    let mut seen = BTreeSet::new();
    for _ in 0..64 {
        if !current.starts_with('@') {
            let n: f64 = current
                .parse()
                .map_err(|_| format!("Unsupported numeric expression: {current}"))?;
            return if n.is_finite() {
                Ok(n)
            } else {
                Err("Non-finite source number".into())
            };
        }
        if !seen.insert(current.clone()) {
            return Err(format!("Scripted-variable cycle: {current}"));
        }
        current = variables
            .get(&current)
            .ok_or_else(|| format!("Unresolved scripted variable: {current}"))?
            .trim()
            .to_owned();
    }
    Err("Scripted-variable depth limit".into())
}
fn numeric(
    block: &Value,
    key: &str,
    default: &str,
    vars: &BTreeMap<String, String>,
) -> Result<f64> {
    let value = block.first(key);
    let raw = match value {
        Some(Value::Atom(s)) => s.as_str(),
        Some(b) => b
            .scalar("factor")
            .ok_or_else(|| format!("{key}: unsupported numeric block without a base factor"))?,
        None => default,
    };
    resolve(raw, vars)
}
fn replace_missing(expr: &Json, known: &BTreeSet<String>) -> Json {
    if expr.get(0) == Some(&json!("tech")) {
        return if known.contains(expr[1].as_str().unwrap()) {
            expr.clone()
        } else {
            json!(false)
        };
    }
    if let Some(list) = expr.as_array() {
        let mut out = vec![list[0].clone()];
        out.extend(list[1..].iter().map(|c| replace_missing(c, known)));
        Json::Array(out)
    } else {
        expr.clone()
    }
}
fn annotated(expr: &Json, known: &BTreeSet<String>, aliases: &BTreeMap<String, Vec<Json>>) -> Json {
    if let Some(value) = expr.as_bool() {
        return json!({"kind":"constant","value":value});
    }
    let list = expr.as_array().unwrap();
    let kind = list[0].as_str().unwrap();
    if kind == "tech" {
        let id = list[1].as_str().unwrap();
        if known.contains(id) {
            return json!({"kind":"tech","id":id,"resolution":"top_level_definition"});
        }
        if let Some(entries) = aliases.get(id) {
            let owners: BTreeSet<_> = entries
                .iter()
                .map(|v| v["canonicalId"].as_str().unwrap().to_owned())
                .collect();
            let mut result = json!({"kind":"unknown","reference":id,"engineLookupVerified":false,
                "reason":if owners.len()==1{"technology_swap_alias_unverified"}else{"ambiguous_technology_swap_alias"},
                "candidateCanonicals":owners,"candidateSources":entries.iter().map(|e|json!({"canonicalId":e["canonicalId"],"source":e["source"],"swapIndex":e["swapIndex"]})).collect::<Vec<_>>()});
            if owners.len() == 1 {
                result["suggestedCanonical"] = json!(owners.first().unwrap());
            }
            return result;
        }
        return json!({"kind":"unknown","reference":id,"reason":"missing_technology_reference","engineLookupVerified":false});
    }
    json!({"kind":kind,"children":list[1..].iter().map(|c|annotated(c,known,aliases)).collect::<Vec<_>>()})
}
fn unknown(expr: &Json, out: &mut Vec<Json>) {
    if expr["kind"] == "unknown" {
        out.push(expr.clone());
    }
    if let Some(children) = expr["children"].as_array() {
        for child in children {
            unknown(child, out);
        }
    }
}

pub fn collect(sources: &mut Sources) -> Result<Json> {
    let mut vars = BTreeMap::new();
    let mut assignments: BTreeMap<String, Vec<Json>> = BTreeMap::new();
    let mut tiers = BTreeSet::new();
    let mut tier_files = false;
    let tier_inputs: Vec<_> = sources
        .files
        .iter()
        .filter(|(key, _)| key.starts_with("common/technology/tier/") && key.ends_with(".txt"))
        .map(|(_, input)| input.clone())
        .collect();
    for input in tier_inputs {
        tier_files = true;
        for e in script::parse(&sources.expand(&input.raw, &[])?)?.entries() {
            if let Some(key) = &e.key {
                if matches!(&e.value, Value::Block(_)) {
                    if let Ok(tier) = key.parse::<i64>() {
                        tiers.insert(tier);
                    }
                }
            }
        }
    }
    for (key, input) in &sources.files {
        if key.starts_with("common/scripted_variables/")
            && key.split('/').count() == 3
            && key.ends_with(".txt")
        {
            for e in script::parse(&input.raw)?.entries() {
                if let Some(k) = &e.key {
                    if k.starts_with('@') {
                        if let Some(v) = e.value.atom() {
                            vars.insert(k.clone(), v.into());
                            assignments
                                .entry(k.clone())
                                .or_default()
                                .push(json!({"file":input.relative,"layer":input.layer,"value":v}));
                        } else {
                            sources.warnings.push(json!({"issue":"block_scripted_variable","variable":k,"file":input.relative}));
                        }
                    }
                }
            }
        }
    }
    for (key, values) in &assignments {
        let distinct: BTreeSet<_> = values.iter().map(|v| v["value"].to_string()).collect();
        if distinct.len() > 1 {
            sources.warnings.push(json!({"issue":"ambiguous_global_variable","variable":key,"assignments":values,"enginePrecedenceVerified":false}));
        }
    }
    let paths: Vec<_> = sources
        .files
        .keys()
        .filter(|k| {
            k.starts_with("common/technology/") && k.split('/').count() == 3 && k.ends_with(".txt")
        })
        .cloned()
        .collect();
    let mut definitions: BTreeMap<String, Tech> = BTreeMap::new();
    for path in paths {
        let input = sources.files[&path].clone();
        let raw: Arc<str> = sources.expand(&input.raw, &[])?.into();
        let parsed = script::parse(&raw)?;
        let filename = input.relative.rsplit('/').next().unwrap().to_owned();
        let mut local = vars.clone();
        for e in parsed.entries() {
            if let Some(k) = &e.key {
                if k.starts_with('@') {
                    if let Some(v) = e.value.atom() {
                        local.insert(k.clone(), v.into());
                    }
                }
            }
        }
        let mut last_byte = 0;
        let mut char_offset = 0;
        for e in parsed.entries() {
            char_offset += raw[last_byte..e.span.start].chars().count();
            let start_chars = char_offset;
            char_offset += raw[e.span.clone()].chars().count();
            let end_chars = char_offset;
            last_byte = e.span.end;
            let Some(id) = &e.key else {
                return Err(format!("Bare top-level technology token in {filename}"));
            };
            if id.starts_with('@') {
                continue;
            }
            if !matches!(e.value, Value::Block(_)) {
                continue;
            }
            if !identifier(id) {
                return Err(format!("Invalid technology ID: {id}"));
            }
            for key in ["area", "tier", "cost", "levels", "prerequisites"] {
                if e.value.all(key).count() > 1 {
                    return Err(format!(
                        "{id}: ambiguous duplicate {key} field; engine precedence is not guessed"
                    ));
                }
            }
            let area = e
                .value
                .scalar("area")
                .ok_or_else(|| format!("{filename}:{id}: no literal area"))?;
            if !["physics", "society", "engineering"].contains(&area) {
                return Err(format!(
                    "{filename}:{id}: unsupported technology area {area}"
                ));
            }
            let tier = numeric(&e.value, "tier", "0", &local)?;
            let cost = numeric(&e.value, "cost", "1000", &local)?;
            let levels = numeric(&e.value, "levels", "1", &local)?;
            if tier.fract() != 0.0
                || levels.fract() != 0.0
                || tier.abs() >= i64::MAX as f64
                || levels.abs() >= i64::MAX as f64
            {
                return Err(format!("{id}: non-integer or out-of-range tier/levels"));
            }
            let cond = condition(e.value.first("prerequisites"), "all")?;
            let mut shadowed = definitions
                .get(id)
                .map(|t| t.record["shadowed"].as_array().unwrap().clone())
                .unwrap_or_default();
            if let Some(previous) = definitions.get(id) {
                shadowed.push(previous.record["source"].clone());
                sources.warnings.push(json!({"issue":"duplicate_technology_definition","id":id,
                    "previous":{"relativePath":previous.record["sourceRelativePath"],"layer":previous.record["sourceLayer"]},
                    "selected":{"relativePath":input.relative,"layer":input.layer},
                    "policy":"legacy lowercased lexical-last analysis policy; engine precedence unverified",
                    "enginePrecedenceVerified":false}));
            }
            let numeric_blocks: BTreeMap<_, _> = ["tier", "cost", "levels"]
                .into_iter()
                .filter_map(|key| {
                    e.value
                        .first(key)
                        .filter(|v| matches!(v, Value::Block(_)))
                        .map(|v| (key, script::data(v)))
                })
                .collect();
            let ambiguous: Vec<_> = [
                "potential",
                "category",
                "icon",
                "is_dangerous",
                "weight_modifier",
            ]
            .into_iter()
            .filter(|k| e.value.all(k).count() > 1)
            .collect();
            if !ambiguous.is_empty() {
                sources.warnings.push(json!({"issue":"ambiguous_optional_technology_fields","id":id,"fields":ambiguous,"handling":"Potential remains unknown; ambiguous display hints are omitted; engine definition preserved"}));
            }
            let record = json!({"id":id,"area":area,"tier":tier as i64,"cost":cost,"condition":cond,"ambiguousOptionalFields":ambiguous,
                "source":filename,"sourceLayer":input.layer,"tierDefined":!tier_files||tiers.contains(&(tier as i64)),
                "repeatable":levels == -1.0||levels>1.0,"levels":levels as i64,"shadowed":shadowed,
                "sourceRelativePath":input.relative,"rawDefinitionFields":script::data(&e.value),"scriptedVariables":used_variables(&e.value,&local),
                "tierDefaulted":e.value.first("tier").is_none(),"costDefaulted":e.value.first("cost").is_none(),
                "prerequisiteOperatorSemanticsVerified":false,
                "numericBlocks":numeric_blocks,"numericValueSemantics":"base factor only; dynamic modifiers remain engine-controlled",
                "definitionSpanBasis":"expanded BOM-stripped LF-normalised source; character offsets",
                "definitionSha256":sha(raw[e.span.clone()].as_bytes())});
            definitions.insert(
                id.clone(),
                Tech {
                    record,
                    block: e.value.clone(),
                    start_chars,
                    end_chars,
                },
            );
        }
    }
    if definitions.is_empty() {
        return Err("No technology definitions found".into());
    }
    let known: BTreeSet<_> = definitions.keys().cloned().collect();
    let mut by_name: BTreeMap<String, Vec<Json>> = BTreeMap::new();
    let mut by_base: BTreeMap<String, Vec<Json>> = BTreeMap::new();
    let mut diagnostics = Vec::new();
    for (id, tech) in &definitions {
        for (index, swap) in tech.block.all("technology_swap").enumerate() {
            if swap.all("name").count() > 1 {
                diagnostics.push(json!({"canonicalId":id,"swapIndex":index,"reason":"ambiguous_duplicate_swap_name"}));
                continue;
            }
            let Some(name) = swap.scalar("name") else {
                diagnostics.push(json!({"canonicalId":id,"swapIndex":index,"reason":"missing_or_invalid_swap_name"}));
                continue;
            };
            if !identifier(name) {
                diagnostics.push(json!({"canonicalId":id,"swapIndex":index,"reason":"missing_or_invalid_swap_name"}));
                continue;
            }
            let entry = json!({"canonicalId":id,"source":tech.record["source"],"swapIndex":index,"name":name,
                "shadowedSources":tech.record["shadowed"],"engineLookupVerified":false,"firstMatchingSwapWins":true,"earlierSwapIndices":(0..index).collect::<Vec<_>>(),
                "trigger":swap.first("trigger").map(script::data),"areaOverride":swap.first("area").map(script::data),
                "categoryOverride":swap.first("category").map(script::data),"inheritIcon":swap.first("inherit_icon").map(script::data),
                "inheritEffects":swap.first("inherit_effects").map(script::data),"rawSwap":script::data(swap),
                "definitionSpan":{"start":tech.start_chars,"end":tech.end_chars}});
            by_name.entry(name.into()).or_default().push(entry.clone());
            by_base.entry(id.clone()).or_default().push(entry);
        }
    }
    let aliases = json!({"byName":by_name,"byBase":by_base,"diagnostics":diagnostics,"engineLookupVerified":false,
        "scope":"Display-swap source metadata, not engine technology-ID aliases"});
    let mut counts = vec![0usize; sources.layers.len()];
    let mut records = BTreeMap::new();
    let mut adjacency: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, mut tech) in definitions {
        let original = tech.record["condition"].clone();
        let reference = annotated(&original, &known, &by_name);
        let mut unresolved = Vec::new();
        unknown(&reference, &mut unresolved);
        tech.record["referenceCondition"] = reference;
        if !unresolved.is_empty() {
            tech.record["unknownPrerequisiteReferences"] = json!(unresolved);
            tech.record["conditionAnalysis"] =
                json!("incomplete; unknown references preserved in referenceCondition");
        }
        let missing: Vec<_> = ids(&original)
            .into_iter()
            .filter(|k| !known.contains(k))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if !missing.is_empty() {
            tech.record["missingPrerequisites"] = json!(missing);
            tech.record["originalCondition"] = original.clone();
            tech.record["condition"] = replace_missing(&original, &known);
        }
        adjacency.insert(id.clone(), ids(&tech.record["condition"]));
        counts[tech.record["sourceLayer"].as_u64().unwrap() as usize] += 1;
        records.insert(id, tech.record);
    }
    // Iterative DFS accepts long real dependency chains without native stack overflow.
    let mut state: BTreeMap<String, u8> = BTreeMap::new();
    for id in adjacency.keys() {
        if state.get(id) == Some(&2) {
            continue;
        }
        let mut todo = vec![(id.clone(), false)];
        while let Some((node, exit)) = todo.pop() {
            if exit {
                state.insert(node, 2);
                continue;
            }
            match state.get(&node) {
                Some(2) => continue,
                Some(1) => return Err(format!("Cyclic prerequisites: {node}")),
                _ => {}
            }
            state.insert(node.clone(), 1);
            todo.push((node.clone(), true));
            for parent in adjacency[&node].iter().rev() {
                todo.push((parent.clone(), false));
            }
        }
    }
    let mut localisations = BTreeMap::new();
    for language in &sources.languages {
        let (values, warnings) = sources.localisation(language);
        sources.warnings.extend(warnings);
        let mut names = BTreeMap::new();
        for id in records.keys() {
            fn localise(id: &str, values: &BTreeMap<String, String>) -> Result<String> {
                let mut s = values.get(id).cloned().unwrap_or_else(|| id.into());
                const LIMIT: usize = 65536;
                if s.len() > LIMIT {
                    return Err(format!(
                        "Localisation {id} exceeds display expansion budget"
                    ));
                }
                for _ in 0..12 {
                    let mut out = String::new();
                    let mut rest = s.as_str();
                    let mut changed = false;
                    while let Some(begin) = rest.find('$') {
                        out.push_str(&rest[..begin]);
                        if out.len() > LIMIT {
                            return Err(format!(
                                "Localisation {id} exceeds display expansion budget"
                            ));
                        }
                        let tail = &rest[begin + 1..];
                        if let Some(end) = tail.find('$') {
                            if let Some(value) = values.get(&tail[..end]) {
                                if value.len() > LIMIT.saturating_sub(out.len()) {
                                    return Err(format!(
                                        "Localisation {id} exceeds display expansion budget"
                                    ));
                                }
                                out.push_str(value);
                                changed = true;
                            } else {
                                out.push_str(&rest[begin..begin + end + 2]);
                            }
                            rest = &tail[end + 1..];
                        } else {
                            out.push_str(&rest[begin..]);
                            rest = "";
                            break;
                        }
                    }
                    out.push_str(rest);
                    if out.len() > LIMIT {
                        return Err(format!(
                            "Localisation {id} exceeds display expansion budget"
                        ));
                    }
                    if !changed || out == s {
                        return Ok(out);
                    }
                    s = out;
                }
                Ok(s)
            }
            names.insert(id.clone(),json!({"name":localise(id,&values)?,"description":localise(&(id.clone()+"_desc"),&values)?}));
        }
        localisations.insert(language.clone(), names);
    }
    let layers:Vec<_>=sources.layers.iter().enumerate().map(|(i,l)|json!({"name":l.name,"root":crate::sources::path_json(&l.root),"kind":if l.archive_hash.is_some(){"zip"}else{"directory"},"archiveSha256":l.archive_hash,"effectiveTechnologies":counts[i]})).collect();
    Ok(
        json!({"format":1,"backend":"compiled-rust","pythonRuntime":false,"runtimeDiscovery":false,
        "capabilities":{"collection":true,"dependencyGraph":true,"gameScriptGeneration":false},
        "analysisPolicy":{"enginePrecedenceVerified":false,"definitionOrder":"legacy lowercased lexical-last",
        "replacePath":"recursive directory prefix","localisation":"source parser; not engine-validated"},
        "definedTiers":if tier_files{json!(tiers)}else{Json::Null},"technologyAliases":aliases,
        "technologies":records,"localisation":localisations,"sources":layers,
        "fileCollisions":sources.collisions,"replacePaths":sources.replacements,"warnings":sources.warnings,
        "inlineScriptsRead":sources.inline_reads,"sourceFingerprint":sources.fingerprint(),
        "specialUnlockAnalysis":"unknown / engine-controlled","gameEngineValidation":"NOT RUN"}),
    )
}

pub fn route(catalog: &Json, goal: &str, owned: &BTreeSet<String>) -> Result<Json> {
    let definitions = catalog["technologies"]
        .as_object()
        .ok_or("Invalid catalog")?;
    if !definitions.contains_key(goal) {
        return Err(format!("Unknown target technology {goal}"));
    }
    for id in owned {
        if !definitions.contains_key(id) {
            return Err(format!("Unknown --owned technology {id}"));
        }
    }
    fn satisfied(e: &Json, owned: &BTreeSet<String>) -> bool {
        if let Some(v) = e.as_bool() {
            return v;
        }
        let list = e.as_array().unwrap();
        match list[0].as_str().unwrap() {
            "tech" => owned.contains(list[1].as_str().unwrap()),
            "all" => list[1..].iter().all(|x| satisfied(x, owned)),
            _ => list[1..].iter().any(|x| satisfied(x, owned)),
        }
    }
    let mut result = BTreeSet::new();
    let mut todo = vec![goal.to_owned()];
    let mut uncertain = Vec::new();
    while let Some(id) = todo.pop() {
        if owned.contains(&id) || !result.insert(id.clone()) {
            continue;
        }
        let tech = &definitions[&id];
        if let Some(missing) = tech.get("missingPrerequisites") {
            uncertain.push(json!({"technology":id,"missing":missing}));
        }
        let mut exprs = vec![&tech["condition"]];
        while let Some(e) = exprs.pop() {
            if satisfied(e, owned) || e.is_boolean() {
                continue;
            }
            let list = e.as_array().unwrap();
            if list[0] == "tech" {
                todo.push(list[1].as_str().unwrap().to_owned());
            } else {
                exprs.extend(list[1..].iter());
            }
        }
    }
    Ok(
        json!({"goal":goal,"candidateClosure":result,"unknown":uncertain,"goalCondition":definitions[goal]["condition"],
            "semantics":"union of unsatisfied candidate branches, NOT all required; prune satisfied OR; engine eligibility and tier gates remain game-controlled"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_or_and_dedup() {
        let v = script::parse("p={a OR={b c} a}").unwrap();
        assert_eq!(
            condition(v.first("p"), "all").unwrap(),
            json!(["all", ["tech", "a"], ["any", ["tech", "b"], ["tech", "c"]]])
        );
    }
    #[test]
    fn unknown_leaves_do_not_become_real_alias_nodes() {
        let known = BTreeSet::from(["base".to_owned()]);
        let aliases = BTreeMap::from([(
            "alias".to_owned(),
            vec![json!({"canonicalId":"base","source":"x","swapIndex":0})],
        )]);
        assert_eq!(
            annotated(&json!(["tech", "alias"]), &known, &aliases)["kind"],
            "unknown"
        );
    }
    #[test]
    fn route_prunes_satisfied_or_branch() {
        let c = json!({"technologies":{"goal":{"condition":["any",["tech","a"],["tech","b"]]},"a":{"condition":true},"b":{"condition":true}}});
        let r = route(&c, "goal", &BTreeSet::from(["a".into()])).unwrap();
        assert_eq!(r["candidateClosure"], json!(["goal"]));
    }
}
