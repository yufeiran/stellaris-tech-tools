//! Cached offered-card arbitration. This is a direct native migration of the
//! accepted lexicographic rank/goal/route/filler policy, not a new scheduler.
use crate::{
    planning::{self, t, Tech, AREAS, MAX_GOALS},
    script::Result,
};

pub fn compile(techs: &[Tech]) -> Result<(Vec<String>, Vec<String>)> {
    let mut result = Vec::new();
    let mut scan = Vec::new();
    let pressure = planning::branch_pressure(techs);
    for area in AREAS {
        for rank in 1..=MAX_GOALS {
            scan.push(t!(
                "remove_country_flag = tte_available_@area@_@rank@",
                area = area,
                rank = rank
            ));
        }
    }
    for kind in ["route", "goal"] {
        for area in AREAS {
            for rank in 1..=MAX_GOALS {
                scan.push(t!(
                    "remove_country_flag = tte_available_@kind@_@area@_@rank@",
                    kind = kind,
                    area = area,
                    rank = rank
                ));
            }
        }
    }
    for area in AREAS {
        scan.push(t!(
            "remove_country_flag = tte_available_filler_@area@",
            area = area
        ));
    }
    for tech in techs {
        scan.push(t!(
            "remove_country_flag = tte_extra_branch_@key@",
            key = tech.id
        ));
    }
    for tech in techs {
        scan.push(t!(
            "remove_country_flag = tte_best_filler_@key@",
            key = tech.id
        ));
    }
    for area in AREAS {
        for rank in 1..=MAX_GOALS {
            scan.push(t!(
                "clear_variable = tte_best_fill_score_@area@_@rank@",
                area = area,
                rank = rank
            ));
        }
    }
    for tech in techs {
        let key = &tech.id;
        let tier = tech.tier;
        let areas = tech.areas()?;
        // Actual engine offers may come from events/debris, bypassing ordinary
        // draw predicates. Never contradict a currently researchable card with
        // a reconstructed tier, potential or prerequisite predicate.
        result.push(t!("tte_offered_@key@ = { has_tech_option = @key@ @addable@ can_research_technology = @key@ }",key=key,addable=tech.addable()));
        let mut branches = Vec::new();
        for (area, rule) in &areas {
            let ranks=(1..=MAX_GOALS).map(|rank|t!("if = { limit = { OR = { AND = { tte_priority_@key@ = yes check_variable = { which = tte_route_rank_@key@ value = @rank@ } } AND = { tte_gate_candidate_@key@ = yes check_variable = { which = tte_fill_rank_@area@_@tier@ value = @rank@ } } } } set_country_flag = tte_available_@area@_@rank@ if = { limit = { tte_priority_@key@ = yes } set_country_flag = tte_available_route_@area@_@rank@ if = { limit = { has_country_flag = tte_goal_@key@ } set_country_flag = tte_available_goal_@area@_@rank@ } } }",key=key,rank=rank,area=area,tier=tier)).collect::<Vec<_>>().join(" ");
            branches.push(t!("if = { limit = { @rule@ } @ranks@ if = { limit = { tte_gate_candidate_@key@ = yes } set_country_flag = tte_available_filler_@area@ } }",rule=rule,ranks=ranks,key=key,area=area));
        }
        let mut branch_check=t!("if = { limit = { tte_filler_@key@ = yes @pressure@ } set_country_flag = tte_extra_branch_@key@ }",key=key,pressure=pressure[key]);
        let score = format!("{:.6}", planning::filler_score(tech.cost, false));
        let extra = format!("{:.6}", planning::filler_score(tech.cost, true));
        branch_check.push_str(&t!("if = { limit = { tte_filler_@key@ = yes } set_variable = { which = tte_fill_score_@key@ value = @score@ } if = { limit = { has_country_flag = tte_extra_branch_@key@ } set_variable = { which = tte_fill_score_@key@ value = @extra@ } } ",key=key,score=score,extra=extra));
        for (area, rule) in &areas {
            for rank in 1..=MAX_GOALS {
                branch_check.push_str(&t!("if = { limit = { NOT = { is_researching_technology = @key@ } @rule@ check_variable = { which = tte_fill_rank_@area@_@tier@ value = @rank@ } OR = { NOT = { is_variable_set = tte_best_fill_score_@area@_@rank@ } check_variable = { which = tte_best_fill_score_@area@_@rank@ value < tte_fill_score_@key@ } } } set_variable = { which = tte_best_fill_score_@area@_@rank@ value = tte_fill_score_@key@ } } ",key=key,rule=rule,area=area,tier=tier,rank=rank));
            }
        }
        branch_check.push('}');
        let available = t!(
            "if = { limit = { NOT = { is_researching_technology = @key@ } } @branches@ }",
            key = key,
            branches = branches.join(" ")
        );
        scan.push(t!("if = { limit = { OR = { tte_priority_@key@ = yes tte_gate_candidate_@key@ = yes } tte_offered_@key@ = yes } @available@ @branch_check@ }",key=key,available=available,branch_check=branch_check));
    }
    for tech in techs {
        let key = &tech.id;
        let mut best = Vec::new();
        for (area, rule) in tech.areas()? {
            for rank in 1..=MAX_GOALS {
                best.push(t!("AND = { @rule@ check_variable = { which = tte_fill_rank_@area@_@tier@ value = @rank@ } check_variable = { which = tte_fill_score_@key@ value = tte_best_fill_score_@area@_@rank@ } }",rule=rule,area=area,tier=tech.tier,rank=rank,key=key));
            }
        }
        scan.push(t!("if = { limit = { tte_filler_@key@ = yes tte_offered_@key@ = yes OR = { is_researching_technology = @key@ OR = { @best@ } } } set_country_flag = tte_best_filler_@key@ }",key=key,best=best.join(" ")));
    }
    for area in AREAS {
        for rank in 1..=MAX_GOALS {
            result.push(t!(
                "tte_available_@area@_@rank@ = { has_country_flag = tte_available_@area@_@rank@ }",
                area = area,
                rank = rank
            ));
            for kind in ["route", "goal"] {
                result.push(t!("tte_available_@kind@_@area@_@rank@ = { has_country_flag = tte_available_@kind@_@area@_@rank@ }",kind=kind,area=area,rank=rank));
            }
        }
        let ranks = (1..=MAX_GOALS)
            .map(|rank| {
                t!(
                    "tte_available_@area@_@rank@ = yes",
                    area = area,
                    rank = rank
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        result.push(t!(
            "tte_available_@area@ = { OR = { @ranks@ } }",
            area = area,
            ranks = ranks
        ));
        result.push(t!(
            "tte_available_filler_@area@ = { has_country_flag = tte_available_filler_@area@ }",
            area = area
        ));
    }
    for tech in techs {
        let key = &tech.id;
        let mut branches = Vec::new();
        for (area, rule) in tech.areas()? {
            let mut ranks = Vec::new();
            for rank in 1..=MAX_GOALS {
                let earlier = (1..rank)
                    .map(|i| t!("tte_available_@area@_@i@ = yes", area = area, i = i))
                    .collect::<Vec<_>>()
                    .join(" ");
                let earlier = if earlier.is_empty() {
                    String::new()
                } else {
                    format!("NOR = {{ {earlier} }}")
                };
                ranks.push(t!("AND = { OR = { AND = { tte_priority_@key@ = yes check_variable = { which = tte_route_rank_@key@ value = @rank@ } } AND = { tte_gate_candidate_@key@ = yes check_variable = { which = tte_fill_rank_@area@_@tier@ value = @rank@ } } } @earlier@ OR = { has_country_flag = tte_goal_@key@ NOT = { tte_available_goal_@area@_@rank@ = yes } } OR = { tte_priority_@key@ = yes NOT = { tte_available_route_@area@_@rank@ = yes } } OR = { tte_priority_@key@ = yes has_country_flag = tte_best_filler_@key@ } }",key=key,rank=rank,area=area,tier=tech.tier,earlier=earlier));
            }
            branches.push(t!(
                "AND = { @rule@ OR = { OR = { @ranks@ } NOT = { tte_available_@area@ = yes } } }",
                rule = rule,
                ranks = ranks.join(" "),
                area = area
            ));
        }
        result.push(t!(
            "tte_admissible_@key@ = { OR = { @branches@ } }",
            key = key,
            branches = branches.join(" ")
        ));
    }
    Ok((
        result,
        vec![format!(
            "tte_refresh_selection = {{\n{}\n}}",
            scan.join("\n")
        )],
    ))
}
