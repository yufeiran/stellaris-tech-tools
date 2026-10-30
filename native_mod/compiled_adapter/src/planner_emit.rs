//! Five-goal persistence, rank-aware dependency walks and tier fillers.
use crate::{
    planning::{self, t, Tech, AREAS, MAX_GOALS, NO},
    script::{Result, Value},
};
use std::collections::BTreeMap;

pub struct Compiled {
    pub triggers: Vec<String>,
    pub effects: Vec<String>,
    pub events: Vec<String>,
}
pub fn compile(techs: &[Tech], potential: &BTreeMap<String, Value>) -> Result<Compiled> {
    if techs.len() > 5000 {
        return Err(
            "Native event-ID reservation supports at most 5000 effective technologies".into(),
        );
    }
    let max_tier = techs
        .iter()
        .filter(|t| t.tier_defined)
        .map(|t| t.tier)
        .max()
        .unwrap_or(5)
        .max(5);
    let (mut triggers, mut effects) = planning::viability(techs, potential)?;
    let mut events = vec![
        "namespace = tteroute".into(),
        "namespace = ttetarget".into(),
        "namespace = tteinternal".into(),
    ];
    let mut cache: Vec<_> = techs
        .iter()
        .map(|t| t!("remove_country_flag = tte_route_@key@", key = t.id))
        .collect();
    for area in AREAS {
        for tier in 2..=max_tier {
            cache.push(t!(
                "remove_country_flag = tte_need_@area@_@tier@",
                area = area,
                tier = tier
            ));
        }
    }
    for area in AREAS {
        for tier in 1..max_tier {
            cache.push(t!(
                "remove_country_flag = tte_fill_@area@_@tier@",
                area = area,
                tier = tier
            ));
        }
    }
    for area in AREAS {
        for tier in 1..=max_tier {
            cache.push(t!("clear_variable = tte_need_rank_@area@_@tier@\nclear_variable = tte_fill_rank_@area@_@tier@",area=area,tier=tier));
        }
    }
    let clear_cache = cache.join("\n");
    let clear_slots = (1..=MAX_GOALS)
        .map(|slot| t!("remove_country_flag = tte_slot_@slot@", slot = slot))
        .collect::<Vec<_>>()
        .join("\n");
    let clear_goals = techs
        .iter()
        .map(|t| {
            t!(
                "remove_country_flag = tte_goal_@key@\nclear_variable = tte_rank_@key@",
                key = t.id
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let any_goal = techs
        .iter()
        .map(|t| t!("has_country_flag = tte_goal_@key@", key = t.id))
        .collect::<Vec<_>>()
        .join(" ");
    triggers.push(t!(
        "tte_goal_active = { OR = { @any_goal@ } }",
        any_goal = any_goal
    ));
    let all_slots = (1..=MAX_GOALS)
        .map(|slot| t!("has_country_flag = tte_slot_@slot@", slot = slot))
        .collect::<Vec<_>>()
        .join(" ");
    triggers.push(t!(
        "tte_queue_full = { @all_slots@ }",
        all_slots = all_slots
    ));
    for (index, tech) in techs.iter().enumerate() {
        let key = &tech.id;
        let id = 1000 + index;
        let tier = tech.tier;
        let areas = tech.areas()?;
        triggers.push(t!("tte_vanilla_@key@ = { OR = { is_ai = yes NOT = { has_country_flag = tte_automation } NOR = { tte_priority_@key@ = yes tte_filler_@key@ = yes } } }",key=key));
        triggers.push(t!("tte_priority_@key@ = { is_ai = no has_country_flag = tte_automation has_country_flag = tte_route_@key@ @pending@ }",key=key,pending=tech.pending()));
        let eligible = 1 <= tier
            && tier < max_tier
            && !tech.repeatable
            && tech.block.scalar("is_dangerous") != Some("yes");
        let filler = if eligible {
            let branches=areas.iter().map(|(area,rule)|t!("AND = { @rule@ has_country_flag = tte_fill_@area@_@tier@ NOT = { can_research_tier = { tier = @next@ area = @area@ } } }",rule=rule,area=area,tier=tier,next=tier+1)).collect::<Vec<_>>().join(" ");
            format!("OR = {{ {branches} }}")
        } else {
            NO.into()
        };
        triggers.push(t!("tte_gate_candidate_@key@ = { is_ai = no has_country_flag = tte_automation NOT = { has_technology = @key@ } @filler@ }",key=key,filler=filler));
        triggers.push(t!("tte_filler_@key@ = { tte_gate_candidate_@key@ = yes NOT = { has_country_flag = tte_route_@key@ } }",key=key));
        effects.push(t!(
            "tte_visit_@key@ = { country_event = { id = tteroute.@id@ } }",
            key = key,
            id = id
        ));
        let need = if tier >= 2 && tech.tier_defined {
            areas.iter().map(|(area,rule)|t!("if = { limit = { @rule@ } set_country_flag = tte_need_@area@_@tier@ if = { limit = { NOT = { is_variable_set = tte_need_rank_@area@_@tier@ } } set_variable = { which = tte_need_rank_@area@_@tier@ value = tte_current_rank } } }",rule=rule,area=area,tier=tier)).collect::<Vec<_>>().join("\n")
        } else {
            String::new()
        };
        events.push(t!("country_event = { id = tteroute.@id@ hide_window = yes is_triggered_only = yes immediate = { if = { limit = { tte_plan_possible_@key@ = yes @pending@ NOT = { has_country_flag = tte_route_@key@ } } set_country_flag = tte_route_@key@ set_variable = { which = tte_route_rank_@key@ value = tte_current_rank } @need@\n@parents@\n} } }",id=id,key=key,pending=tech.pending(),need=need,parents=planning::route_calls(&tech.condition)));
        effects.push(t!(
            "tte_target_@key@ = { country_event = { id = ttetarget.@id@ } }",
            key = key,
            id = id
        ));
        let allocation=(1..=MAX_GOALS).map(|slot|t!("@op@ = { limit = { NOT = { has_country_flag = tte_slot_@slot@ } } set_country_flag = tte_slot_@slot@ set_country_flag = tte_goal_@key@ set_variable = { which = tte_rank_@key@ value = @slot@ } set_country_flag = tte_has_goal set_country_flag = tte_automation }",op=if slot==1{"if"}else{"else_if"},slot=slot,key=key)).collect::<Vec<_>>().join("\n");
        events.push(t!("country_event = { id = ttetarget.@id@ hide_window = yes is_triggered_only = yes immediate = { tte_refresh = yes if = { limit = { @addable@ NOT = { has_country_flag = tte_goal_@key@ } } @allocation@ tte_refresh = yes } if = { limit = { has_country_flag = tte_reopen_goal } remove_country_flag = tte_reopen_goal country_event = { id = tte.@id@ } } } }",id=id,key=key,addable=tech.addable(),allocation=allocation));
        effects.push(t!("tte_remove_@key@ = { remove_country_flag = tte_goal_@key@ clear_variable = tte_rank_@key@ tte_refresh = yes }",key=key));
    }
    effects.push("tte_clear_goal = { country_event = { id = tteinternal.2 } }".into());
    events.push(t!("country_event = { id = tteinternal.2 hide_window = yes is_triggered_only = yes immediate = { @clear_goals@ @clear_cache@ @clear_slots@ remove_country_flag = tte_has_goal remove_country_flag = tte_automation if = { limit = { has_country_flag = tte_reopen_clear } remove_country_flag = tte_reopen_clear country_event = { id = tte.1 } } } }",clear_goals=clear_goals,clear_cache=clear_cache,clear_slots=clear_slots));
    let mut refresh = vec![
        "tte_refresh_viability = yes".into(),
        clear_cache,
        clear_slots,
    ];
    for tech in techs {
        refresh.push(t!("if = { limit = { has_country_flag = tte_goal_@key@ } if = { limit = { @done@ } remove_country_flag = tte_goal_@key@ clear_variable = tte_rank_@key@ } else = { if = { limit = { NOT = { is_variable_set = tte_rank_@key@ } } set_variable = { which = tte_rank_@key@ value = 1 } } } }",key=tech.id,done=tech.completed()));
    }
    for slot in 1..=MAX_GOALS {
        refresh.push(t!(
            "set_variable = { which = tte_current_rank value = @slot@ }",
            slot = slot
        ));
        for tech in techs {
            refresh.push(t!("if = { limit = { has_country_flag = tte_goal_@key@ check_variable = { which = tte_rank_@key@ value = @slot@ } } set_country_flag = tte_slot_@slot@ tte_visit_@key@ = yes }",key=tech.id,slot=slot));
        }
    }
    for area in AREAS {
        for gate in 2..=max_tier {
            for rank in 1..=MAX_GOALS {
                let needed=(gate..=max_tier).map(|tier|t!("AND = { is_variable_set = tte_need_rank_@area@_@tier@ check_variable = { which = tte_need_rank_@area@_@tier@ value = @rank@ } }",area=area,tier=tier,rank=rank)).collect::<Vec<_>>().join(" ");
                refresh.push(t!("if = { limit = { NOT = { has_country_flag = tte_fill_@area@_@previous@ } OR = { @needed@ } NOT = { can_research_tier = { tier = @gate@ area = @area@ } } can_research_tier = { tier = @previous@ area = @area@ } } set_country_flag = tte_fill_@area@_@previous@ set_variable = { which = tte_fill_rank_@area@_@previous@ value = @rank@ } }",area=area,previous=gate-1,needed=needed,gate=gate,rank=rank));
            }
        }
    }
    refresh.push("if = { limit = { NOT = { tte_goal_active = yes } } remove_country_flag = tte_has_goal remove_country_flag = tte_automation } else = { set_country_flag = tte_has_goal }".into());
    refresh.push("tte_refresh_selection = yes".into());
    refresh.push("if = { limit = { has_country_flag = tte_automation NOT = { has_country_flag = tte_selection_loop } } set_country_flag = tte_selection_loop country_event = { id = tteinternal.3 days = 1 } }".into());
    effects.push("tte_refresh = { country_event = { id = tteinternal.1 } }".into());
    events.push(t!("country_event = { id = tteinternal.1 hide_window = yes is_triggered_only = yes immediate = {\n@refresh@\n} }",refresh=refresh.join("\n")));
    events.push("country_event = { id = tteinternal.3 hide_window = yes is_triggered_only = yes immediate = { if = { limit = { is_ai = no has_country_flag = tte_automation } tte_refresh_selection = yes country_event = { id = tteinternal.3 days = 1 } } else = { remove_country_flag = tte_selection_loop } } }".into());
    let (selection, scans) = crate::selection::compile(techs)?;
    triggers.extend(selection);
    effects.extend(scans);
    Ok(Compiled {
        triggers,
        effects,
        events,
    })
}
