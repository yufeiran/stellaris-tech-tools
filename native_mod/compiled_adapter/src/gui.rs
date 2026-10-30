//! Copy the installed game's code-bound event widgets, changing only owned
//! geometry/classes. Missing native bindings fail instead of being omitted.
use crate::script::{self, Entry, Result, Value};
use std::collections::BTreeMap;
fn gui_variables(v: &Value) -> Result<BTreeMap<String, String>> {
    let mut vars = BTreeMap::new();
    for e in v.entries() {
        if let Some(k) = e.key.as_deref().filter(|k| k.starts_with('@')) {
            let value = e
                .value
                .atom()
                .ok_or("Non-scalar GUI constant is not supported")?;
            if vars.insert(k.into(), value.into()).is_some() {
                return Err(format!("Duplicate GUI constant {k}"));
            }
        }
    }
    Ok(vars)
}
fn resolve_gui_value(
    key: &str,
    vars: &BTreeMap<String, String>,
    stack: &mut Vec<String>,
) -> Result<String> {
    if !key.starts_with('@') {
        return Ok(key.into());
    }
    if stack.len() >= 64 || stack.iter().any(|s| s == key) {
        return Err(format!("Cyclic/deep GUI constant {key}"));
    }
    let value = vars
        .get(key)
        .ok_or_else(|| format!("Unresolved file-local GUI constant {key}"))?;
    stack.push(key.into());
    let result = resolve_gui_value(value, vars, stack);
    stack.pop();
    result
}
fn materialise_gui_constants(raw: &str, inherited: &BTreeMap<String, String>) -> Result<String> {
    fn walk(
        v: &Value,
        raw: &str,
        inherited: &BTreeMap<String, String>,
        changes: &mut Vec<(usize, usize, String)>,
    ) -> Result<()> {
        let mut vars = inherited.clone();
        vars.extend(gui_variables(v)?);
        for e in v.entries() {
            match &e.value {
                Value::Atom(value) if value.starts_with('@') => {
                    let original = &raw[e.span.clone()];
                    let rhs = if e.key.is_some() {
                        original
                            .split_once('=')
                            .map(|(_, s)| s.trim_start())
                            .unwrap_or(original)
                    } else {
                        original
                    };
                    if rhs.starts_with('"') {
                        continue;
                    }
                    let resolved = resolve_gui_value(value, &vars, &mut Vec::new())?;
                    let item = if let Some(key) = &e.key {
                        format!(
                            "{} {} {}",
                            crate::planning::atom(key),
                            e.op,
                            crate::planning::atom(&resolved)
                        )
                    } else {
                        crate::planning::atom(&resolved)
                    };
                    changes.push((e.span.start, e.span.end, item));
                }
                Value::Block(_) => walk(&e.value, raw, &vars, changes)?,
                _ => {}
            }
        }
        Ok(())
    }
    let ast = script::parse(raw)?;
    let mut changes = Vec::new();
    walk(&ast, raw, inherited, &mut changes)?;
    edits(raw, changes)
}
pub fn effective(sources: &crate::sources::Sources, names: &[&str]) -> Result<String> {
    let mut found: std::collections::BTreeMap<&str, Vec<(String, String)>> =
        names.iter().map(|n| (*n, Vec::new())).collect();
    for (path, input) in &sources.files {
        if !path.starts_with("interface/") || !path.ends_with(".gui") {
            continue;
        }
        let parsed =
            script::parse(&input.raw).map_err(|e| format!("GUI index {}: {e}", input.relative))?;
        for root in parsed
            .entries()
            .iter()
            .filter(|e| e.key.as_deref() == Some("guiTypes"))
        {
            for e in root.value.entries() {
                if let Some(name) = e.value.scalar("name") {
                    if let Some(rows) = found.get_mut(name) {
                        let expected = if name == "right_vertical_slider" {
                            "extendedScrollbarType"
                        } else {
                            "containerWindowType"
                        };
                        if e.key.as_deref() != Some(expected) {
                            return Err(format!(
                                "GUI type {name}: expected {expected}, found {:?} in {}",
                                e.key, input.relative
                            ));
                        }
                        // Unrelated UI constants must not make an otherwise
                        // usable source combination fail to build.
                        let mut vars = gui_variables(&parsed)?;
                        vars.extend(gui_variables(&root.value)?);
                        rows.push((
                            input.relative.clone(),
                            materialise_gui_constants(&input.raw[e.span.clone()], &vars)
                                .map_err(|e| format!("{}: {e}", input.relative))?,
                        ));
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for name in names {
        let rows = &found[name];
        if rows.len() != 1 {
            return Err(format!("GUI type {name}: expected one effective definition, found {} in {:?}; cross-file precedence is not guessed",rows.len(),rows.iter().map(|r|&r.0).collect::<Vec<_>>()));
        }
        out.push(rows[0].1.clone());
    }
    Ok(format!("guiTypes={{\n{}\n}}", out.join("\n")))
}

// Native research UI is requested from its original source file. Re-declaring
// its class in an additional late-named file is not a reliable override under
// UI mods. Retain the whole winning file, its constants and sibling classes.
pub fn source_file(sources: &crate::sources::Sources, name: &str) -> Result<(String, String)> {
    let mut found = Vec::new();
    for (path, input) in &sources.files {
        if !path.starts_with("interface/") || !path.ends_with(".gui") {
            continue;
        }
        let ast =
            script::parse(&input.raw).map_err(|e| format!("GUI index {}: {e}", input.relative))?;
        for root in ast
            .entries()
            .iter()
            .filter(|e| e.key.as_deref() == Some("guiTypes"))
        {
            for e in root
                .value
                .entries()
                .iter()
                .filter(|e| e.value.scalar("name") == Some(name))
            {
                if e.key.as_deref() != Some("containerWindowType") {
                    return Err(format!(
                        "GUI type {name}: expected containerWindowType, found {:?} in {}",
                        e.key, input.relative
                    ));
                }
                found.push((input.relative.clone(), input.raw.clone()));
            }
        }
    }
    if found.len() != 1 {
        return Err(format!("GUI type {name}: expected one effective definition, found {} in {:?}; cross-file precedence is not guessed",found.len(),found.iter().map(|f|&f.0).collect::<Vec<_>>()));
    }
    Ok(found.pop().unwrap())
}

pub fn edits(raw: &str, mut changes: Vec<(usize, usize, String)>) -> Result<String> {
    changes.sort_by_key(|e| e.0);
    for pair in changes.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err("Overlapping native GUI edits".into());
        }
    }
    let mut out = raw.to_owned();
    for (start, end, text) in changes.into_iter().rev() {
        out.replace_range(start..end, &text);
    }
    Ok(out)
}
pub fn property(raw: &str, key: &str, value: &str) -> Result<String> {
    let (_, root) = fragment_root(raw)?;
    let fields: Vec<_> = root
        .value
        .entries()
        .iter()
        .filter(|e| {
            e.key
                .as_deref()
                .is_some_and(|k| k.eq_ignore_ascii_case(key))
        })
        .collect();
    let replacement = format!("{key}={value}");
    match fields.as_slice() {
        [] => edits(
            raw,
            vec![(
                root.span.end - 1,
                root.span.end - 1,
                format!("\n{replacement}\n"),
            )],
        ),
        [entry] => edits(raw, vec![(entry.span.start, entry.span.end, replacement)]),
        _ => Err(format!("Ambiguous direct GUI property: {key}")),
    }
}
fn without_property(raw: &str, key: &str) -> Result<String> {
    let (_, root) = fragment_root(raw)?;
    edits(
        raw,
        root.value
            .entries()
            .iter()
            .filter(|e| {
                e.key
                    .as_deref()
                    .is_some_and(|k| k.eq_ignore_ascii_case(key))
            })
            .map(|e| (e.span.start, e.span.end, String::new()))
            .collect(),
    )
}
fn one<'a>(entries: &[&'a Entry], label: &str) -> Result<&'a Entry> {
    match entries {
        [e] => Ok(*e),
        _ => Err(format!("Expected one {label}, found {}", entries.len())),
    }
}
fn fragment_root(raw: &str) -> Result<(Value, Entry)> {
    let parsed = script::parse(raw)?;
    if parsed.entries().len() != 1 {
        return Err("Expected one GUI fragment root".into());
    }
    let entry = parsed.entries()[0].clone();
    Ok((parsed, entry))
}
pub fn named<'a>(value: &'a Value, name: &str) -> Result<&'a Entry> {
    let found: Vec<_> = value
        .entries()
        .iter()
        .filter(|e| e.value.scalar("name") == Some(name))
        .collect();
    one(&found, name)
}
pub fn extract(source: &str, name: &str) -> Result<String> {
    let parsed = script::parse(source)?;
    let roots: Vec<_> = parsed
        .entries()
        .iter()
        .filter(|e| e.key.as_deref() == Some("guiTypes"))
        .collect();
    let root = one(&roots, "guiTypes")?;
    let node = named(&root.value, name)?;
    Ok(source[node.span.clone()].to_owned())
}
fn props(mut raw: String, values: &[(&str, &str)]) -> Result<String> {
    for (key, value) in values {
        raw = property(&raw, key, value)?;
    }
    Ok(raw)
}
pub fn append(raw: &str, text: &str) -> Result<String> {
    let (_, root) = fragment_root(raw)?;
    edits(
        raw,
        vec![(root.span.end - 1, root.span.end - 1, format!("\n{text}\n"))],
    )
}

