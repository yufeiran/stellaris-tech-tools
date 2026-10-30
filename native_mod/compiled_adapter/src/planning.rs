//! Native equivalents of the accepted planner's script semantics. The engine
//! remains responsible for draws/eligibility; this code never grants research.
use crate::{
    catalog,
    script::{Entry, Result, Value},
};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};

pub const AREAS: [&str; 3] = ["physics", "society", "engineering"];
pub const MAX_GOALS: usize = 5;
pub const YES: &str = "always = yes";
pub const NO: &str = "always = no";

pub fn template(raw: &str, values: &[(&str, String)]) -> String {
    // Replace trusted template markers exactly once. Inserted source strings
    // must never be interpreted as further markers.
    let mut out = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find('@') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        if let Some(end) = tail.find('@') {
            let key = &tail[..end];
            if let Some((_, value)) = values.iter().find(|(name, _)| *name == key) {
                out.push_str(value);
            } else {
                out.push_str(&rest[start..start + end + 2]);
            }
            rest = &tail[end + 1..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}
macro_rules! t {
    ($text:expr $(, $name:ident=$value:expr)* $(,)?) => {
        $crate::planning::template($text,&[$((stringify!($name),($value).to_string())),*])
    };
}
pub(crate) use t;

fn from_data(data: &Json) -> Result<Value> {
    if let Some(s) = data.as_str() {
        return Ok(Value::Atom(s.into()));
    }
    let items = data["items"]
        .as_array()
        .ok_or("Invalid source block metadata")?;
    let mut entries = Vec::new();
    for item in items {
        let (key, op, value) = if let Some(key) = item.get("key").and_then(Json::as_str) {
            (
                Some(key.into()),
                item.get("operator")
                    .and_then(Json::as_str)
                    .unwrap_or("=")
                    .to_owned(),
                from_data(&item["value"])?,
            )
        } else {
            (None, String::new(), from_data(item)?)
        };
        entries.push(Entry {
            key,
            op,
            value,
            span: 0..0,
        });
    }
    Ok(Value::Block(entries))
}
fn resolved_variables(v: Value, variables: &Json, stack: &[String]) -> Result<Value> {
    match v {
        Value::Atom(s) if s.starts_with('@') => {
            if stack.len() >= 64 || stack.contains(&s) {
                return Err(format!("Cyclic/deep scripted variable: {s}"));
            }
            let Some(value) = variables.get(&s).and_then(Json::as_str) else {
                return Err(format!(
                    "Unresolved scripted variable in generated planning rule: {s}"
                ));
            };
            let mut stack = stack.to_vec();
            stack.push(s);
            resolved_variables(Value::Atom(value.into()), variables, &stack)
        }
        Value::Block(es) => Ok(Value::Block(
            es.into_iter()
                .map(|mut e| {
                    e.value = resolved_variables(e.value, variables, stack)?;
                    Ok(e)
                })
                .collect::<Result<_>>()?,
        )),
        _ => Ok(v),
    }
}
pub fn atom(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.:@+-/".contains(c))
    {
        s.into()
    } else {
        serde_json::to_string(s).unwrap()
    }
}
pub fn render(value: &Value) -> String {
    match value {
        Value::Atom(s) => atom(s),
        Value::Block(entries) => entries
            .iter()
            .map(|e| {
                let value = if matches!(e.value, Value::Block(_)) {
                    format!("{{ {} }}", render(&e.value))
                } else {
                    render(&e.value)
                };
                match &e.key {
                    Some(k) => format!("{} {} {value}", atom(k), e.op),
                    None => value,
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}
pub fn combine(op: &str, parts: Vec<String>) -> String {
    let (neutral, absorbing) = if op == "AND" { (YES, NO) } else { (NO, YES) };
    if parts.iter().any(|s| s == absorbing) {
        return absorbing.into();
    }
    let mut seen = BTreeSet::new();
    let parts: Vec<_> = parts
        .into_iter()
        .filter(|s| s != neutral && seen.insert(s.clone()))
        .collect();
    match parts.len() {
        0 => neutral.into(),
        1 => parts[0].clone(),
        _ => format!("{op} = {{ {} }}", parts.join(" ")),
    }
}
pub fn negate(value: &str) -> String {
    if value == YES {
        NO.into()
    } else if value == NO {
        YES.into()
    } else {
        format!("NOT = {{ {value} }}")
    }
}
pub fn condition(expr: &Json, substitute: Option<&str>, future: bool, compact: bool) -> String {
    let Some(list) = expr.as_array() else {
        return if expr == false { NO } else { YES }.into();
    };
    if list[0] == "tech" {
        let id = list[1].as_str().unwrap();
        if substitute == Some(id) {
            return YES.into();
        }
        return if future {
            format!("OR = {{ has_technology = {id} has_country_flag = tte_possible_{id} }}")
        } else {
            format!("has_technology = {id}")
        };
    }
    if list[0] == "unknown" {
        return if future { YES } else { NO }.into();
    }
    let op = if list[0] == "all" { "AND" } else { "OR" };
    let parts = list[1..]
        .iter()
        .map(|v| condition(v, substitute, future, compact))
        .collect::<Vec<_>>();
    if compact {
        combine(op, parts)
    } else {
        format!("{op} = {{ {} }}", parts.join(" "))
    }
}
pub fn route_calls(expr: &Json) -> String {
    let Some(list) = expr.as_array() else {
        return String::new();
    };
    if list[0] == "tech" {
        return format!("tte_visit_{} = yes", list[1].as_str().unwrap());
    }
    if list[0] == "unknown" {
        return String::new();
    }
    let calls = list[1..]
        .iter()
        .map(route_calls)
        .collect::<Vec<_>>()
        .join("\n");
    if list[0] == "any" {
        format!(
            "if = {{ limit = {{ NOT = {{ {} }} }}\n{calls}\n}}",
            condition(expr, None, false, false)
        )
    } else {
        calls
    }
}

#[derive(Clone)]
pub struct Tech {
    pub id: String,
    pub area: String,
    pub tier: i64,
    pub cost: f64,
    pub repeatable: bool,
    pub condition: Json,
    pub block: Value,
    pub tier_defined: bool,
}
impl Tech {
    pub fn pending(&self) -> String {
        let missing = format!("NOT = {{ has_technology = {} }}", self.id);
        if self.repeatable {
            format!(
                "OR = {{ {missing} has_country_flag = tte_goal_{} }}",
                self.id
            )
        } else {
            missing
        }
    }
    pub fn completed(&self) -> String {
        if self.repeatable {
            NO.into()
        } else {
            format!("has_technology = {}", self.id)
        }
    }
    pub fn addable(&self) -> String {
        if self.repeatable {
            YES.into()
        } else {
            format!("NOT = {{ has_technology = {} }}", self.id)
        }
    }
    pub fn state(&self, status: &str) -> String {
        let key = &self.id;
        match (self.repeatable,status) {
            (false,"done")=>format!("has_technology = {key}"),
            (false,"busy")=>format!("NOT = {{ has_technology = {key} }} is_researching_technology = {key}"),
            (false,"offered")=>format!("NOR = {{ has_technology = {key} is_researching_technology = {key} }} has_tech_option = {key}"),
            (true,"done")=>format!("has_technology = {key} NOR = {{ is_researching_technology = {key} has_tech_option = {key} }}"),
            (true,"busy")=>format!("is_researching_technology = {key}"),
            (true,"offered")=>format!("NOT = {{ is_researching_technology = {key} }} has_tech_option = {key}"),
            (_,"locked")=>format!("NOR = {{ has_technology = {key} is_researching_technology = {key} has_tech_option = {key} }}"),
            _=>panic!("Internal unsupported state"),
        }
    }
    pub fn areas(&self) -> Result<Vec<(String, String)>> {
        let swaps: Vec<_> = self.block.all("technology_swap").collect();
        if !swaps.iter().any(|s| s.scalar("area").is_some()) {
            return Ok(vec![(self.area.clone(), YES.into())]);
        }
        let mut branches: BTreeMap<String, Vec<String>> =
            AREAS.into_iter().map(|a| (a.into(), Vec::new())).collect();
        let mut previous = Vec::new();
        for swap in swaps {
            if !matches!(swap, Value::Block(_)) {
                continue;
            }
            fn relative_scope(v: &Value) -> bool {
                match v {
                    Value::Atom(s) => ["root", "from", "prev", "fromfrom", "prevprev"]
                        .contains(&s.split('.').next().unwrap_or("").to_lowercase().as_str()),
                    Value::Block(es) => es.iter().any(|e| {
                        e.key.as_ref().is_some_and(|k| {
                            ["root", "from", "prev", "fromfrom", "prevprev"].contains(
                                &k.split('.').next().unwrap_or("").to_lowercase().as_str(),
                            )
                        }) || relative_scope(&e.value)
                    }),
                }
            }
            if swap.first("trigger").is_some_and(relative_scope) {
                return Err(format!(
                    "{}: scope-relative swap rule needs engine-specific validation",
                    self.id
                ));
            }
            let trigger = swap
                .first("trigger")
                .map(render)
                .unwrap_or_else(|| YES.into());
            let branch = format!(
                "AND = {{ {trigger} {} }}",
                if previous.is_empty() {
                    String::new()
                } else {
                    format!("NOT = {{ OR = {{ {} }} }}", previous.join(" "))
                }
            );
            let area = swap.scalar("area").unwrap_or(&self.area);
            branches
                .get_mut(area)
                .ok_or("Unsupported swapped area")?
                .push(branch);
            previous.push(format!("AND = {{ {trigger} }}"));
        }
        branches
            .get_mut(&self.area)
            .unwrap()
            .push(format!("NOT = {{ OR = {{ {} }} }}", previous.join(" ")));
        Ok(AREAS
            .into_iter()
            .filter_map(|a| {
                let parts = branches.remove(a).unwrap();
                (!parts.is_empty()).then(|| (a.into(), format!("OR = {{ {} }}", parts.join(" "))))
            })
            .collect())
    }
}
pub fn technologies(catalog: &Json) -> Result<Vec<Tech>> {
    fn native_refs(expr: &Json, known: &std::collections::BTreeSet<&str>) -> Json {
        if expr.get(0).and_then(Json::as_str) == Some("tech")
            && !known.contains(expr[1].as_str().unwrap())
        {
            return serde_json::json!(["unknown", expr[1]]);
        }
        if let Some(es) = expr.as_array() {
            return Json::Array(
                std::iter::once(es[0].clone())
                    .chain(es[1..].iter().map(|v| native_refs(v, known)))
                    .collect(),
            );
        }
        expr.clone()
    }
    let known = catalog["technologies"]
        .as_object()
        .ok_or("Missing catalog")?
        .keys()
        .map(String::as_str)
        .collect();
    let mut out = Vec::new();
    for (id, row) in catalog["technologies"]
        .as_object()
        .ok_or("Missing catalog")?
    {
        let tier = row["tier"].as_i64().ok_or("Invalid integer tier")?;
        if !(0..=32).contains(&tier) {
            return Err("Native planner supports tiers 0..32".into());
        }
        out.push(Tech {
            id: id.clone(),
            area: row["area"].as_str().ok_or("Invalid area")?.into(),
            tier,
            cost: row["cost"].as_f64().ok_or("Invalid cost")?,
            repeatable: row["repeatable"].as_bool().unwrap_or(false),
            condition: row
                .get("originalCondition")
                .map(|e| native_refs(e, &known))
                .unwrap_or_else(|| row["condition"].clone()),
            block: if row.get("scriptedVariables").is_some() {
                resolved_variables(
                    from_data(&row["rawDefinitionFields"])?,
                    &row["scriptedVariables"],
                    &[],
                )?
            } else {
                from_data(&row["rawDefinitionFields"])?
            },
            tier_defined: row["tierDefined"].as_bool().unwrap_or(true),
        });
    }
    Ok(out)
}

pub fn potential_bounds(
    value: Option<&Value>,
    triggers: &BTreeMap<String, Value>,
    stack: &[String],
) -> (String, String) {
    let mut budget = 4096usize;
    bounds_budget(value, triggers, stack, 0, &mut budget)
}
fn bounds_budget(
    value: Option<&Value>,
    triggers: &BTreeMap<String, Value>,
    stack: &[String],
    depth: usize,
    budget: &mut usize,
) -> (String, String) {
    if depth >= 64 || *budget == 0 {
        return (YES.into(), YES.into());
    }
    *budget -= 1;
    let Some(Value::Block(entries)) = value else {
        return (YES.into(), YES.into());
    };
    fn parameter_syntax(v: &Value) -> bool {
        match v {
            Value::Atom(s) => s.contains("[[") || s.contains(']'),
            Value::Block(es) => es.iter().any(|e| {
                e.key
                    .as_deref()
                    .is_none_or(|k| k.contains('$') || k.contains('[') || k.contains(']'))
                    || parameter_syntax(&e.value)
            }),
        }
    }
    // A preprocessor branch can change which surrounding terms exist. Do not
    // AND both branches merely because the tokenizer retained their atoms.
    if parameter_syntax(value.unwrap()) {
        return (YES.into(), YES.into());
    }
    let mut values = Vec::new();
    for entry in entries {
        let Some(key) = entry.key.as_deref() else {
            values.push((YES.into(), YES.into()));
            continue;
        };
        let op = key.to_uppercase();
        let v = &entry.value;
        let pair = if matches!(v, Value::Block(_))
            && ["AND", "OR", "NOT", "NOR", "NAND"].contains(&op.as_str())
        {
            if op == "NOT" && v.entries().len() != 1 {
                (YES.into(), YES.into())
            } else {
                let children: Vec<_> = v
                    .entries()
                    .iter()
                    .map(|c| {
                        bounds_budget(
                            Some(&Value::Block(vec![c.clone()])),
                            triggers,
                            stack,
                            depth + 1,
                            budget,
                        )
                    })
                    .collect();
                let and = ["AND", "NAND", "NOT"].contains(&op.as_str());
                let yes = combine(
                    if and { "AND" } else { "OR" },
                    children.iter().map(|p| p.0.clone()).collect(),
                );
                let no = combine(
                    if and { "OR" } else { "AND" },
                    children.iter().map(|p| p.1.clone()).collect(),
                );
                if ["NOT", "NOR", "NAND"].contains(&op.as_str()) {
                    (no, yes)
                } else {
                    (yes, no)
                }
            }
        } else if entry.op != "=" {
            (YES.into(), YES.into())
        } else if key == "always" && v.atom().is_some_and(|s| s == "yes" || s == "no") {
            if v.atom() == Some("yes") {
                (YES.into(), NO.into())
            } else {
                (NO.into(), YES.into())
            }
        } else if v
            .atom()
            .is_some_and(|s| s.contains('$') || s.starts_with('@'))
        {
            (YES.into(), YES.into())
        } else if ["has_dlc", "host_has_dlc", "has_origin", "is_ai"].contains(&key)
            && v.atom().is_some()
        {
            let check = format!("{key} = {}", render(v));
            (check.clone(), negate(&check))
        } else if ["has_technology", "has_ascension_perk"].contains(&key) && v.atom().is_some() {
            (YES.into(), negate(&format!("{key} = {}", render(v))))
        } else if v.atom().is_some_and(|s| s == "yes" || s == "no")
            && triggers.contains_key(key)
            && stack.len() < 64
            && !stack.iter().any(|s| s == key)
        {
            let mut stack = stack.to_vec();
            stack.push(key.into());
            let pair = bounds_budget(triggers.get(key), triggers, &stack, depth + 1, budget);
            if v.atom() == Some("yes") {
                pair
            } else {
                (pair.1, pair.0)
            }
        } else {
            (YES.into(), YES.into())
        };
        values.push(pair);
    }
    (
        combine("AND", values.iter().map(|p| p.0.clone()).collect()),
        combine("OR", values.into_iter().map(|p| p.1).collect()),
    )
}
pub fn dependency_order(techs: &[Tech]) -> Result<Vec<usize>> {
    let lookup: BTreeMap<_, _> = techs
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();
    let mut state = vec![0u8; techs.len()];
    let mut out = Vec::new();
    for root in 0..techs.len() {
        let mut todo = vec![(root, false)];
        while let Some((i, exit)) = todo.pop() {
            if exit {
                state[i] = 2;
                out.push(i);
                continue;
            }
            if state[i] == 2 {
                continue;
            }
            if state[i] == 1 {
                return Err("Cyclic planning prerequisites".into());
            }
            state[i] = 1;
            todo.push((i, true));
            for parent in catalog::ids(&techs[i].condition).iter().rev() {
                todo.push((
                    *lookup
                        .get(parent.as_str())
                        .ok_or("Missing planner dependency")?,
                    false,
                ));
            }
        }
    }
    Ok(out)
}
pub fn viability(
    techs: &[Tech],
    triggers: &BTreeMap<String, Value>,
) -> Result<(Vec<String>, Vec<String>)> {
    let mut checks = Vec::new();
    let mut refresh = Vec::new();
    for i in dependency_order(techs)? {
        let tech = &techs[i];
        let key = &tech.id;
        let potential = if tech.block.all("potential").count() > 1 {
            YES.into()
        } else if tech.tier_defined {
            potential_bounds(tech.block.first("potential"), triggers, &[]).0
        } else {
            NO.into()
        };
        let parents = condition(&tech.condition, None, true, true);
        checks.push(t!(
            "tte_plan_possible_@key@ = { has_country_flag = tte_possible_@key@ }",
            key = key
        ));
        refresh.push(t!("remove_country_flag = tte_possible_@key@\nif = { limit = { OR = { has_technology = @key@ is_researching_technology = @key@ AND = { has_tech_option = @key@ can_research_technology = @key@ } AND = { @potential@ @parents@ } } } set_country_flag = tte_possible_@key@ }",key=key,potential=potential,parents=parents));
    }
    Ok((
        checks,
        vec![format!(
            "tte_refresh_viability = {{\n{}\nset_country_flag = tte_viability_ready\n}}",
            refresh.join("\n")
        )],
    ))
}
pub fn branch_pressure(techs: &[Tech]) -> BTreeMap<String, String> {
    let mut children: BTreeMap<String, Vec<String>> =
        techs.iter().map(|t| (t.id.clone(), Vec::new())).collect();
    for tech in techs {
        let child = &tech.id;
        for parent in catalog::ids(&tech.condition) {
            let full = condition(&tech.condition, None, false, true);
            let without = condition(&tech.condition, Some(&parent), false, true);
            children.get_mut(&parent).unwrap().push(t!("AND = { NOT = { has_technology = @child@ } has_country_flag = tte_possible_@child@ NOT = { has_country_flag = tte_route_@child@ } NOT = { @full@ } @without@ }",child=child,full=full,without=without));
        }
    }
    children
        .into_iter()
        .map(|(id, parts)| (id, combine("OR", parts)))
        .collect()
}
pub fn filler_score(cost: f64, extra: bool) -> f64 {
    let factor = (10000000.0 / cost.max(1.0)).clamp(10.0, 10000.0);
    (factor / if extra { 200.0 } else { 100.0 }).clamp(10.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_potential_is_not_permanently_unreachable() {
        let block =
            crate::script::parse("NOT={has_country_flag=event_unlock} has_origin=lost_colony")
                .unwrap();
        assert_eq!(
            potential_bounds(Some(&block), &BTreeMap::new(), &[]).0,
            "has_origin = lost_colony"
        );
    }
    #[test]
    fn comparison_operators_survive_source_roundtrip() {
        let source = crate::script::parse("x={check_variable={which=gate value>=3}}").unwrap();
        let data = crate::script::data(&source);
        let roundtrip = from_data(&data).unwrap();
        assert!(render(&roundtrip).contains("value >= 3"));
    }
    #[test]
    fn exclusive_technology_only_blocks_when_already_owned() {
        let block = crate::script::parse("NOT={has_technology=exclusive}").unwrap();
        assert_eq!(
            potential_bounds(Some(&block), &BTreeMap::new(), &[]).0,
            "NOT = { has_technology = exclusive }"
        );
    }
    #[test]
    fn markers_inside_source_data_are_not_reinterpreted() {
        assert_eq!(
            template(
                "@first@ @second@",
                &[("first", "@second@".into()), ("second", "literal".into())]
            ),
            "@second@ literal"
        );
    }

    fn equivalent(expected: &str, actual: &str, label: &str) {
        // Explicit policy delta from the legacy snapshot: engine-offered and
        // currently eligible cards are authoritative. The dedicated test below
        // checks those guards; snapshots still compare every other planner AST.
        fn legacy_remainder(v: &mut Value) {
            if let Value::Block(es) = v {
                es.retain(|e| {
                    if e.key
                        .as_deref()
                        .is_some_and(|k| k.starts_with("tte_offered_"))
                    {
                        return false;
                    }
                    !(e.key.as_deref() == Some("AND")
                        && e.value.entries().len() == 2
                        && e.value.scalar("has_tech_option").is_some()
                        && e.value.scalar("has_tech_option")
                            == e.value.scalar("can_research_technology"))
                });
                for e in es {
                    legacy_remainder(&mut e.value)
                }
            }
        }
        fn compare(a: &Value, b: &Value, path: &str) {
            match (a, b) {
                (Value::Atom(a), Value::Atom(b)) => assert_eq!(a, b, "{path}"),
                (Value::Block(a), Value::Block(b)) => {
                    assert_eq!(a.len(), b.len(), "{path}: child count");
                    for (i, (a, b)) in a.iter().zip(b).enumerate() {
                        assert_eq!(a.key, b.key, "{path}/{i}: key");
                        assert_eq!(a.op, b.op, "{path}/{i}: operator");
                        compare(
                            &a.value,
                            &b.value,
                            &format!("{path}/{i}:{}", a.key.as_deref().unwrap_or("<list>")),
                        );
                    }
                }
                _ => panic!("{path}: scalar/block mismatch"),
            }
        }
        let mut a = crate::script::parse(expected)
            .unwrap_or_else(|e| panic!("{label}: reference parse: {e}"));
        let mut b =
            crate::script::parse(actual).unwrap_or_else(|e| panic!("{label}: native parse: {e}"));
        legacy_remainder(&mut a);
        legacy_remainder(&mut b);
        compare(&a, &b, label);
    }

    #[test]
    fn researchable_engine_offer_overrides_reconstructed_draw_predicates() {
        let reference: Json =
            serde_json::from_str(include_str!("../tests/fixtures/planning-reference.json"))
                .unwrap();
        for case in reference["cases"].as_object().unwrap().values() {
            let mut techs = technologies(&case["catalog"]).unwrap();
            for tech in &mut techs {
                tech.tier_defined = false;
                tech.block = crate::script::parse("potential={always=no}").unwrap();
            }
            let (raw, _) = crate::selection::compile(&techs).unwrap();
            let parsed = crate::script::parse(&raw.join("\n")).unwrap();
            let (_, viable) = viability(&techs, &BTreeMap::new()).unwrap();
            for tech in &techs {
                let offered = parsed.first(&format!("tte_offered_{}", tech.id)).unwrap();
                assert_eq!(offered.scalar("has_tech_option"), Some(tech.id.as_str()));
                assert_eq!(
                    offered.scalar("can_research_technology"),
                    Some(tech.id.as_str())
                );
                assert!(offered.first("OR").is_none());
                assert_eq!(offered.entries().len(), 3);
                assert!(viable.join("\n").contains(&format!(
                    "AND = {{ has_tech_option = {} can_research_technology = {} }}",
                    tech.id, tech.id
                )));
            }
        }
    }

    #[test]
    fn full_selection_viability_and_repeatable_semantics_match_legacy_fixtures() {
        let reference: Json =
            serde_json::from_str(include_str!("../tests/fixtures/planning-reference.json"))
                .unwrap();
        for (name, case) in reference["cases"].as_object().unwrap() {
            let techs = technologies(&case["catalog"]).unwrap();
            let triggers: BTreeMap<_, _> = case["scriptedTriggers"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), from_data(v).unwrap()))
                .collect();
            let actual = crate::selection::compile(&techs).unwrap();
            for (kind, raw) in [("triggers", actual.0), ("effects", actual.1)] {
                // Legacy equivalence remains for arbitration. Offered-card
                // eligibility intentionally changed to engine authority in F.
                let expected = case["selection"][kind]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .filter(|s| !s.trim_start().starts_with("tte_offered_"))
                    .collect::<Vec<_>>()
                    .join("\n");
                equivalent(
                    &expected,
                    &raw.iter()
                        .filter(|s| !s.trim_start().starts_with("tte_offered_"))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n"),
                    &format!("{name}/selection/{kind}"),
                );
            }
            let actual = viability(&techs, &triggers).unwrap();
            for (kind, raw) in [("triggers", actual.0), ("effects", actual.1)] {
                let mut expected = case["viability"][kind]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join("\n");
                // The new offered+eligible OR branch is the only delta from
                // legacy future-reachability bounds on these authored cases.
                if kind == "effects" {
                    for tech in &techs {
                        let old = format!("is_researching_technology = {}", tech.id);
                        let extra = format!(
                            "{old} AND = {{ has_tech_option = {} can_research_technology = {} }}",
                            tech.id, tech.id
                        );
                        expected = expected.replace(&old, &extra);
                    }
                }
                equivalent(
                    &expected,
                    &raw.join("\n"),
                    &format!("{name}/viability/{kind}"),
                );
            }
            for (id, raw) in branch_pressure(&techs) {
                equivalent(
                    case["branchPressure"][&id].as_str().unwrap(),
                    &raw,
                    &format!("{name}/pressure/{id}"),
                );
            }
            for tech in techs {
                let expected = &case["techSemantics"][&tech.id];
                equivalent(
                    expected["pending"].as_str().unwrap(),
                    &tech.pending(),
                    "pending",
                );
                equivalent(
                    expected["completed"].as_str().unwrap(),
                    &tech.completed(),
                    "completed",
                );
                equivalent(
                    expected["addable"].as_str().unwrap(),
                    &tech.addable(),
                    "addable",
                );
                for (area, rule) in tech.areas().unwrap() {
                    equivalent(expected["areas"][area].as_str().unwrap(), &rule, "area");
                }
                for status in ["done", "busy", "offered", "locked"] {
                    equivalent(
                        expected["states"][status].as_str().unwrap(),
                        &tech.state(status),
                        status,
                    );
                }
            }
        }
    }

    #[test]
    fn five_goal_queue_routes_tier_fillers_and_events_match_legacy_fixtures() {
        let reference: Json = serde_json::from_str(include_str!(
            "../tests/fixtures/planning-full-reference.json"
        ))
        .unwrap();
        for (name, case) in reference["cases"].as_object().unwrap() {
            let techs = technologies(&case["catalog"]).unwrap();
            let triggers: BTreeMap<_, _> = case["scriptedTriggers"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), from_data(v).unwrap()))
                .collect();
            let actual = crate::planner_emit::compile(&techs, &triggers).unwrap();
            for (kind, raw) in [
                ("triggers", actual.triggers),
                ("effects", actual.effects),
                ("events", actual.events),
            ] {
                let expected = case["planner"][kind]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join("\n");
                equivalent(
                    &expected,
                    &raw.join("\n"),
                    &format!("{name}/planner/{kind}"),
                );
            }
        }
    }

    #[test]
    fn manual_advice_scripts_gui_and_bilingual_reasons_match_legacy() {
        let reference: Json =
            serde_json::from_str(include_str!("../tests/fixtures/advice-reference.json")).unwrap();
        for (name, case) in reference["cases"].as_object().unwrap() {
            let techs = technologies(&case["catalog"]).unwrap();
            let actual = crate::advice::compile(&techs).unwrap();
            for (kind, raw) in [
                ("triggers", actual.triggers),
                ("effects", actual.effects),
                ("buttons", actual.buttons),
                ("getters", actual.getters),
            ] {
                let expected = case["advice"][kind]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join("\n");
                equivalent(&expected, &raw.join("\n"), &format!("{name}/advice/{kind}"));
            }
            equivalent(
                case["advice"]["gui"].as_str().unwrap(),
                &actual.gui,
                &format!("{name}/advice/gui"),
            );
            assert_eq!(
                serde_json::to_value(actual.localisation).unwrap(),
                case["advice"]["localisation"],
                "{name}/advice/localisation"
            );
        }
    }
}
