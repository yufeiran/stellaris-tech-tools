//! Player explanations use the same native selection policy. They describe the
//! current role, never claim to be an engine decision log or a complete draw model.
use crate::{
    planning::{self, t, Tech, AREAS, MAX_GOALS},
    script::{self, Result, Value},
    sources::Sources,
};
use std::collections::BTreeMap;

pub struct Diagnostics {
    pub effects: Vec<String>,
    pub getters: Vec<String>,
    pub localisation: BTreeMap<String, BTreeMap<String, String>>,
}
pub fn compile(techs: &[Tech], sources: &mut Sources) -> Result<Diagnostics> {
    let mut limits: BTreeMap<i64, Vec<Option<i64>>> = BTreeMap::new();
    let mut dynamic = false;
    let files: Vec<_> = sources
        .files
        .iter()
        .filter(|(key, _)| key.starts_with("common/technology/tier/") && key.ends_with(".txt"))
        .map(|(_, f)| f.clone())
        .collect();
    for file in files {
        let raw = sources.expand(&file.raw, &[])?;
        let parsed = script::parse(&raw)?;
        for entry in parsed.entries() {
            if let Some(tier) = entry.key.as_deref().and_then(|s| s.parse::<i64>().ok()) {
                let values: Vec<_> = entry.value.all("previously_unlocked").collect();
                limits.entry(tier).or_default().push(if values.len() == 1 {
                    values[0]
                        .atom()
                        .and_then(|s| s.parse::<i64>().ok())
                        .filter(|x| *x >= 0)
                } else {
                    None
                });
            } else if !entry.key.as_deref().is_some_and(|s| s.starts_with('@')) {
                dynamic = true;
            }
        }
    }
    let max = techs
        .iter()
        .filter(|t| t.tier_defined)
        .map(|t| t.tier)
        .max()
        .unwrap_or(5)
        .max(5);
    let mut out = Diagnostics {
        effects: Vec::new(),
        getters: Vec::new(),
        localisation: BTreeMap::from([
            ("english".into(), BTreeMap::new()),
            ("simp_chinese".into(), BTreeMap::new()),
        ]),
    };
    let mut script = Vec::new();
    let mut names: BTreeMap<&str, Vec<String>> =
        AREAS.into_iter().map(|a| (a, Vec::new())).collect();
    for area in AREAS {
        for role in ["goal", "route", "filler", "outside"] {
            script.push(t!(
                "remove_country_flag=tte_explain_@role@_@area@",
                role = role,
                area = area
            ));
        }
        script.push(t!("set_variable={which=tte_explain_slot_@area@ value=0} clear_variable=tte_explain_limit_@area@",area=area));
        for tier in 1..max {
            script.push(t!(
                "set_variable={which=tte_count_@area@_@tier@ value=0}",
                area = area,
                tier = tier
            ));
        }
    }
    for tech in techs {
        for (area, rule) in tech.areas()? {
            if (1..max).contains(&tech.tier) {
                script.push(t!("if={limit={has_technology=@key@ @rule@} change_variable={which=tte_count_@area@_@tier@ value=1}}",key=tech.id,rule=rule,area=area,tier=tech.tier));
            }
            names.get_mut(area.as_str()).unwrap().push(t!(
                "text={trigger={is_researching_technology=@key@ @rule@} localization_key=@key@}",
                key = tech.id,
                rule = rule
            ));
        }
    }
    for tech in techs {
        for (area, rule) in tech.areas()? {
            let rank=(1..=MAX_GOALS).map(|slot|t!("@op@={limit={OR={AND={tte_priority_@key@=yes check_variable={which=tte_route_rank_@key@ value=@slot@}} AND={tte_gate_candidate_@key@=yes check_variable={which=tte_fill_rank_@area@_@tier@ value=@slot@}}}} set_variable={which=tte_explain_slot_@area@ value=@slot@}}",op=if slot==1{"if"}else{"else_if"},key=tech.id,slot=slot,area=area,tier=tech.tier)).collect::<Vec<_>>().join(" ");
            let earlier=planning::combine("OR",(1..MAX_GOALS).map(|slot|{
            let previous=(1..=slot).map(|rank|t!("check_variable={which=tte_route_rank_@key@ value=@rank@}",key=tech.id,rank=rank)).collect::<Vec<_>>().join(" ");
            t!("AND={tte_priority_@key@=yes tte_gate_candidate_@key@=yes check_variable={which=tte_fill_rank_@area@_@tier@ value=@slot@} NOT={OR={@previous@}}}",key=tech.id,area=area,tier=tech.tier,slot=slot,previous=previous)
        }).collect());
            let role=t!("if={limit={@earlier@} set_country_flag=tte_explain_filler_@area@} else_if={limit={has_country_flag=tte_goal_@key@} set_country_flag=tte_explain_goal_@area@} else_if={limit={has_country_flag=tte_route_@key@} set_country_flag=tte_explain_route_@area@} else_if={limit={tte_gate_candidate_@key@=yes} set_country_flag=tte_explain_filler_@area@} else={set_country_flag=tte_explain_outside_@area@}",earlier=earlier,key=tech.id,area=area);
            let threshold = limits
                .get(&(tech.tier + 1))
                .filter(|values| {
                    !dynamic && !values.is_empty() && values.iter().all(|v| v == &values[0])
                })
                .and_then(|v| v[0]);
            let denominator = threshold
                .map(|n| {
                    t!(
                        "set_variable={which=tte_explain_limit_@area@ value=@n@}",
                        area = area,
                        n = n
                    )
                })
                .unwrap_or_else(|| t!("clear_variable=tte_explain_limit_@area@", area = area));
            script.push(t!("if={limit={is_researching_technology=@key@ @rule@} @rank@ @role@ set_variable={which=tte_explain_tier_@area@ value=@tier@} set_variable={which=tte_explain_next_@area@ value=@next@} set_variable={which=tte_explain_count_@area@ value=@count@} @denominator@}",key=tech.id,rule=rule,rank=rank,role=role,area=area,tier=tech.tier,next=tech.tier+1,count=if (1..max).contains(&tech.tier){format!("tte_count_{area}_{}",tech.tier)}else{"0".into()},denominator=denominator));
        }
    }
    let mut loc = |key: String, zh: String, en: String| {
        out.localisation
            .get_mut("english")
            .unwrap()
            .insert(key.clone(), en);
        out.localisation
            .get_mut("simp_chinese")
            .unwrap()
            .insert(key, zh);
    };
    for slot in 1..=MAX_GOALS {
        loc(
            format!("tte_plan_slot_{slot}"),
            format!("目标 {slot} · [Root.GetTTEGoal{slot}]"),
            format!("Goal {slot} · [Root.GetTTEGoal{slot}]"),
        );
    }
    for area in AREAS {
        out.getters.push(t!(
            "defined_text={name=GetTTECurrent_@area@ random=no @names@ default=tte_no_research}",
            area = area,
            names = names[area].join(" ")
        ));
        let slots=(1..=MAX_GOALS).map(|slot|t!("text={trigger={check_variable={which=tte_explain_slot_@area@ value=@slot@}} localization_key=tte_plan_slot_@slot@}",area=area,slot=slot)).collect::<Vec<_>>().join(" ");
        out.getters.push(t!(
            "defined_text={name=GetTTEPlanGoal_@area@ random=no @slots@ default=tte_plan_no_goal}",
            area = area,
            slots = slots
        ));
        let mut texts=vec!["text={trigger={NOT={has_country_flag=tte_automation}} localization_key=tte_plan_paused}".into()];
        for (role,zh,en) in [
            ("goal","这是已追踪的目标科技。".into(),"This is a tracked goal.".into()),
            ("route","这是目标路线中的前置科技。".into(),"This is a prerequisite on a goal route.".into()),
            ("filler",format!("补足 T[Root.tte_explain_tier_{area}]，解锁 T[Root.tte_explain_next_{area}]；已统计 [Root.tte_explain_count_{area}]/[Root.tte_explain_limit_{area}]。"),format!("Fill T[Root.tte_explain_tier_{area}] toward T[Root.tte_explain_next_{area}]; counted [Root.tte_explain_count_{area}]/[Root.tte_explain_limit_{area}].")),
            ("outside","当前不属于目标路线；可能是原版或手动选择。".into(),"Not on the current plan; vanilla or manual selection may apply.".into())]{
            let label=format!("tte_plan_{role}_{area}");loc(label.clone(),zh,en);
            texts.push(t!("text={trigger={has_country_flag=tte_explain_@role@_@area@ @known@} localization_key=@label@}",role=role,area=area,known=if role=="filler"{format!("is_variable_set=tte_explain_limit_{area}")}else{String::new()},label=label));
            if role=="filler"{
                let label=format!("tte_plan_filler_unknown_{area}");loc(label.clone(),format!("补足 T[Root.tte_explain_tier_{area}]，解锁 T[Root.tte_explain_next_{area}]；层级门槛由游戏判定。"),format!("Fill T[Root.tte_explain_tier_{area}] toward T[Root.tte_explain_next_{area}]; tier requirement is determined by the game."));
                texts.push(t!("text={trigger={has_country_flag=tte_explain_filler_@area@ NOT={is_variable_set=tte_explain_limit_@area@}} localization_key=@label@}",area=area,label=label));
            }
        }
        out.getters.push(t!(
            "defined_text={name=GetTTEPlanRole_@area@ random=no @texts@ default=tte_no_research}",
            area = area,
            texts = texts.join(" ")
        ));
    }
    out.effects
        .push(format!("tte_explain_plan={{\n{}\n}}", script.join("\n")));
    Ok(out)
}

