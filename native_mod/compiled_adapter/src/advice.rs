//! Manual recommendations share the accepted offered-card policy. Buttons only
//! refresh advice; the native research card/gear remains under player control.
use crate::{
    planning::{t, Tech, AREAS, MAX_GOALS},
    script::{self, Result},
};
use serde_json::Value as Json;
use std::collections::BTreeMap;

pub struct Advice {
    pub effects: Vec<String>,
    pub buttons: Vec<String>,
    pub gui: String,
    pub getters: Vec<String>,
    pub triggers: Vec<String>,
    pub localisation: BTreeMap<String, BTreeMap<String, String>>,
}
fn strings(v: &Json) -> Result<Vec<String>> {
    v.as_array()
        .ok_or("Invalid trusted advice asset")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "Invalid advice asset text".into())
        })
        .collect()
}
pub fn compile(techs: &[Tech]) -> Result<Advice> {
    let base: Json = serde_json::from_str(include_str!("../assets/advice-base.json"))
        .map_err(|e| e.to_string())?;
    let mut out = Advice {
        effects: Vec::new(),
        buttons: Vec::new(),
        gui: base["gui"].as_str().ok_or("Missing advice GUI")?.into(),
        getters: strings(&base["getters"])?,
        triggers: Vec::new(),
        localisation: serde_json::from_value(base["localisation"].clone())
            .map_err(|e| e.to_string())?,
    };
    let mut script: Vec<_> = techs
        .iter()
        .map(|tech| t!("remove_country_flag = tte_advice_pick_@key@", key = tech.id))
        .collect();
    for area in AREAS {
        script.push(t!(
            "remove_country_flag = tte_advice_has_@area@",
            area = area
        ));
        for role in ["goal", "route", "filler"] {
            script.push(t!(
                "remove_country_flag = tte_advice_role_@role@_@area@",
                role = role,
                area = area
            ));
        }
        script.push(t!("clear_variable = tte_advice_slot_@area@", area = area));
    }
    let mut ordered: Vec<_> = techs.iter().collect();
    ordered.sort_by(|a, b| a.cost.total_cmp(&b.cost).then(a.id.cmp(&b.id)));
    for tech in ordered {
        let key = &tech.id;
        let tier = tech.tier;
        for (area, rule) in tech.areas()? {
            let ranks=(1..=MAX_GOALS).map(|slot|t!("@op@ = { limit = { OR = { AND = { tte_priority_@key@ = yes check_variable = { which = tte_route_rank_@key@ value = @slot@ } } AND = { tte_gate_candidate_@key@ = yes check_variable = { which = tte_fill_rank_@area@_@tier@ value = @slot@ } } } } set_variable = { which = tte_advice_slot_@area@ value = @slot@ } }",op=if slot==1{"if"}else{"else_if"},key=key,slot=slot,area=area,tier=tier)).collect::<Vec<_>>().join(" ");
            let gates=(1..=MAX_GOALS).map(|slot| {
                let earlier=(1..=slot).map(|rank|t!("check_variable = { which = tte_route_rank_@key@ value = @rank@ }",key=key,rank=rank)).collect::<Vec<_>>().join(" ");
                t!("AND = { tte_gate_candidate_@key@ = yes check_variable = { which = tte_fill_rank_@area@_@tier@ value = @slot@ } NOT = { AND = { tte_priority_@key@ = yes OR = { @earlier@ } } } }",key=key,area=area,tier=tier,slot=slot,earlier=earlier)
            }).collect::<Vec<_>>().join(" ");
            let earlier_gate = format!("OR = {{ {gates} }}");
            let roles=t!("if = { limit = { @earlier_gate@ } set_country_flag = tte_advice_role_filler_@area@ } else_if = { limit = { has_country_flag = tte_goal_@key@ } set_country_flag = tte_advice_role_goal_@area@ } else_if = { limit = { tte_priority_@key@ = yes } set_country_flag = tte_advice_role_route_@area@ } else = { set_country_flag = tte_advice_role_filler_@area@ }",earlier_gate=earlier_gate,area=area,key=key);
            script.push(t!("if = { limit = { is_ai = no has_country_flag = tte_automation NOT = { has_country_flag = tte_advice_has_@area@ } @rule@ NOT = { is_researching_technology = @key@ } tte_offered_@key@ = yes OR = { tte_priority_@key@ = yes tte_gate_candidate_@key@ = yes } tte_admissible_@key@ = yes } set_country_flag = tte_advice_pick_@key@ set_country_flag = tte_advice_has_@area@ @ranks@ @roles@ }",area=area,rule=rule,key=key,ranks=ranks,roles=roles));
        }
    }
    out.effects.push(format!(
        "tte_refresh_advice = {{\n{}\n}}",
        script.join("\n")
    ));
    let base_buttons = strings(&base["buttons"])?;
    out.buttons.push(base_buttons[0].clone());
    let parsed = script::parse(&out.gui)?;
    let root = parsed.entries().first().ok_or("Empty advice GUI")?;
    let mut inserts = Vec::new();
    for (index, area) in AREAS.into_iter().enumerate() {
        let mut live = Vec::new();
        let mut widgets = Vec::new();
        let y = 44 + index * 98;
        for tech in techs {
            if let Some((_, rule)) = tech.areas()?.into_iter().find(|(a, _)| a == area) {
                live.push(t!("AND = { has_country_flag = tte_advice_pick_@key@ @rule@ NOT = { is_researching_technology = @key@ } tte_offered_@key@ = yes }",key=tech.id,rule=rule));
            }
        }
        out.triggers.push(t!("tte_advice_live_@area@ = { is_ai = no has_country_flag = tte_automation OR = { @live@ } }",area=area,live=live.join(" ")));
        out.buttons.push(base_buttons[index + 1].clone());
        for tech in techs {
            let Some((_, rule)) = tech.areas()?.into_iter().find(|(a, _)| a == area) else {
                continue;
            };
            for role in ["goal", "route", "filler"] {
                let name = format!("tte_advice_{area}_{role}_{}", tech.id);
                let role_template = if role == "goal" && tech.repeatable {
                    "repeat"
                } else {
                    role
                };
                for language in ["english", "simp_chinese"] {
                    let loc = &base["roleLocalisation"][role_template][language];
                    for (suffix, kind) in [("", "label"), ("_tip", "tip")] {
                        let raw = loc[kind]
                            .as_str()
                            .ok_or("Missing trusted advice localisation")?;
                        let text = t!(
                            raw,
                            key = tech.id,
                            area = area,
                            tier = tech.tier,
                            next = tech.tier + 1
                        );
                        out.localisation
                            .get_mut(language)
                            .unwrap()
                            .insert(name.clone() + suffix, text);
                    }
                }
                out.buttons.push(t!("@name@ = { potential = { from = { is_ai = no has_country_flag = tte_automation has_country_flag = tte_advice_pick_@key@ has_country_flag = tte_advice_role_@role@_@area@ @rule@ NOT = { is_researching_technology = @key@ } tte_offered_@key@ = yes } } allow = { from = { is_ai = no } } effect = { custom_tooltip = @name@_tip from = { hidden_effect = { tte_refresh = yes } } } }",name=name,key=tech.id,role=role,area=area,rule=rule));
                widgets.push(t!("effectbuttonType = { name = @name@ position = { x = 8 y = @y@ } size = { x = 336 y = 64 } quadTextureSprite = GFX_tiling_button_standard font = \"cg_16b\" buttonText = @name@ multiline = yes effect = @name@ }",name=name,y=y+24));
            }
        }
        let empty = format!("tte_advice_empty_{area}");
        let position = root
            .value
            .entries()
            .iter()
            .find(|e| e.value.scalar("name") == Some(&empty))
            .ok_or("Trusted advice empty row missing")?
            .span
            .end;
        inserts.push((position, widgets.join("\n")));
    }
    inserts.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (at, raw) in inserts {
        out.gui.insert_str(at, &format!("\n{raw}\n"));
    }
    Ok(out)
}