pub fn shell(source: &str, name: &str, graph: bool) -> Result<String> {
    let original = extract(source, "enclave_curator_window")?;
    let (_, root) = fragment_root(&original)?;
    // Every one of these has a native C++ binding; do not replace the window
    // with a lookalike that lacks them (that caused older custom-event crashes).
    for required in ["EVENT_DIPLO", "alien_message", "background", "close"] {
        named(&root.value, required)?;
    }
    let mut changes = Vec::new();
    for child in root.value.entries() {
        let Some(widget) = child.value.scalar("name") else {
            continue;
        };
        let raw = original[child.span.clone()].to_owned();
        let changed = match widget {
            "background" => property(&raw, "quadTextureSprite", "GFX_tile_outliner_bg_solid")?,
            "close" => props(
                raw,
                &[("position", "{x=-34 y=10}"), ("orientation", "upper_right")],
            )?,
            "alien_message" => {
                if graph {
                    props(
                        raw,
                        &[
                            ("font", "\"cg_16b\""),
                            ("position", "{x=20 y=-84}"),
                            ("orientation", "lower_left"),
                            ("maxWidth", "600"),
                            ("maxHeight", "48"),
                            ("scrollbartype", "\"standardtext_slider\""),
                        ],
                    )?
                } else {
                    props(
                        raw,
                        &[
                            ("font", "\"cg_16b\""),
                            ("position", "{x=-314 y=132}"),
                            ("orientation", "upper_right"),
                            ("maxWidth", "294"),
                            ("maxHeight", "320"),
                            ("scrollbartype", "\"standardtext_slider\""),
                        ],
                    )?
                }
            }
            "EVENT_DIPLO" => {
                for required in ["action_title", "action_desc", "option_list"] {
                    named(&child.value, required)?;
                }
                let mut inside = Vec::new();
                for item in child.value.entries() {
                    let Some(n) = item.value.scalar("name") else {
                        continue;
                    };
                    let part = original[item.span.clone()].to_owned();
                    let fixed = match n {
                        "option_list" => props(
                            without_property(&part, "horizontalScrollbar")?,
                            &[
                                ("position", "{x=0 y=0}"),
                                ("orientation", "upper_left"),
                                (
                                    "size",
                                    if graph {
                                        "{x=294 y=384}"
                                    } else {
                                        "{x=520 y=384}"
                                    },
                                ),
                                ("scrollbartype", "\"standardlistbox_slider\""),
                            ],
                        )?,
                        "action_title" if !graph => props(
                            part,
                            &[
                                ("position", "{x=0 y=-72}"),
                                ("orientation", "upper_left"),
                                ("maxWidth", "600"),
                                ("maxHeight", "52"),
                                ("font", "\"cg_16b\""),
                            ],
                        )?,
                        _ => property(&part, "position", "{x=-10000 y=-10000}")?,
                    };
                    inside.push((
                        item.span.start - child.span.start,
                        item.span.end - child.span.start,
                        fixed,
                    ));
                }
                let raw = without_property(&edits(&raw, inside)?, "horizontalScrollbar")?;
                props(
                    raw,
                    &[
                        (
                            "position",
                            if graph {
                                "{x=-314 y=52}"
                            } else {
                                "{x=24 y=88}"
                            },
                        ),
                        (
                            "orientation",
                            if graph { "upper_right" } else { "upper_left" },
                        ),
                        ("origo", "upper_left"),
                        (
                            "size",
                            if graph {
                                "{width=300 height=-64}"
                            } else {
                                "{width=528 height=398}"
                            },
                        ),
                    ],
                )?
            }
            _ => property(&raw, "position", "{x=-10000 y=-10000}")?,
        };
        changes.push((child.span.start, child.span.end, changed));
    }
    let result = props(
        without_property(&edits(&original, changes)?, "horizontalScrollbar")?,
        &[
            ("name", name),
            ("position", "{x=0 y=0}"),
            ("orientation", "upper_left"),
            ("origo", "upper_left"),
            ("moveable", "no"),
            ("size", "{width=100% height=100%}"),
        ],
    )?;
    if graph {
        append(&result,"instantTextBoxType={name=tte_graph_heading position={x=20 y=18} text=tte_full font=\"cg_16b\" maxWidth=600 maxHeight=28 alwaysTransparent=yes}")
    } else {
        let result = props(
            result,
            &[
                ("size", "{width=960 height=540}"),
                ("orientation", "center"),
                ("origo", "center"),
            ],
        )?;
        append(&result,"instantTextBoxType={name=tte_sidebar_heading position={x=-314 y=80} orientation=upper_right text=tte_select_sidebar font=\"cg_16b\" maxWidth=294 maxHeight=40 alwaysTransparent=yes}")
    }
}