pub fn readiness(tech: &Tech) -> Result<Vec<(String, String)>> {
    let k = &tech.id;
    if !tech.tier_defined {
        return Ok(vec![(planning::YES.into(), "tte_tier_unknown".into())]);
    }
    let mut rows = vec![
        (
            format!("has_country_flag=tte_viability_ready NOT={{tte_plan_possible_{k}=yes}}"),
            "tte_unreachable".into(),
        ),
        (
            format!("NOT={{can_research_technology={k}}}"),
            "tte_need_conditions".into(),
        ),
        (
            format!(
                "NOT={{ {} }}",
                planning::condition(&tech.condition, None, false, false)
            ),
            "tte_need_prerequisites".into(),
        ),
    ];
    // Special gates stay source-driven. Never assume a technology with a
    // familiar ID still uses vanilla perks in an overhaul.
    let tiers = tech
        .areas()?
        .into_iter()
        .map(|(a, r)| {
            t!(
                "AND={@rule@ NOT={can_research_tier={tier=@tier@ area=@area@}}}",
                rule = r,
                tier = tech.tier,
                area = a
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    rows.push((format!("OR={{ {tiers} }}"), "tte_need_tier".into()));
    if tech.block.all("weight_modifier").count() == 1
        && tech
            .block
            .first("weight_modifier")
            .is_some_and(|b| b.scalar("factor").is_some_and(|s| s == "0" || s == "0.0"))
    {
        rows.push((planning::YES.into(), "tte_need_special".into()));
    }
    rows.push((planning::YES.into(), "tte_wait_draw".into()));
    Ok(rows)
}

pub fn potential_triggers(sources: &mut Sources) -> Result<BTreeMap<String, Value>> {
    let files: Vec<_> = sources
        .files
        .iter()
        .filter(|(key, _)| key.starts_with("common/scripted_triggers/") && key.ends_with(".txt"))
        .map(|(_, f)| f.clone())
        .collect();
    let mut out = BTreeMap::new();
    let mut ambiguous = std::collections::BTreeSet::new();
    for file in files {
        let raw = match sources.expand(&file.raw, &[]) {
            Ok(raw) => raw,
            Err(error) => {
                sources.warnings.push(serde_json::json!({"issue":"unanalysed_named_trigger_file","file":file.relative,"reason":error,"handling":"unknown; original engine conditions remain authoritative"}));
                continue;
            }
        };
        for e in script::parse(&raw)?.entries() {
            if let (Some(k), Value::Block(_)) = (&e.key, &e.value) {
                let body = &raw[e.span.clone()];
                if body.contains("[[") || body.contains('$') {
                    out.remove(k);
                    ambiguous.insert(k.clone());
                    sources.warnings.push(serde_json::json!({"issue":"parameterized_potential_trigger","file":file.relative,"name":k,"handling":"unknown; never permanently block a goal"}));
                    continue;
                }
                if out.contains_key(k) || ambiguous.contains(k) {
                    out.remove(k);
                    ambiguous.insert(k.clone());
                    sources.warnings.push(serde_json::json!({"issue":"ambiguous_potential_trigger","name":k,"handling":"unknown; never permanently block a goal"}));
                } else {
                    out.insert(k.clone(), e.value.clone());
                }
            }
        }
    }
    Ok(out)
}
