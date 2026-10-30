//! Shared, atomic five-slot editing. Only country flags/ranks change; no grants,
//! research gears, card draws, source definitions or research costs are touched.
use crate::{catalog, planning::t, script::Result};

pub fn scripts(ids: &[String], focus: &str) -> Result<String> {
    if !catalog::identifier(focus) || ids.iter().any(|id| !catalog::identifier(id)) {
        return Err("Unsafe goal editor identifier".into());
    }
    let mut clear = String::new();
    let mut capture = String::new();
    let mut origin = String::new();
    let mut displace = String::new();
    let mut assign = String::new();
    for id in ids {
        clear.push_str(&format!("remove_country_flag=ttegs_pending_{id}\n"));
        let current = t!("if={limit={has_country_flag=ttegs_pending_@id@ has_country_flag=tte_goal_@id@} set_variable={which=ttegs_origin value=tte_rank_@id@}}\n",id=id);
        origin.push_str(&current);
        capture.push_str(&t!("if={limit={has_country_flag=@focus@@id@} set_country_flag=ttegs_pending_@id@ change_variable={which=ttegs_count value=1} @current@}\n",focus=focus,id=id,current=current));
        displace.push_str(&t!("if={limit={has_country_flag=tte_goal_@id@ NOT={has_country_flag=ttegs_pending_@id@} check_variable={which=tte_rank_@id@ value=ttegs_destination}} if={limit={check_variable={which=ttegs_origin value>0}} set_variable={which=tte_rank_@id@ value=ttegs_origin}} else={remove_country_flag=tte_goal_@id@ clear_variable=tte_rank_@id@}}\n",id=id));
        assign.push_str(&t!("if={limit={has_country_flag=ttegs_pending_@id@} set_country_flag=tte_goal_@id@ set_variable={which=tte_rank_@id@ value=ttegs_destination}}\n",id=id));
    }
    let occupied = (1..=5).map(|n| format!("if={{limit={{check_variable={{which=ttegs_destination value={n}}} has_country_flag=tte_slot_{n}}} set_country_flag=ttegs_occupied}}\n")).collect::<String>();
    let mut text = include_str!("../assets/goal-slots-core.txt").to_owned();
    for (key, value) in [
        ("CLEAR", clear),
        ("CAPTURE", capture),
        ("ORIGIN", origin),
        ("DISPLACE", displace),
        ("ASSIGN", assign),
        ("OCCUPIED", occupied),
    ] {
        text = text.replace(&format!("{{{{{key}}}}}"), &value);
    }
    if text.contains("{{") {
        return Err("Unexpanded goal editor template".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_and_compiled_editors_share_the_same_script_semantics() {
        let generated = scripts(
            &[
                "alpha".into(),
                "arbitrary_mod_new".into(),
                "repeatable".into(),
            ],
            "tte_viewed_",
        )
        .unwrap();
        let reference = include_str!("../tests/fixtures/goal-slots-reference.txt");
        assert_eq!(
            crate::planning::render(&crate::script::parse(&generated).unwrap()),
            crate::planning::render(&crate::script::parse(reference).unwrap())
        );
    }
    #[test]
    fn identifiers_are_data_and_the_editor_has_constant_windows() {
        assert!(scripts(&["bad } injected=yes".into()], "focus_").is_err());
        let s = scripts(
            &["arbitrary_tech".into(), "repeatable_other".into()],
            "focus_",
        )
        .unwrap();
        let p = crate::script::parse(&s).unwrap();
        assert_eq!(
            p.entries()
                .iter()
                .filter(|e| e.key.as_deref() == Some("country_event"))
                .count(),
            7
        );
        assert!(!s.contains("research_technology"));
        assert!(!s.contains("enable_automatic_research"));
        assert!(s.contains("value=ttegs_origin"));
        assert!(s.contains("ttegs_pending_arbitrary_tech"));
    }
}
