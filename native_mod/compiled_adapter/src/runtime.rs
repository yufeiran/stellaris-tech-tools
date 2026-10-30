//! Assemble an explicit experimental local planner. Collection remains usable
//! independently. Generated candidates must pass native QA before public use.
use crate::{
    advice, diagnostics, emitter, gui, partition, planner_emit, planning,
    script::{self, Result},
    sources::{self, Sources},
    view,
};
use serde_json::{json, Value as Json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
};

pub struct Runtime {
    pub files: BTreeMap<String, Vec<u8>>,
    pub proof: Json,
}
fn merge(files: &mut BTreeMap<String, Vec<u8>>, incoming: BTreeMap<String, Vec<u8>>) -> Result<()> {
    for (name, bytes) in incoming {
        if files.insert(name.clone(), bytes).is_some() {
            return Err(format!("Generated path collision: {name}"));
        }
    }
    Ok(())
}
fn put(files: &mut BTreeMap<String, Vec<u8>>, name: &str, text: impl Into<String>) -> Result<()> {
    if files
        .insert(name.into(), text.into().into_bytes())
        .is_some()
    {
        return Err(format!("Generated path collision: {name}"));
    }
    Ok(())
}
fn loc_merge(base: &mut view::Localisation, extra: view::Localisation) {
    for (lang, rows) in extra {
        base.entry(lang).or_default().extend(rows);
    }
}
fn validate_ids(report: &Json) -> Result<()> {
    for id in report["technologies"]
        .as_object()
        .ok_or("Catalog missing")?
        .keys()
    {
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!(
                "Unverified technology ID for generated symbol: {id}; collection still works"
            ));
        }
    }
    Ok(())
}
fn validate_objects(objects: &[String], kind: &str, field: &str) -> Result<usize> {
    let mut names = BTreeSet::new();
    for raw in objects {
        let parsed = script::parse(raw)?;
        for e in parsed.entries() {
            if e.key.as_deref() == Some("namespace") {
                continue;
            }
            let name = if field.is_empty() {
                e.key.as_deref()
            } else {
                e.value.scalar(field)
            }
            .ok_or_else(|| format!("Missing {kind} name"))?;
            if !names.insert(name.to_owned()) {
                return Err(format!("Duplicate generated {kind}: {name}"));
            }
        }
    }
    Ok(names.len())
}
fn namespace_order(objects: &[String]) -> Result<()> {
    let mut known = BTreeSet::new();
    for raw in objects {
        for e in script::parse(raw)?.entries() {
            if e.key.as_deref() == Some("namespace") {
                known.insert(e.value.atom().ok_or("Invalid event namespace")?.to_owned());
                continue;
            }
            if let Some(id) = e.value.scalar("id") {
                let namespace = id.split('.').next().unwrap_or("");
                if !known.contains(namespace) {
                    return Err(format!(
                        "Event namespace must precede its definitions: {id}"
                    ));
                }
            }
        }
    }
    Ok(())
}
pub fn assemble(sources: &mut Sources, report: &Json) -> Result<Runtime> {
    validate_ids(report)?;
    crate::references::source_gui(sources)?;
    eprintln!("[build 1/6] Preserve source technology definitions / 核对来源科技定义");
    let techs = planning::technologies(report)?;
    let weights = emitter::technology_weights(sources, report)?;
    eprintln!("[build 2/6] Compile planning and manual advice / 生成规划与手动推荐");
    let potential = diagnostics::potential_triggers(sources)?;
    let mut planner = planner_emit::compile(&techs, &potential)?;
    let advice = advice::compile(&techs)?;
    let diag = diagnostics::compile(&techs, sources)?;
    let mut advice_insertions = 0;
    for effect in &mut planner.effects {
        if effect.starts_with("tte_refresh_selection = {") {
            let end = effect.rfind('}').ok_or("Malformed selection effect")?;
            effect.insert_str(end, "\ntte_refresh_advice=yes\n");
            advice_insertions += 1;
        }
    }
    if advice_insertions != 1 {
        return Err("Expected exactly one manual-advice refresh insertion".into());
    }
    eprintln!("[build 3/6] Build shared tree and selection menus / 生成共用科技树和选择菜单");
    let mut ui = view::compile(&techs, report, sources)?;
    planner.effects.extend(advice.effects);
    planner.effects.extend(diag.effects);
    planner.effects.extend(ui.effects);
    planner.triggers.extend(advice.triggers);
    let mut buttons = advice.buttons;
    buttons.extend(ui.buttons);
    let mut getters = advice.getters;
    getters.extend(diag.getters);
    getters.extend(ui.getters);
    planner.events.extend(ui.events);
    // Shared heavy routines must cross a country-event boundary. Otherwise
    // event chunk -> selection dispatcher -> selection chunk -> advice ->
    // advice chunk exceeds the engine's five-level post-init traversal.
    for (number, name) in [
        (1, "tte_refresh_selection"),
        (2, "tte_refresh_advice"),
        (10, "tte_inspect_selected"),
        (11, "tte_track_selected"),
        (12, "tte_untrack_selected"),
    ] {
        let index = planner
            .effects
            .iter()
            .position(|raw| {
                script::parse(raw).ok().is_some_and(|p| {
                    p.entries().len() == 1 && p.entries()[0].key.as_deref() == Some(name)
                })
            })
            .ok_or_else(|| format!("Missing shared routine {name}"))?;
        let parsed = script::parse(&planner.effects[index])?;
        let body = planning::render(&parsed.entries()[0].value);
        planner.effects[index] = format!("{name}={{country_event={{id=tteaux.{number}}}}}");
        planner.events.push(format!("country_event={{id=tteaux.{number} hide_window=yes is_triggered_only=yes immediate={{ {body} }} }}"));
    }
    planner.events.insert(0, "namespace=tteaux".into());
    loc_merge(&mut ui.localisation, advice.localisation);
    loc_merge(&mut ui.localisation, diag.localisation);
    let emitted_keys: BTreeSet<_> = ui
        .localisation
        .values()
        .flat_map(|m| m.keys().cloned())
        .collect();
    crate::languages::expand(&mut ui.localisation, report, sources)?;
    // The language pass imports absent source keys as fallbacks. Those keys
    // remain source-owned even though a generated locale contains a copy.
    let foreign_keys: BTreeSet<_> = ui
        .localisation
        .values()
        .flat_map(|m| m.keys().cloned())
        .filter(|k| !emitted_keys.contains(k))
        .collect();
    crate::traditional::settings(&mut ui.localisation, &mut planner.events)?;
    let extra_keys = crate::traditional::caption_keys(
        ui.gui
            .iter()
            .chain(planner.events.iter())
            .chain(buttons.iter())
            .map(String::as_str),
    )?;
    let mut preference = crate::traditional::Preference::prepare(
        &mut ui.localisation,
        sources,
        &mut getters,
        extra_keys,
    )?;
    for class in &mut ui.gui {
        *class = preference.gui(class, &mut getters, &mut buttons, false)?;
    }
    let (research_path, research) = gui::source_file(sources, "technology_view_window")?;
    let research_source_hash = sources::sha(research.as_bytes());
    let research = gui::research_entry(&research, &advice.gui)?;
    let research = preference.gui(&research, &mut getters, &mut buttons, true)?;
    for raw in planner.events.iter_mut().chain(buttons.iter_mut()) {
        *raw = preference.event_strings(raw, &mut ui.localisation, &mut getters)?;
    }
    let button_count = validate_objects(&buttons, "button effect", "")?;
    let trigger_count = validate_objects(&planner.triggers, "trigger", "")?;
    let effect_count = validate_objects(&planner.effects, "scripted effect", "")?;
    let getter_count = validate_objects(&getters, "localisation getter", "name")?;
    let event_count = validate_objects(&planner.events, "event", "id")?;
    namespace_order(&planner.events)?;
    validate_objects(&ui.gui, "GUI class", "name")?;
    eprintln!("[build 4/6] Bound script parser units / 限制单个脚本大小");
    let (effect_text, chunks) = partition::effects(&planner.effects.join("\n"), "native_runtime")?;
    let mut event_helpers = Vec::new();
    let mut events = Vec::new();
    for raw in &planner.events {
        let (text, helpers) = partition::hidden_events(raw)?;
        events.push(text);
        event_helpers.extend(helpers);
    }
    let effect_text = effect_text + "\n" + &event_helpers.join("\n");
    let effect_depth = partition::effect_depth(&effect_text)?;
    let roots = events
        .iter()
        .chain(buttons.iter())
        .cloned()
        .collect::<Vec<_>>();
    let eager_cost = partition::eager_effect_guard(&effect_text, &roots)?;
    let effect_parsed = script::parse(&effect_text)?;
    let effects: Vec<_> = effect_parsed
        .entries()
        .iter()
        .map(|e| effect_text[e.span.clone()].to_owned())
        .collect();
    let mut files = BTreeMap::new();
    for (path, text) in weights.files {
        put(&mut files, &path, text)?;
    }
    merge(
        &mut files,
        partition::files("common/scripted_effects", &effects)?,
    )?;
    merge(
        &mut files,
        partition::files("common/scripted_triggers", &planner.triggers)?,
    )?;
    merge(
        &mut files,
        partition::files("common/button_effects", &buttons)?,
    )?;
    merge(
        &mut files,
        partition::files("common/scripted_loc", &getters)?,
    )?;
    let mut namespaces = BTreeSet::new();
    for raw in &events {
        for e in script::parse(raw)?.entries() {
            if e.key.as_deref() == Some("namespace") {
                namespaces.insert(e.value.atom().ok_or("Invalid namespace")?.to_owned());
            }
        }
    }
    let preamble = namespaces
        .iter()
        .map(|n| format!("namespace={n}\n"))
        .collect::<String>();
    let event_files = partition::files_with_preamble("events", &events, &preamble)?;
    for bytes in event_files.values() {
        namespace_order(&[String::from_utf8(bytes.clone()).map_err(|e| e.to_string())?])?;
    }
    merge(&mut files, event_files)?;
    // GUI's engine wrapper is restored around each complete class partition.
    for (index, class) in ui.gui.iter().enumerate() {
        // GUI classes cannot be instantiated by inventing a new widget type.
        // Emit one ordinary class per file and bound the formatter's actual
        // structural line estimate, rather than script-token cost.
        let parsed = script::parse(class)?;
        let lines = partition::formatted_lines(&parsed);
        if lines > 90000 {
            return Err(format!(
                "GUI class exceeds safe formatter estimate: {lines}"
            ));
        }
        put(
            &mut files,
            &format!("interface/tte_native_{index:04}.gui"),
            format!("guiTypes={{\n{class}\n}}"),
        )?;
    }
    // The F4 entry does not require replacing the shared scientist portrait.
    // Keep the effective source layout, including UI mods' relative geometry.
    let research_proof = json!({"path":research_path,"sourceSha256":research_source_hash,"outputSha256":sources::sha(research.as_bytes()),"edits":["append planner entry to technology_view_window","set select_window clipping=no","append advice at select_window upper_right+8","bind saved caption preference only inside inserted tte_ controls"],"otherClassesAndFileConstants":"preserved at original source path"});
    if partition::formatted_lines(&script::parse(&research)?) > 90000 {
        return Err(
            "Research advice GUI exceeds safe formatter estimate; reduce the source combination"
                .into(),
        );
    }
    put(&mut files, &research_path, research)?;
    put(
        &mut files,
        "interface/tte_native_tree.gfx",
        format!("spriteTypes={{\n{}\n}}", ui.gfx.join("\n")),
    )?;
    merge(&mut files, ui.assets)?;
    merge(&mut files, view::localisation_files(&ui.localisation)?)?;
    put(&mut files,"common/on_actions/tte_native_on_actions.txt","on_tech_increased={events={tte.2}}\non_monthly_pulse={events={tte.3}}\non_research_option_added={events={tte.4}}\non_single_player_save_game_load={events={tte.3}}\n")?;
    put(&mut files,"common/edicts/tte_native_open.txt","tte_open_planner={length=0 potential={is_ai=no} allow={always=yes} effect={country_event={id=tte.1}}}\n")?;
    // The generated edict is a zero-cost fallback entry if another UI replaces F4.
    for lang in crate::languages::LANGUAGES {
        let path = format!("localisation/{lang}/tte_native_l_{lang}.yml");
        let bytes = files.get_mut(&path).unwrap();
        bytes.extend(b" edict_tte_open_planner:0 \"$tte_open_planner_label$\"\n edict_tte_open_planner_desc:0 \"$tte_generated_help$\"\n");
    }
    eprintln!("[build 5/6] Validate references and source freshness / 校验引用和来源");
    // Ownership comes from our emitters, never from a source technology's
    // spelling. A foreign ID may start with tte_ and lack localisation.
    let owned_keys = ui
        .localisation
        .values()
        .flat_map(|m| m.keys().cloned())
        .filter(|k| !foreign_keys.contains(k))
        .collect();
    validate_localisation(&files, &ui.localisation, &owned_keys)?;
    crate::references::generated(&files)?;
    sources.verify()?;
    let name = "Tech Tree & Research Planner - Local TEST";
    put(&mut files,"descriptor.mod",format!("name=\"{name}\"\nversion=\"1.2.0-native-test\"\nsupported_version=\"4.5.*\"\ntags={{\"Technologies\" \"Gameplay\"}}\n"))?;
    put(&mut files,"README.txt","EXPERIMENTAL LOCAL TEST / 本地测试候选\nGenerated entirely by native code; no Python runtime.\nEnable this AFTER the exact source mods and disable the Workshop planner.\nOnly one planner may be active. Do not upload third-party/game source data.\nRerun after source technologies/playset change. Keep a save copy.\nNo source mods, launcher selection or saves were modified.\nNative game acceptance for this generated candidate has not been run.\n")?;
    let inventory: BTreeMap<_, _> = files
        .iter()
        .map(|(p, b)| (p.clone(), json!({"bytes":b.len(),"sha256":sources::sha(b)})))
        .collect();
    let proof = json!({"format":1,"identity":"tte-native-generated","generation":"COMPILED RUST; no Python process/runtime","version":"1.2.0-native-test","sourceFingerprint":report["sourceFingerprint"],"technologies":techs.len(),"sourceFieldProof":weights.proof,"nonAiWeightFields":"AST IDENTICAL to final source definitions","structure":{"guiClasses":ui.gui.len(),"sharedFieldTreeClasses":6,"perTechnologyRouteWindows":0,"buttonEffects":button_count,"scriptedTriggers":trigger_count,"scriptedEffectsBeforePartition":effect_count,"localisationGetters":getter_count,"events":event_count,"effectChunks":chunks,"hiddenEventChunks":event_helpers.len()},"nativeValidation":"NOT RUN","supportedEngine":"4.5.* candidate; tested engine still required","inventory":inventory,"sourceSelection":report["selection"],"limitations":["Build-time discovery, not runtime enumeration","Unknown special conditions remain engine-controlled","Sorted event IDs require the same technology catalog; regenerate only with save backup","Imported definitions may contain upstream parser warnings"],"publication":"LOCAL ONLY; NOT UPLOADED"});
    let mut proof = proof;
    proof["researchSource"] = research_proof;
    proof["postInitEffectGraph"] = json!({"limit":4,"depth":effect_depth.0,"chain":effect_depth.1});
    proof["postInitRepeatedEffectEstimate"] = json!({"maxRepeatedExpandedNodes":eager_cost.0,"routine":eager_cost.1,"budget":1000000,"memoryBytes":"NOT INFERRED","hiddenSelectionGateways":true});
    Ok(Runtime { files, proof })
}
fn validate_localisation(
    files: &BTreeMap<String, Vec<u8>>,
    rows: &view::Localisation,
    owned_keys: &BTreeSet<String>,
) -> Result<()> {
    fn visit(v: &script::Value, parent: &str, needed: &mut BTreeSet<String>) {
        for e in v.entries() {
            let key = e.key.as_deref().unwrap_or("");
            if [
                "buttonText",
                "localization_key",
                "title",
                "desc",
                "text",
                "default",
                "custom_tooltip",
            ]
            .contains(&key)
                || (parent == "option" && key == "name")
            {
                if let Some(s) = e.value.atom() {
                    if s.starts_with("tte_") {
                        needed.insert(s.to_owned());
                    }
                }
            }
            visit(&e.value, key, needed);
        }
    }
    let mut needed = BTreeSet::new();
    for (name, bytes) in files {
        if name.ends_with(".txt") || name.ends_with(".gui") {
            let raw = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
            visit(&script::parse(raw)?, "", &mut needed);
        }
    }
    for (lang, texts) in rows {
        let missing: Vec<_> = needed
            .iter()
            .filter(|k| owned_keys.contains(*k) && !texts.contains_key(*k))
            .collect();
        if !missing.is_empty() {
            return Err(format!("Missing {lang} UI strings: {missing:?}"));
        }
        for (key, value) in texts.iter().filter(|(k, _)| owned_keys.contains(*k)) {
            let mut rest = value.as_str();
            while let Some(a) = rest.find('$') {
                let tail = &rest[a + 1..];
                let Some(b) = tail.find('$') else { break };
                let reference = tail[..b].split('|').next().unwrap();
                if owned_keys.contains(reference) && !texts.contains_key(reference) {
                    return Err(format!(
                        "Missing {lang} owned UI reference {reference} in {key}"
                    ));
                }
                rest = &tail[b + 1..];
            }
        }
    }
    Ok(())
}
pub fn write(output: &Path, mut runtime: Runtime) -> Result<Json> {
    if output.exists() {
        return Err("Generated output must be a fresh directory".into());
    }
    fs::create_dir(output).map_err(|e| format!("Create candidate directory: {e}"))?;
    let generating = output.join("tte-generating.json");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&generating)
        .and_then(|mut f| f.write_all(b"{\"identity\":\"tte-native-generated-incomplete\"}"))
        .map_err(|e| e.to_string())?;
    // Every fresh generated directory needs a distinct launcher name. Two
    // local registrations must not shadow one another by display identity.
    let identity = sources::sha(sources::path_json(&sources::canonical(output)?).as_bytes());
    let descriptor = String::from_utf8(
        runtime
            .files
            .remove("descriptor.mod")
            .ok_or("Missing descriptor")?,
    )
    .map_err(|e| e.to_string())?;
    let name = format!("Tech Tree & Research Planner - Local {}", &identity[..10]);
    let descriptor = gui::edits(
        &descriptor,
        script::parse(&descriptor)?
            .entries()
            .iter()
            .filter(|e| e.key.as_deref() == Some("name"))
            .map(|e| {
                (
                    e.span.start,
                    e.span.end,
                    format!("name={}", planning::atom(&name)),
                )
            })
            .collect(),
    )?;
    runtime
        .files
        .insert("descriptor.mod".into(), descriptor.into_bytes());
    runtime.proof["displayName"] = json!(name);
    runtime.proof["inventory"]["descriptor.mod"] = json!({"bytes":runtime.files["descriptor.mod"].len(),"sha256":sources::sha(&runtime.files["descriptor.mod"])});
    // Success manifest is written LAST. An interrupted/failed directory has no
    // completed marker and is never registered or mistaken for a usable build.
    for (name, bytes) in &runtime.files {
        let relative = sources::relative(name)?;
        let path = output.join(relative);
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        f.write_all(bytes)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    let mut proof = runtime.proof;
    proof["buildStatus"] = json!("GENERATION COMPLETE; NATIVE QA PENDING");
    crate::atomic_report::publish(
        &output.join("tte-build.json"),
        &serde_json::to_vec_pretty(&proof).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::remove_file(generating).map_err(|e| e.to_string())?;
    Ok(proof)
}
pub fn register(runtime: &Path, documents: &Path) -> Result<std::path::PathBuf> {
    let runtime = sources::canonical(runtime)?;
    let documents = sources::canonical(documents)?;
    if !documents.join("mod").is_dir() {
        return Err("Stellaris user directory needs an existing mod folder".into());
    }
    let proof: Json = serde_json::from_slice(
        &fs::read(runtime.join("tte-build.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if proof["identity"] != "tte-native-generated"
        || proof["buildStatus"] != "GENERATION COMPLETE; NATIVE QA PENDING"
    {
        return Err("Not a completed native candidate".into());
    }
    if runtime.join("tte-generating.json").exists() {
        return Err("Candidate still has an incomplete-generation marker".into());
    }
    for (name, record) in proof["inventory"]
        .as_object()
        .ok_or("Missing candidate inventory")?
    {
        let path = runtime.join(sources::relative(name)?);
        let bytes = fs::read(&path).map_err(|e| format!("Candidate file {name}: {e}"))?;
        if record["bytes"] != bytes.len() || record["sha256"] != sources::sha(&bytes) {
            return Err(format!(
                "Generated candidate changed before registration: {name}"
            ));
        }
    }
    if runtime.to_str().is_none() {
        return Err("Local registration requires a Unicode path".into());
    }
    let path = sources::path_json(&runtime);
    let hash = sources::sha(path.as_bytes());
    let registry = documents
        .join("mod")
        .join(format!("tte_native_generated_{}.mod", &hash[..16]));
    let descriptor = sources::text(&runtime.join("descriptor.mod"))?;
    if proof["inventory"]["descriptor.mod"]["sha256"] != sources::sha(descriptor.as_bytes()) {
        return Err("Generated descriptor changed before registration".into());
    }
    let meta = script::parse(&descriptor)?;
    if meta.first("path").is_some()
        || meta.first("archive").is_some()
        || meta.first("remote_file_id").is_some()
    {
        return Err("Generated local descriptor has unexpected identity/path".into());
    }
    let raw = format!(
        "{descriptor}\npath={}\n",
        serde_json::to_string(&path).map_err(|e| e.to_string())?
    );
    crate::atomic_report::publish(&registry, raw.as_bytes())
        .map_err(|e| format!("Local registration was not replaced: {e}"))?;
    println!("Registered a NEW local TEST mod: {}. Enable it AFTER the exact source mods; disable the Workshop planner. / 已登记新的本地测试 Mod，请放在来源 Mod 后，并关闭工坊规划器。",sources::path_json(&registry));
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_refuses_changed_files_and_incomplete_marker() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../build")
            .join(format!(
                "registration-unit-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(root.join("docs/mod")).unwrap();
        let files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
            ("descriptor.mod".into(), b"name=owned_test\n".to_vec()),
            (
                "events/owned.txt".into(),
                b"namespace=owned_test\n".to_vec(),
            ),
        ]);
        let inventory: BTreeMap<_, _> = files
            .iter()
            .map(|(p, b)| (p.clone(), json!({"bytes":b.len(),"sha256":sources::sha(b)})))
            .collect();
        let candidate = root.join("candidate");
        write(
            &candidate,
            Runtime {
                files,
                proof: json!({"identity":"tte-native-generated","inventory":inventory}),
            },
        )
        .unwrap();
        let registered = register(&candidate, &root.join("docs")).unwrap();
        let before = fs::read(&registered).unwrap();
        fs::write(candidate.join("events/owned.txt"), b"namespace=changed\n").unwrap();
        assert!(register(&candidate, &root.join("docs"))
            .unwrap_err()
            .contains("changed before registration"));
        assert_eq!(before, fs::read(&registered).unwrap());
        fs::write(candidate.join("tte-generating.json"), b"{}").unwrap();
        assert!(register(&candidate, &root.join("docs"))
            .unwrap_err()
            .contains("incomplete-generation"));
    }
    #[test]
    fn an_owned_translation_in_another_language_cannot_mask_a_missing_row() {
        let files = BTreeMap::from([(
            "interface/owned.gui".into(),
            b"guiTypes={instantTextBoxType={text=tte_owned}}".to_vec(),
        )]);
        let rows = BTreeMap::from([
            ("english".into(), BTreeMap::new()),
            (
                "japanese".into(),
                BTreeMap::from([("tte_owned".into(), "owned".into())]),
            ),
        ]);
        assert!(
            validate_localisation(&files, &rows, &BTreeSet::from(["tte_owned".into()]))
                .unwrap_err()
                .contains("Missing english")
        );
    }
    #[test]
    fn owned_locale_value_references_are_checked_without_replacing_native_text() {
        let files = BTreeMap::new();
        let rows = BTreeMap::from([(
            "japanese".into(),
            BTreeMap::from([("tte_owned".into(), "$tte_missing|Y$".into())]),
        )]);
        let owned = BTreeSet::from(["tte_owned".into(), "tte_missing".into()]);
        assert!(validate_localisation(&files, &rows, &owned)
            .unwrap_err()
            .contains("tte_missing"));
        // An unknown source reference is left to the engine, including raw-ID
        // fallback. It must not be mistaken for a missing owned UI string.
        assert!(
            validate_localisation(&files, &rows, &BTreeSet::from(["tte_owned".into()])).is_ok()
        );
    }
    #[test]
    fn namespace_is_declared_before_definition() {
        assert!(namespace_order(&[
            "country_event={id=owned.1}".into(),
            "namespace=owned".into()
        ])
        .is_err());
        assert!(namespace_order(&[
            "namespace=owned".into(),
            "country_event={id=owned.1}".into()
        ])
        .is_ok());
    }
}
