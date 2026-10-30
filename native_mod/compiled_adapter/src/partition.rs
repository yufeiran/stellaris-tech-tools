//! Bound parser/formatter units without splitting scopes or if/else chains.
//! This only operates on our generated country effects/events, not source data.
use crate::{
    script::{self, Entry, Result, Value},
    sources::sha,
};
use std::collections::BTreeMap;
pub const FILE_BUDGET: usize = 60000;
const EFFECT_BUDGET: usize = 10000;
pub fn cost(v: &Value) -> usize {
    match v {
        Value::Atom(_) => 1,
        Value::Block(e) => 2 + e.iter().map(|e| 1 + cost(&e.value)).sum::<usize>(),
    }
}
pub fn formatted_lines(v: &Value) -> usize {
    match v {
        Value::Atom(_) => 1,
        Value::Block(es) => 2 + es.iter().map(|e| formatted_lines(&e.value)).sum::<usize>(),
    }
}
/// A shallow call graph can still allocate quadratically: an O(N) dispatcher
/// expanded into O(N) event choices. Country events form an explicit boundary.
/// This is a structural estimate, deliberately not a byte/RAM prediction.
pub fn eager_effect_guard(raw: &str, roots: &[String]) -> Result<(usize, String)> {
    let ast = script::parse(raw)?;
    let definitions: BTreeMap<_, _> = ast
        .entries()
        .iter()
        .map(|e| Ok((e.key.clone().ok_or("Missing effect name")?, e.value.clone())))
        .collect::<Result<_>>()?;
    fn expanded(
        name: &str,
        defs: &BTreeMap<String, Value>,
        cache: &mut BTreeMap<String, usize>,
        stack: &mut Vec<String>,
    ) -> Result<usize> {
        if let Some(c) = cache.get(name) {
            return Ok(*c);
        }
        if stack.len() >= 64 || stack.iter().any(|s| s == name) {
            return Err(format!("Recursive effect expansion {name}"));
        }
        fn score(
            v: &Value,
            defs: &BTreeMap<String, Value>,
            cache: &mut BTreeMap<String, usize>,
            stack: &mut Vec<String>,
        ) -> Result<usize> {
            let mut total = 2usize;
            for e in v.entries() {
                let child = if let Some(k) = e
                    .key
                    .as_deref()
                    .filter(|k| defs.contains_key(*k) && e.value.atom().is_some())
                {
                    expanded(k, defs, cache, stack)?
                } else if matches!(e.value, Value::Block(_)) {
                    score(&e.value, defs, cache, stack)?
                } else {
                    1
                };
                total = total.saturating_add(1 + child);
            }
            Ok(total)
        }
        stack.push(name.into());
        let n = score(&defs[name], defs, cache, stack)?;
        stack.pop();
        cache.insert(name.into(), n);
        Ok(n)
    }
    fn count(v: &Value, defs: &BTreeMap<String, Value>, uses: &mut BTreeMap<String, usize>) {
        for e in v.entries() {
            if let Some(k) = e
                .key
                .as_deref()
                .filter(|k| defs.contains_key(*k) && e.value.atom().is_some())
            {
                *uses.entry(k.into()).or_default() += 1;
            }
            count(&e.value, defs, uses);
        }
    }
    let mut uses = BTreeMap::new();
    for value in definitions.values() {
        count(value, &definitions, &mut uses);
    }
    for raw in roots {
        let ast = script::parse(raw)?;
        for e in ast.entries() {
            count(&e.value, &definitions, &mut uses);
        }
    }
    let mut cache = BTreeMap::new();
    let mut maximum = (0, String::new());
    for (name, n) in uses {
        let score = expanded(&name, &definitions, &mut cache, &mut Vec::new())?.saturating_mul(n);
        if score > maximum.0 {
            maximum = (score, name);
        }
    }
    if maximum.0 > 1000000 {
        return Err(format!("Repeated eager effect expansion exceeds structural budget: {} ({} estimated nodes); use a hidden country-event gateway",maximum.1,maximum.0));
    }
    Ok(maximum)
}
pub fn effect_depth(raw: &str) -> Result<(usize, Vec<String>)> {
    let parsed = script::parse(raw)?;
    let definitions: BTreeMap<_, _> = parsed
        .entries()
        .iter()
        .map(|e| Ok((e.key.clone().ok_or("Effect missing name")?, e.value.clone())))
        .collect::<Result<_>>()?;
    fn calls(
        v: &Value,
        definitions: &BTreeMap<String, Value>,
        out: &mut std::collections::BTreeSet<String>,
    ) {
        for e in v.entries() {
            if let Some(k) = &e.key {
                if definitions.contains_key(k) {
                    out.insert(k.clone());
                }
            }
            calls(&e.value, definitions, out);
        }
    }
    let mut graph = BTreeMap::new();
    for (name, value) in &definitions {
        let mut set = std::collections::BTreeSet::new();
        calls(value, &definitions, &mut set);
        graph.insert(name.clone(), set);
    }
    fn walk(
        name: &str,
        graph: &BTreeMap<String, std::collections::BTreeSet<String>>,
        cache: &mut BTreeMap<String, Vec<String>>,
        pending: &mut std::collections::BTreeSet<String>,
    ) -> Result<Vec<String>> {
        if let Some(v) = cache.get(name) {
            return Ok(v.clone());
        }
        if pending.len() >= 64 || !pending.insert(name.into()) {
            return Err(format!("Recursive/deep generated effect chain: {name}"));
        }
        let mut longest = Vec::new();
        for next in &graph[name] {
            let path = walk(next, graph, cache, pending)?;
            if path.len() > longest.len() {
                longest = path;
            }
        }
        pending.remove(name);
        longest.insert(0, name.into());
        cache.insert(name.into(), longest.clone());
        Ok(longest)
    }
    let mut longest = Vec::new();
    let mut cache = BTreeMap::new();
    for name in graph.keys() {
        let path = walk(
            name,
            &graph,
            &mut cache,
            &mut std::collections::BTreeSet::new(),
        )?;
        if path.len() > longest.len() {
            longest = path;
        }
    }
    if longest.len() > 4 {
        return Err(format!(
            "Generated effect depth exceeds 4: {}",
            longest.join(" -> ")
        ));
    }
    Ok((longest.len(), longest))
}
fn tokens(v: &Value) -> Value {
    match v {
        Value::Atom(s) => Value::Atom(s.clone()),
        Value::Block(es) => Value::Block(
            es.iter()
                .map(|e| Entry {
                    key: e.key.clone(),
                    op: e.op.clone(),
                    value: tokens(&e.value),
                    span: 0..0,
                })
                .collect(),
        ),
    }
}
fn sequence(raw: &str, entries: &[Entry], budget: usize) -> Result<Vec<String>> {
    let mut groups: Vec<(String, usize)> = Vec::new();
    let mut previous = "";
    for e in entries {
        let k = e.key.as_deref().ok_or("Unkeyed generated statement")?;
        let fragment = raw[e.span.clone()].to_owned();
        let c = 1 + cost(&e.value);
        if k == "else" || k == "else_if" {
            if !["if", "else_if"].contains(&previous) {
                return Err("Orphan generated else chain".into());
            }
            let group = groups.last_mut().unwrap();
            group.0.push(' ');
            group.0.push_str(&fragment);
            group.1 += c;
        } else {
            groups.push((fragment, c));
        }
        previous = k;
    }
    let mut out = Vec::new();
    let mut text = String::new();
    let mut size = 2;
    for (fragment, c) in groups {
        if c + 2 > budget {
            return Err("One indivisible generated statement exceeds parser budget".into());
        }
        if size + c > budget && !text.is_empty() {
            out.push(std::mem::take(&mut text));
            size = 2;
        }
        text.push_str(&fragment);
        text.push('\n');
        size += c;
    }
    if !text.is_empty() {
        out.push(text);
    }
    Ok(out)
}
fn context_sensitive(v: &Value) -> bool {
    match v {
        Value::Atom(s) => {
            s.contains('$')
                || s.starts_with('@')
                || [
                    "root", "from", "prev", "prevprev", "fromfrom", "break", "return",
                ]
                .contains(&s.to_lowercase().as_str())
        }
        Value::Block(es) => es.iter().any(|e| {
            e.key.as_ref().is_some_and(|k| {
                [
                    "root", "from", "prev", "prevprev", "fromfrom", "break", "return",
                ]
                .contains(&k.to_lowercase().as_str())
            }) || context_sensitive(&e.value)
        }),
    }
}
pub fn effects(raw: &str, identity: &str) -> Result<(String, usize)> {
    let parsed = script::parse(raw)?;
    let mut result = Vec::new();
    let mut chunks = 0;
    for entry in parsed.entries() {
        if cost(&entry.value) + 1 <= EFFECT_BUDGET {
            result.push(raw[entry.span.clone()].to_owned());
            continue;
        }
        let key = entry.key.as_ref().ok_or("Generated effect missing name")?;
        if !key.starts_with("tte_") || context_sensitive(&entry.value) {
            return Err(format!(
                "Cannot safely delegate context-sensitive effect {key}"
            ));
        }
        let parts = sequence(raw, entry.value.entries(), EFFECT_BUDGET)?;
        let prefix = format!(
            "tte_native_chunk_{}",
            &sha(format!("{identity}:{key}").as_bytes())[..16]
        );
        let mut calls = Vec::new();
        let mut restored = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            let name = format!("{prefix}_{i}");
            if raw.contains(&name) {
                return Err("Chunk name collision".into());
            }
            calls.push(format!("{name}=yes"));
            result.push(format!("{name}={{\n{part}\n}}"));
            restored.extend(script::parse(part)?.entries().to_vec());
            chunks += 1;
        }
        if tokens(&Value::Block(restored)) != tokens(&entry.value) {
            return Err("Generated effect inverse AST proof failed".into());
        }
        result.push(format!("{key}={{ {} }}", calls.join(" ")));
    }
    Ok((result.join("\n"), chunks))
}
pub fn hidden_events(raw: &str) -> Result<(String, Vec<String>)> {
    let parsed = script::parse(raw)?;
    let mut edits = Vec::new();
    let mut effects_out = Vec::new();
    for event in parsed.entries() {
        if event.key.as_deref() != Some("country_event") {
            continue;
        }
        let Some(imm) = event
            .value
            .entries()
            .iter()
            .find(|e| e.key.as_deref() == Some("immediate"))
        else {
            continue;
        };
        if cost(&imm.value) < EFFECT_BUDGET {
            continue;
        }
        if event.value.scalar("hide_window") != Some("yes") || context_sensitive(&imm.value) {
            return Err("Oversized event is not a context-safe hidden country event".into());
        }
        let id = event.value.scalar("id").ok_or("Hidden event ID missing")?;
        let mut calls = Vec::new();
        let mut restored = Vec::new();
        for (i, part) in sequence(raw, imm.value.entries(), EFFECT_BUDGET)?
            .into_iter()
            .enumerate()
        {
            let name = format!("tte_native_event_{}_{}", &sha(id.as_bytes())[..16], i);
            calls.push(format!("{name}=yes"));
            effects_out.push(format!("{name}={{\n{part}\n}}"));
            restored.extend(script::parse(&part)?.entries().to_vec());
        }
        if tokens(&Value::Block(restored)) != tokens(&imm.value) {
            return Err("Hidden event inverse AST proof failed".into());
        }
        edits.push((
            imm.span.start,
            imm.span.end,
            format!("immediate={{ {} }}", calls.join(" ")),
        ));
    }
    Ok((crate::gui::edits(raw, edits)?, effects_out))
}
pub fn files(folder: &str, objects: &[String]) -> Result<BTreeMap<String, Vec<u8>>> {
    files_with_preamble(folder, objects, "")
}
pub fn files_with_preamble(
    folder: &str,
    objects: &[String],
    preamble: &str,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    let prefix_cost = cost(&script::parse(preamble)?);
    let mut pending = preamble.to_owned();
    let mut size = prefix_cost;
    let mut used = false;
    let mut i = 0;
    for object in objects {
        let parsed = script::parse(object)?;
        let count = cost(&parsed);
        if count + prefix_cost > FILE_BUDGET {
            return Err(format!(
                "Object in {folder} exceeds parser budget ({count})"
            ));
        }
        if size + count > FILE_BUDGET && used {
            out.insert(
                format!("{folder}/tte_native_{i:04}.txt"),
                std::mem::take(&mut pending).into_bytes(),
            );
            pending = preamble.to_owned();
            size = prefix_cost;
            i += 1;
        }
        pending.push_str(object);
        pending.push_str("\n\n");
        size += count;
        used = true;
    }
    if used {
        out.insert(
            format!("{folder}/tte_native_{i:04}.txt"),
            pending.into_bytes(),
        );
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_wide_dispatchers_require_an_event_boundary() {
        let body = "set_country_flag=owned ".repeat(9000);
        let roots = vec![format!(
            "country_event={{id=owned.1 option={{ {} }} }}",
            "tte_big=yes ".repeat(100)
        )];
        assert!(eager_effect_guard(&format!("tte_big={{ {body} }}"), &roots).is_err());
        assert!(eager_effect_guard("tte_big={country_event={id=owned.2}}", &roots).is_ok());
    }
    #[test]
    fn if_else_remain_indivisible() {
        let s =
            "if={limit={always=yes} x=yes} else_if={limit={always=no} y=yes} else={z=yes} a=yes";
        let p = script::parse(s).unwrap();
        let parts = sequence(s, p.entries(), 30).unwrap();
        assert!(parts[0].contains("else={z=yes}"));
        assert!(sequence(s, p.entries(), 5).is_err());
    }
    #[test]
    fn oversized_effect_has_inverse_proof() {
        let s = format!(
            "tte_many={{ {} }}",
            (0..12000)
                .map(|i| format!("set_country_flag=x_{i}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let (v, n) = effects(&s, "owned").unwrap();
        assert!(n > 1);
        for e in script::parse(&v).unwrap().entries() {
            assert!(cost(&e.value) <= EFFECT_BUDGET);
        }
    }
    #[test]
    fn contextual_frame_is_not_silently_changed() {
        let s = format!("tte_many={{ root={{a=yes}} {} }}", "x=yes ".repeat(12000));
        assert!(effects(&s, "owned").is_err());
    }
}