pub fn option_class(source: &str) -> Result<String> {
    let option = extract(source, "enclave_curator_option")?;
    let (_, root) = fragment_root(&option)?;
    let mut changes = Vec::new();
    for child in root.value.entries() {
        let Some(name) = child.value.scalar("name") else {
            continue;
        };
        let raw = option[child.span.clone()].to_owned();
        let changed = if name == "option_button" {
            props(
                raw,
                &[
                    ("quadTextureSprite", "GFX_tiling_button_standard"),
                    ("size", "{x=518 y=42}"),
                    ("position", "{x=0 y=0}"),
                    ("orientation", "upper_left"),
                    ("multiline", "yes"),
                ],
            )?
        } else {
            property(&raw, "position", "{x=-10000 y=-10000}")?
        };
        changes.push((child.span.start, child.span.end, changed));
    }
    props(
        edits(&option, changes)?,
        &[
            ("name", "tte_option"),
            ("size", "{width=520 height=44}"),
            ("orientation", "upper_left"),
            ("origo", "upper_left"),
        ],
    )
}

pub fn research_entry(source: &str, advice: &str) -> Result<String> {
    let parsed = script::parse(source)?;
    let roots: Vec<_> = parsed
        .entries()
        .iter()
        .filter(|e| e.key.as_deref() == Some("guiTypes"))
        .collect();
    let root = one(&roots, "research guiTypes")?;
    let window = named(&root.value, "technology_view_window")?;
    let select = named(&window.value, "select_window")?;
    if window
        .value
        .entries()
        .iter()
        .any(|e| e.value.scalar("name") == Some("tte_open_tree"))
    {
        return Err("Planner entry already present in base game UI".into());
    }
    let entry="effectbuttonType={name=tte_open_tree position={x=180 y=6} size={x=170 y=24} quadTextureSprite=GFX_tiling_button_standard font=\"cg_16b\" buttonText=tte_open effect=tte_open clicksound=click}";
    let select_fixed = append(
        &property(&source[select.span.clone()], "clipping", "no")?,
        &props(
            advice.to_owned(),
            &[
                ("orientation", "upper_right"),
                ("origo", "upper_left"),
                ("position", "{x=8 y=0}"),
            ],
        )?,
    )?;
    edits(
        source,
        vec![
            (
                window.span.end - 1,
                window.span.end - 1,
                format!("\n{entry}\n"),
            ),
            (select.span.start, select.span.end, select_fixed),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advice_follows_source_selection_window_right_edge() {
        let source = "guiTypes={containerWindowType={name=technology_view_window containerWindowType={name=select_window size={width=565 height=455}}}}";
        let advice = "containerWindowType={name=tte_advice_panel position={x=498 y=0} size={width=356 height=450}}";
        let result = research_entry(source, advice).unwrap();
        let parsed = script::parse(&result).unwrap();
        let root = &parsed.entries()[0].value;
        let window = &named(root, "technology_view_window").unwrap().value;
        let select = &named(window, "select_window").unwrap().value;
        assert_eq!(select.first("size").unwrap().scalar("width"), Some("565"));
        let panel = &named(select, "tte_advice_panel").unwrap().value;
        assert_eq!(panel.scalar("orientation"), Some("upper_right"));
        assert_eq!(panel.scalar("origo"), Some("upper_left"));
        assert_eq!(panel.first("position").unwrap().scalar("x"), Some("8"));
    }
    #[test]
    fn extracted_gui_constants_are_local_and_unknowns_fail() {
        let vars = BTreeMap::from([("@size".into(), "32".into())]);
        let raw="containerWindowType={name=owned size={x=2 y=@size} text=\"@literal\" containerWindowType={name=inner @size=19 size={x=@size y=@size}}}";
        let result = materialise_gui_constants(raw, &vars).unwrap();
        assert!(result.contains("y = 32"));
        assert!(result.contains("x = 19 y = 19"));
        assert!(result.contains("text=\"@literal\""));
        assert!(materialise_gui_constants(
            "containerWindowType={name=owned size={x=@missing}}",
            &vars
        )
        .is_err());
    }
    const SCHEMA:&str="guiTypes={containerWindowType={name=enclave_curator_window orientation=center origo=center size={width=974 height=680} background={name=background quadTextureSprite=original} containerWindowType={name=EVENT_DIPLO instantTextBoxType={name=action_title} instantTextBoxType={name=action_desc} listboxType={name=option_list}} instantTextBoxType={name=alien_message} buttonType={name=close} containerWindowType={name=portrait position={x=1 y=1}}} containerWindowType={name=enclave_curator_option size={width=388 height=30} buttonType={name=option_button text=OPTION_TEXT} overlappingElementsBoxType={name=trigger_icons}}}";
    #[test]
    fn full_screen_shell_preserves_native_control_bindings() {
        let result = shell(SCHEMA, "tte_graph_window", true).unwrap();
        let parsed = script::parse(&result).unwrap();
        let root = &parsed.entries()[0].value;
        for name in [
            "background",
            "EVENT_DIPLO",
            "alien_message",
            "close",
            "portrait",
        ] {
            named(root, name).unwrap();
        }
        let event = &named(root, "EVENT_DIPLO").unwrap().value;
        for name in ["action_title", "action_desc", "option_list"] {
            named(event, name).unwrap();
        }
        assert_eq!(root.scalar("orientation"), Some("upper_left"));
        assert_eq!(root.first("size").unwrap().scalar("width"), Some("100%"));
    }
    #[test]
    fn missing_native_widget_fails_without_creating_a_lookalike() {
        assert!(shell(
            &SCHEMA.replace("name=alien_message", "name=missing"),
            "tte_graph_window",
            true
        )
        .is_err());
    }
    #[test]
    fn nested_property_with_same_name_is_not_edited() {
        let raw="containerWindowType={name=root size={width=10 height=20} containerWindowType={name=child size={width=7 height=8}}}";
        let changed = property(raw, "size", "{width=100% height=100%}").unwrap();
        assert!(changed.contains("name=child size={width=7 height=8}"));
    }
    #[test]
    fn source_anchors_and_case_variants_do_not_move_owned_controls() {
        let source = SCHEMA
            .replace(
                "name=EVENT_DIPLO",
                "name=EVENT_DIPLO origo=lower_right Orientation=lower_right",
            )
            .replace(
                "name=option_list",
                "name=option_list scrollbarType=old Orientation=lower_left",
            );
        for graph in [false, true] {
            let result = shell(&source, "owned", graph).unwrap();
            let root = script::parse(&result).unwrap();
            let event = &named(&root.entries()[0].value, "EVENT_DIPLO")
                .unwrap()
                .value;
            assert_eq!(event.scalar("origo"), Some("upper_left"));
            let list = &named(event, "option_list").unwrap().value;
            assert_eq!(list.scalar("origo"), None);
            assert_eq!(list.scalar("orientation"), Some("upper_left"));
            assert_eq!(
                list.entries()
                    .iter()
                    .filter(|e| e
                        .key
                        .as_deref()
                        .is_some_and(|s| s.eq_ignore_ascii_case("scrollbartype")))
                    .count(),
                1
            );
        }
    }
    #[test]
    fn inherited_horizontal_scrollbars_are_removed_at_each_owned_level() {
        let source = SCHEMA
            .replace(
                "name=enclave_curator_window",
                "name=enclave_curator_window HorizontalScrollbar=foreign_root",
            )
            .replace(
                "name=EVENT_DIPLO",
                "name=EVENT_DIPLO horizontalscrollbar=foreign_event",
            )
            .replace(
                "name=option_list",
                "name=option_list horizontalScrollbar=foreign_list",
            );
        for graph in [false, true] {
            let result = shell(&source, "owned", graph).unwrap();
            assert!(!result.to_ascii_lowercase().contains("horizontalscrollbar"));
            script::parse(&result).unwrap();
        }
    }
}
