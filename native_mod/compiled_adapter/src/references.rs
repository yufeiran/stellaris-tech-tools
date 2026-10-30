//! Validate generated symbols and the native GUI resources we explicitly use.
use crate::{
    script::{self, Result, Value},
    sources::Sources,
};
use std::collections::{BTreeMap, BTreeSet};
fn definitions(v: &Value, kind: &str, names: &mut BTreeSet<String>) {
    for e in v.entries() {
        if e.key
            .as_deref()
            .is_some_and(|k| k == kind || kind == "spriteType" && k.ends_with("SpriteType"))
        {
            if let Some(name) = e.value.scalar("name") {
                names.insert(name.into());
            }
        }
        definitions(&e.value, kind, names);
    }
}
pub fn source_gui(sources: &Sources) -> Result<()> {
    let mut scrolls = BTreeSet::new();
    let mut sprites = BTreeSet::new();
    let mut fonts = BTreeSet::new();
    for (path, input) in &sources.files {
        if !path.starts_with("interface/") || !(path.ends_with(".gui") || path.ends_with(".gfx")) {
            continue;
        }
        let ast = script::parse(&input.raw)
            .map_err(|e| format!("GUI resource index {}: {e}", input.relative))?;
        definitions(&ast, "extendedScrollbarType", &mut scrolls);
        definitions(&ast, "scrollbarType", &mut scrolls);
        definitions(&ast, "spriteType", &mut sprites);
        definitions(&ast, "bitmapfont", &mut fonts);
    }
    for (kind, names, needed) in [
        (
            "scrollbar",
            &scrolls,
            &[
                "right_vertical_slider",
                "standardtext_slider",
                "standardlistbox_slider",
            ][..],
        ),
        (
            "sprite",
            &sprites,
            &[
                "GFX_tiling_button_standard",
                "GFX_tile_outliner_bg",
                "GFX_tile_outliner_bg_solid",
            ][..],
        ),
        ("font", &fonts, &["cg_16b"][..]),
    ] {
        for name in needed {
            if !names.contains(*name) {
                return Err(format!(
                    "Missing native {kind} resource {name}; no dangling GUI is generated"
                ));
            }
        }
    }
    Ok(())
}
fn prefixed(name: &str) -> bool {
    name.starts_with("tte_") || name.starts_with("ttepart_")
}
pub fn generated(files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let mut calls = BTreeSet::new();
    let mut buttons = BTreeSet::new();
    let mut events = BTreeSet::new();
    let mut classes = BTreeSet::new();
    let mut scrolls = BTreeSet::new();
    let mut sprites = BTreeSet::new();
    for (path, bytes) in files {
        if !path.ends_with(".txt") && !path.ends_with(".gui") && !path.ends_with(".gfx") {
            continue;
        }
        let raw = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let ast = script::parse(raw)?;
        if path.ends_with(".gui") {
            definitions(&ast, "extendedScrollbarType", &mut scrolls);
            definitions(&ast, "scrollbarType", &mut scrolls);
        }
        if path.ends_with(".gfx") {
            definitions(&ast, "spriteType", &mut sprites);
        }
        for e in ast.entries() {
            if path.starts_with("common/scripted_effects/")
                || path.starts_with("common/scripted_triggers/")
            {
                if let Some(name) = &e.key {
                    calls.insert(name.clone());
                }
            }
            if path.starts_with("common/button_effects/") {
                if let Some(name) = &e.key {
                    buttons.insert(name.clone());
                }
            }
            if path.starts_with("events/") {
                if let Some(id) = e.value.scalar("id") {
                    events.insert(id.into());
                }
            }
        }
        if path.ends_with(".gui") {
            if let Some(root) = ast.first("guiTypes") {
                for e in root.entries() {
                    if let Some(name) = e.value.scalar("name") {
                        classes.insert(name.into());
                    }
                }
            }
        }
    }
    fn visit(
        v: &Value,
        calls: &BTreeSet<String>,
        buttons: &BTreeSet<String>,
        events: &BTreeSet<String>,
        classes: &BTreeSet<String>,
        scrolls: &BTreeSet<String>,
        sprites: &BTreeSet<String>,
    ) -> Result<()> {
        for e in v.entries() {
            let key = e.key.as_deref().unwrap_or("");
            if prefixed(key) && matches!(e.value.atom(), Some("yes" | "no")) && !calls.contains(key)
            {
                return Err(format!("Missing generated scripted symbol: {key}"));
            }
            if key == "effect" {
                if let Some(name) = e.value.atom() {
                    if prefixed(name) && !buttons.contains(name) {
                        return Err(format!("Missing generated button effect: {name}"));
                    }
                }
            }
            if key.ends_with("_event") || key == "event" {
                if let Some(id) = e.value.scalar("id") {
                    if id.starts_with("tte") && !events.contains(id) {
                        return Err(format!("Missing generated event: {id}"));
                    }
                }
            }
            if key == "events" {
                for item in e.value.entries() {
                    if let Some(id) = item.value.atom() {
                        if id.starts_with("tte") && !events.contains(id) {
                            return Err(format!("Missing generated on-action event: {id}"));
                        }
                    }
                }
            }
            if key == "custom_gui" || key == "custom_gui_option" {
                if let Some(name) = e.value.atom() {
                    if prefixed(name) && !classes.contains(name) {
                        return Err(format!("Missing generated GUI class: {name}"));
                    }
                }
            }
            if key.eq_ignore_ascii_case("verticalScrollbar") {
                if let Some(name) = e.value.atom() {
                    if prefixed(name) && !scrolls.contains(name) {
                        return Err(format!("Missing generated scrollbar: {name}"));
                    }
                }
            }
            if key.eq_ignore_ascii_case("spriteType")
                || key.eq_ignore_ascii_case("quadTextureSprite")
            {
                if let Some(name) = e.value.atom() {
                    if name.starts_with("GFX_tte_") && !sprites.contains(name) {
                        return Err(format!("Missing generated sprite: {name}"));
                    }
                }
            }
            visit(&e.value, calls, buttons, events, classes, scrolls, sprites)?;
        }
        Ok(())
    }
    for (path, bytes) in files {
        if !(path.ends_with(".gui") || path.ends_with(".txt"))
            || path.starts_with("common/technology/")
        {
            continue;
        }
        let ast = script::parse(std::str::from_utf8(bytes).map_err(|e| e.to_string())?)?;
        // Root entries define symbols; only nested entries invoke them.
        for e in ast.entries() {
            visit(
                &e.value, &calls, &buttons, &events, &classes, &scrolls, &sprites,
            )
            .map_err(|e| format!("{path}: {e}"))?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_on_action_callbacks_must_resolve() {
        let mut files = BTreeMap::from([(
            "common/on_actions/owned.txt".into(),
            b"on_game_start={events={tte.42 native_foreign.1}}".to_vec(),
        )]);
        assert!(generated(&files)
            .unwrap_err()
            .contains("on-action event: tte.42"));
        files.insert(
            "events/owned.txt".into(),
            b"namespace=tte country_event={id=tte.42}".to_vec(),
        );
        assert!(generated(&files).is_ok());
    }
    #[test]
    fn generated_scrollbar_and_connector_sprite_must_resolve() {
        let mut files=BTreeMap::from([("interface/owned.gui".into(),b"guiTypes={containerWindowType={name=owned VerticalScrollbar=tte_tree_wheel background={name=line quadTextureSprite=GFX_tte_native_line}}}".to_vec())]);
        assert!(generated(&files)
            .unwrap_err()
            .contains("scrollbar: tte_tree_wheel"));
        files.insert(
            "interface/scroll.gui".into(),
            b"guiTypes={extendedScrollbarType={name=tte_tree_wheel}}".to_vec(),
        );
        assert!(generated(&files)
            .unwrap_err()
            .contains("sprite: GFX_tte_native_line"));
        files.insert(
            "interface/line.gfx".into(),
            b"spriteTypes={spriteType={name=GFX_tte_native_line}}".to_vec(),
        );
        assert!(generated(&files).is_ok());
    }
    #[test]
    fn dangling_generated_navigation_and_calls_are_rejected() {
        let mut f = BTreeMap::new();
        f.insert(
            "events/owned.txt".into(),
            b"country_event={id=tte.1 immediate={tte_missing=yes}}".to_vec(),
        );
        assert!(generated(&f).unwrap_err().contains("tte_missing"));
        f.insert(
            "common/scripted_effects/owned.txt".into(),
            b"tte_missing={country_event={id=tte.99}}".to_vec(),
        );
        assert!(generated(&f).unwrap_err().contains("tte.99"));
        f.insert(
            "events/owned.txt".into(),
            b"country_event={id=tte.1 immediate={tte_missing=yes}} country_event={id=tte.99}"
                .to_vec(),
        );
        assert!(generated(&f).is_ok());
    }
}
