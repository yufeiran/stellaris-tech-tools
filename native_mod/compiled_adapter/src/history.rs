//! MRU history uses stable technology flags and hidden event boundaries. Never
//! inline an O(N) shift/recount into every preview (O(N²) engine expansion).
use crate::planning::{t, Tech, MAX_GOALS};
pub struct History {
    pub effects: Vec<String>,
    pub events: Vec<String>,
    pub getters: Vec<String>,
}
pub fn compile(techs: &[Tech]) -> History {
    let mut out = History {
        effects: Vec::new(),
        events: vec!["namespace=ttehistory".into()],
        getters: Vec::new(),
    };
    let mut shift = Vec::new();
    let mut clear = Vec::new();
    let mut recount = (1..=8)
        .map(|slot| format!("remove_country_flag=tte_recent_slot_{slot}"))
        .collect::<Vec<_>>();
    for (i, tech) in techs.iter().enumerate() {
        let key = &tech.id;
        shift.push(t!("if={limit={has_country_flag=tte_recent_tech_@key@ OR={check_variable={which=tte_recent_previous value=0} check_variable={which=tte_recent_rank_@key@ value<tte_recent_previous}}} change_variable={which=tte_recent_rank_@key@ value=1} if={limit={check_variable={which=tte_recent_rank_@key@ value>8}} remove_country_flag=tte_recent_tech_@key@ clear_variable=tte_recent_rank_@key@}}",key=key));
        for slot in 1..=8 {
            recount.push(t!("if={limit={has_country_flag=tte_recent_tech_@key@ check_variable={which=tte_recent_rank_@key@ value=@slot@}} set_country_flag=tte_recent_slot_@slot@}",key=key,slot=slot));
        }
        clear.push(t!(
            "remove_country_flag=tte_recent_tech_@key@ clear_variable=tte_recent_rank_@key@",
            key = key
        ));
        out.effects.push(t!(
            "tte_remember_@key@={country_event={id=ttehistory.@id@}}",
            key = key,
            id = 1000 + i
        ));
        out.events.push(t!("country_event={id=ttehistory.@id@ hide_window=yes is_triggered_only=yes immediate={set_variable={which=tte_recent_previous value=0} if={limit={has_country_flag=tte_recent_tech_@key@} set_variable={which=tte_recent_previous value=tte_recent_rank_@key@}} tte_recent_shift=yes set_country_flag=tte_recent_tech_@key@ set_variable={which=tte_recent_rank_@key@ value=1} tte_recent_recount=yes clear_variable=tte_recent_previous}}",id=1000+i,key=key));
    }
    clear.extend(recount[..8].iter().cloned());
    for (n, name, body) in [
        (1, "tte_recent_shift", shift),
        (2, "tte_recent_recount", recount),
        (3, "tte_recent_clear", clear),
    ] {
        out.effects.push(t!(
            "@name@={country_event={id=ttehistory.@n@}}",
            name = name,
            n = n
        ));
        out.events.push(t!("country_event={id=ttehistory.@n@ hide_window=yes is_triggered_only=yes immediate={@body@}}",n=n,body=body.join("\n")));
    }
    for (kind, max, flag, rank) in [
        ("Recent", 8, "tte_recent_tech_", "tte_recent_rank_"),
        ("Tracked", MAX_GOALS, "tte_goal_", "tte_rank_"),
    ] {
        for slot in 1..=max {
            let mut texts = Vec::new();
            let mut jumps = Vec::new();
            for (i, tech) in techs.iter().enumerate() {
                let condition = t!(
                    "has_country_flag=@flag@@key@ check_variable={which=@rank@@key@ value=@slot@}",
                    flag = flag,
                    key = tech.id,
                    rank = rank,
                    slot = slot
                );
                texts.push(t!(
                    "text={trigger={@condition@} localization_key=@key@}",
                    condition = condition,
                    key = tech.id
                ));
                jumps.push(t!(
                    "if={limit={@condition@} country_event={id=tte.@id@}}",
                    condition = condition,
                    id = 1000 + i
                ));
            }
            out.getters.push(t!("defined_text={name=GetTTE@kind@Entry@slot@ random=no @texts@ default=tte_goal_none}",kind=kind,slot=slot,texts=texts.join(" ")));
            if kind == "Recent" {
                out.effects.push(t!(
                    "tte_open_recent_@slot@={@jumps@}",
                    slot = slot,
                    jumps = jumps.join(" ")
                ));
            }
        }
    }
    out
}
