use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
fn zip_source(source: &Path, path: &Path) {
    use std::io::Write;
    fn visit(root: &Path, p: &Path, zip: &mut zip::ZipWriter<fs::File>) {
        for e in fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                visit(root, &p, zip);
            } else {
                zip.start_file(
                    p.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
                zip.write_all(&fs::read(p).unwrap()).unwrap();
            }
        }
    }
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    visit(source, source, &mut zip);
    zip.finish().unwrap();
}
struct Fixture {
    root: PathBuf,
    parent: PathBuf,
    game: PathBuf,
    source: PathBuf,
    docs: PathBuf,
}
fn write(path: &Path, raw: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, raw).unwrap();
}
impl Fixture {
    fn new() -> Self {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../build/native_core_tests");
        fs::create_dir_all(&parent).unwrap();
        let parent = fs::canonicalize(parent).unwrap();
        let root = parent.join(format!(
            "native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&root).unwrap();
        let game = root.join("中文 game");
        let source = root.join("source");
        let docs = root.join("Documents");
        write(
            &game.join("common/technology/base.txt"),
            "@price=10\nbase={area=physics tier=0 cost=@price category={computing}}\n",
        );
        write(
            &source.join("descriptor.mod"),
            "name=\"Unknown standard source\"",
        );
        fs::create_dir_all(docs.join("mod")).unwrap();
        let db = Connection::open(docs.join("launcher-v2.sqlite")).unwrap();
        db.execute_batch("create table playsets(id text,name text,isActive integer);create table mods(id text,gameRegistryId text,dirPath text,archivePath text,status text);create table playsets_mods(playsetId text,modId text,enabled integer,position integer);insert into playsets values('p','中文 TEST',1);").unwrap();
        drop(db);
        write(&docs.join("dlc_load.json"), "{\"enabled_mods\":[]}");
        Self {
            root,
            parent,
            game,
            source,
            docs,
        }
    }
    fn run(&self, extra: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tte-core"));
        cmd.arg("--game")
            .arg(&self.game)
            .arg("--output")
            .arg(self.root.join("result.json"));
        cmd.args(extra);
        cmd.env_remove("PYTHONPATH")
            .env("PYTHONHOME", "NO_PYTHON")
            .env("PYTHONNOUSERSITE", "1");
        cmd.output().unwrap()
    }
    fn report(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("result.json")).unwrap()).unwrap()
    }
    fn ok(&self, extra: &[&str]) -> Value {
        let p = self.run(extra);
        assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
        self.report()
    }
    fn enable(&self, registry: &str, root: &Path, position: i64, own: bool) {
        let name = if own {
            "Own planner"
        } else {
            "Unknown standard source"
        };
        let raw = if own {
            format!("name=\"{name}\" remote_file_id=\"3818261596\" archive=\"runtime.zip\"")
        } else {
            format!(
                "name=\"{name}\"\npath={}",
                json!(root.to_string_lossy().replace('\\', "/"))
            )
        };
        write(&self.docs.join(registry), &raw);
        let db = Connection::open(self.docs.join("launcher-v2.sqlite")).unwrap();
        let id = format!("mod{position}");
        db.execute(
            "insert into mods values(?1,?2,?3,null,'ready_to_play')",
            [id.as_str(), registry, root.to_str().unwrap()],
        )
        .unwrap();
        db.execute(
            "insert into playsets_mods values('p',?1,1,?2)",
            rusqlite::params![id, position],
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(path) = fs::canonicalize(&self.root) {
            assert!(
                path.starts_with(&self.parent)
                    && path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("native-")
            );
            fs::remove_dir_all(path).unwrap();
        }
    }
}

fn owned_gui_fixture(f: &Fixture) {
    write(&f.game.join("interface/owned_resources.gfx"),"spriteTypes={spriteType={name=GFX_tiling_button_standard} spriteType={name=GFX_tile_outliner_bg} spriteType={name=GFX_tile_outliner_bg_solid} spriteType={name=GFX_ncp_scroll_slider} spriteType={name=gfx_transparency_button_2}} bitmapfonts={bitmapfont={name=cg_16b}}");
    write(&f.game.join("interface/owned_scrolls.gui"),"guiTypes={extendedScrollbarType={name=bottom_horizontal_slider} extendedScrollbarType={name=right_vertical_slider} extendedScrollbarType={name=standardtext_slider} extendedScrollbarType={name=standardlistbox_slider}}");
    write(&f.game.join("interface/diplomacy_curator_event_view.gui"),"guiTypes={containerWindowType={name=enclave_curator_window size={width=974 height=680} background={name=background quadTextureSprite=old} containerWindowType={name=EVENT_DIPLO instantTextBoxType={name=action_title} instantTextBoxType={name=action_desc} listboxType={name=option_list}} instantTextBoxType={name=alien_message} buttonType={name=close} containerWindowType={name=portrait}} containerWindowType={name=enclave_curator_option size={width=388 height=30} buttonType={name=option_button text=OPTION_TEXT} overlappingElementsBoxType={name=trigger_icons}}}");
    write(&f.game.join("interface/technology_view.gui"),"guiTypes={containerWindowType={name=technology_view_window size={width=480 height=700} containerWindowType={name=select_window}}}");
    write(&f.game.join("interface/paragon_ui_types.gui"),"guiTypes={containerWindowType={name=paragon_portrait_container_research size={width=600 height=300} clipping=no iconType={name=owned_portrait spriteType=unmodified}}}");
}

#[test]
fn collector_does_not_decode_unrelated_gfx_and_build_checks_resources() {
    let f = Fixture::new();
    fs::create_dir_all(f.source.join("interface")).unwrap();
    fs::write(f.source.join("interface/old.gfx"), [b'#', 0xe9]).unwrap();
    let r = f.run(&["--mod", f.source.to_str().unwrap()]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    fs::remove_file(f.root.join("result.json")).unwrap();
    owned_gui_fixture(&f);
    fs::remove_file(f.game.join("interface/owned_resources.gfx")).unwrap();
    let r = f.run(&[
        "--experimental-build",
        f.root.join("missing_resource").to_str().unwrap(),
    ]);
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("Missing native sprite"));
    assert!(!f.root.join("missing_resource").exists());
}

#[test]
fn sparse_mod_tier_and_unrelated_gui_constants_do_not_break_generation() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(
        &f.source.join("common/technology/sparse.txt"),
        "Sparse_Unknown={area=physics tier=20 cost=10 category={computing} prerequisites={base}}",
    );
    write(&f.source.join("interface/unrelated.gui"),"@duplicate={a=1} @duplicate=2 guiTypes={containerWindowType={name=unrelated @local={x=1}}}");
    let resources = f.game.join("interface/owned_resources.gfx");
    write(
        &resources,
        &fs::read_to_string(&resources)
            .unwrap()
            .replace("spriteType={name=GFX_ncp_scroll_slider}", ""),
    );
    let built = f.root.join("sparse");
    let r = f.ok(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        built.to_str().unwrap(),
    ]);
    assert_eq!(r["technologies"]["Sparse_Unknown"]["tier"], 20);
    let gui = fs::read_dir(built.join("interface"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "gui"))
        .map(|p| fs::read_to_string(p).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(gui.contains("tier_column_20"));
    assert!(!gui.contains("tier_column_19"));
    // A matching native control with the wrong widget kind fails explicitly.
    fs::remove_file(f.root.join("result.json")).unwrap();
    let path = f.game.join("interface/technology_view.gui");
    write(
        &path,
        &fs::read_to_string(&path).unwrap().replace(
            "containerWindowType={name=technology_view_window",
            "buttonType={name=technology_view_window",
        ),
    );
    let p = f.run(&[
        "--experimental-build",
        f.root.join("wrong_kind").to_str().unwrap(),
    ]);
    assert!(!p.status.success());
    assert!(String::from_utf8_lossy(&p.stderr).contains("expected containerWindowType"));
    assert!(!f.root.join("wrong_kind").exists());
}

#[test]
fn purpose_and_unlocks_follow_unlisted_source_changes_without_changing_rules() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(&f.source.join("common/technology/unlisted.txt"),"@bonus=0.15\nnew_resource={area=engineering tier=1 cost=40 modifier={volatile_motes_produces_mult=@bonus} prereqfor_desc={title=owned_tooltip}} ");
    write(
        &f.game.join("common/buildings/authored.txt"),
        "chemical_plant={prerequisites={base} allow={has_technology=new_resource}} ",
    );
    // Same relative object file must be replaced by the selected source layer.
    write(
        &f.source.join("common/buildings/authored.txt"),
        "chemical_plant={prerequisites={new_resource} allow={has_technology=base}} ",
    );
    write(&f.game.join("localisation/english/authored_l_english.yml"),"l_english:\n chemical_plant:0 \"Chemical Plants\"\n MOD_VOLATILE_MOTES_PRODUCES_MULT:0 \"Mote output\"\n");
    write(
        &f.game.join("common/armies/shared.txt"),
        "shared_key={prerequisites={base}}",
    );
    write(
        &f.game.join("common/buildings/shared.txt"),
        "shared_key={prerequisites={new_resource}}",
    );
    let built = f.root.join("generated");
    let r = f.ok(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        built.to_str().unwrap(),
    ]);
    assert_eq!(
        r["browseMetadata"]["new_resource"]["unlocks"][0]["key"],
        "chemical_plant"
    );
    assert!(r["browseMetadata"]["base"]["unlocks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["key"] == "shared_key" && u["kind"] == "army"));
    assert!(r["browseMetadata"]["new_resource"]["unlocks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["key"] == "shared_key" && u["kind"] == "building"));
    assert!(!r["browseMetadata"]["base"]["unlocks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["key"] == "chemical_plant"));
    assert_eq!(
        r["browseMetadata"]["new_resource"]["effects"][0],
        "$MOD_VOLATILE_MOTES_PRODUCES_MULT$ +15%"
    );
    assert!(r["browseMetadata"]["new_resource"]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "strategic"));
    let en =
        fs::read_to_string(built.join("localisation/english/tte_native_l_english.yml")).unwrap();
    assert!(en.contains("$chemical_plant$"));
    assert!(en.contains("Unlocks"));
    assert!(en.contains("Browse by purpose"));
    assert!(r["browseMetadata"]["new_resource"]["unlocks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["key"] == "owned_tooltip"));
    // A never-listed definition and its relationship change need no code edits.
    fs::remove_file(f.root.join("result.json")).unwrap();
    write(
        &f.source.join("common/buildings/authored.txt"),
        "chemical_plant={prerequisites={base}} ",
    );
    let r = f.ok(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        f.root.join("changed").to_str().unwrap(),
    ]);
    assert!(r["browseMetadata"]["base"]["unlocks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["key"] == "chemical_plant"));
    assert!(!r["browseMetadata"]["new_resource"]["unlocks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["key"] == "chemical_plant"));
}

#[test]
fn language_output_uses_source_locale_and_only_fills_missing_references() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(&f.game.join("localisation/english/owned_l_english.yml"),"l_english:\n base:0 \"English Base\"\n base_desc:0 \"$owned_nested$\"\n owned_nested:0 \"English fallback detail\"\n computing:0 \"Computing\"\n");
    write(
        &f.source.join("localisation/japanese/owned_l_japanese.yml"),
        "l_japanese:\n base:0 \"出典の技術\"\n computing:0 \"出典の分類\"\n",
    );
    let built = f.root.join("locales");
    let r = f.ok(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        built.to_str().unwrap(),
    ]);
    assert_eq!(r["localisation"]["japanese"]["base"]["name"], "出典の技術");
    for language in [
        "japanese", "korean", "russian", "french", "german", "spanish", "polish", "braz_por",
    ] {
        assert!(built
            .join(format!(
                "localisation/{language}/tte_native_l_{language}.yml"
            ))
            .is_file());
    }
    let ja =
        fs::read_to_string(built.join("localisation/japanese/tte_native_l_japanese.yml")).unwrap();
    assert!(ja.contains("出典の技術"));
    assert!(!ja.lines().any(|l| l.starts_with(" base:0")));
    assert!(!ja.lines().any(|l| l.starts_with(" computing:0")));
    assert!(ja.contains(" owned_nested:0 \"English fallback detail\""));
    assert!(ja.contains("§G出典の技術§!"));
    assert!(ja.contains("edict_tte_open_planner:0 \"$tte_open_planner_label$\""));
    let zh =
        fs::read_to_string(built.join("localisation/simp_chinese/tte_native_l_simp_chinese.yml"))
            .unwrap();
    assert!(
        zh.contains("§GEnglish Base§!"),
        "Missing Chinese source names must fall back to English, not raw IDs"
    );
}

#[test]
fn inline_tier_definitions_are_collected_before_tier_eligibility() {
    let f = Fixture::new();
    write(
        &f.source.join("common/inline_scripts/tiers/extra.txt"),
        "6={previously_unlocked=8}",
    );
    write(
        &f.source.join("common/technology/tier/extra.txt"),
        "inline_script=tiers/extra",
    );
    write(
        &f.source.join("common/technology/extra.txt"),
        "tier_probe={area=physics tier=6 cost=30}",
    );
    let r = f.ok(&["--mod", f.source.to_str().unwrap()]);
    assert_eq!(r["technologies"]["tier_probe"]["tierDefined"], true);
}

#[test]
fn uncertain_potentials_and_missing_refs_never_become_permanent_false() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(
        &f.source.join("common/scripted_triggers/owned.txt"),
        "owned_gate={[[ALT] has_origin=origin_a ] [[!ALT] has_origin=origin_b ]}\n",
    );
    write(&f.source.join("common/technology/owned.txt"),"conditional={area=physics tier=1 cost=20 potential={owned_gate=yes}} duplicate={area=physics tier=1 cost=20 potential={has_origin=origin_a} potential={always=yes}} direct_macro={area=physics tier=1 cost=20 potential={[[ALT]has_origin=origin_a]}} unknown_ref={area=physics tier=1 cost=20 prerequisites={display_only}} ");
    let built = f.root.join("uncertainty");
    let r = f.ok(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        built.to_str().unwrap(),
    ]);
    assert!(r["technologies"]["duplicate"]["ambiguousOptionalFields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "potential"));
    assert_eq!(
        r["technologies"]["unknown_ref"]["missingPrerequisites"][0],
        "display_only"
    );
    let effects = fs::read_dir(built.join("common/scripted_effects"))
        .unwrap()
        .map(|p| fs::read_to_string(p.unwrap().path()).unwrap())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        !effects.contains("has_origin"),
        "Unknown branches must not turn into exact false upper bounds"
    );
    assert!(!effects.contains("tte_possible_display_only"));
    let triggers = fs::read_dir(built.join("common/scripted_triggers"))
        .unwrap()
        .map(|p| fs::read_to_string(p.unwrap().path()).unwrap())
        .collect::<Vec<_>>()
        .join(" ");
    let own = triggers
        .split("tte_offered_unknown_ref =")
        .nth(1)
        .unwrap()
        .split("tte_")
        .next()
        .unwrap();
    assert!(own.contains("has_tech_option = unknown_ref"));
    assert!(!own.contains("always = no"));
}

#[test]
fn report_variable_tables_only_include_used_transitive_dependencies() {
    let f = Fixture::new();
    let globals = (0..2000)
        .map(|i| format!("@unused{i}={i}\n"))
        .collect::<String>();
    write(
        &f.game.join("common/scripted_variables/global.txt"),
        &(globals + "@needed=19\n@alias=@needed\n"),
    );
    write(
        &f.game.join("common/technology/base.txt"),
        "base={area=physics tier=0 cost=@alias} second={area=society tier=1 cost=20}",
    );
    let r = f.ok(&[]);
    assert_eq!(r["technologies"]["base"]["cost"], 19.0);
    assert_eq!(
        r["technologies"]["base"]["scriptedVariables"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert!(r["technologies"]["second"]["scriptedVariables"]
        .as_object()
        .unwrap()
        .is_empty());
    assert!(fs::metadata(f.root.join("result.json")).unwrap().len() < 20000);
}

#[test]
fn large_catalog_each_event_file_declares_its_namespaces() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    let s = (0..180)
        .map(|i| format!("Unknown_{i}={{area=physics tier=1 cost=10}}"))
        .collect::<Vec<_>>()
        .join("\n");
    write(&f.source.join("common/technology/large.txt"), &s);
    let target = f.root.join("large_candidate");
    let r = f.run(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        target.to_str().unwrap(),
    ]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let events = fs::read_dir(target.join("events"))
        .unwrap()
        .map(|p| fs::read_to_string(p.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    assert!(
        events.len() > 1,
        "Must exercise partitioning, not a single small fixture"
    );
    for raw in events {
        for ns in [
            "tteaux",
            "tteview",
            "ttepreviewsmall",
            "ttepreviewlarge",
            "tteselect",
            "ttehistory",
        ] {
            assert!(raw.starts_with("namespace="));
            assert!(raw.contains(&format!("namespace={ns}\n")), "Missing {ns}");
        }
    }
}

#[test]
fn full_native_candidate_discovers_an_unlisted_technology_and_modified_parent() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(&f.source.join("common/technology/extra.txt"),"New_Unknown={area=engineering tier=1 cost=321 prerequisites={base} levels=-1 ai_weight={weight=2}} Other_Unknown={area=society tier=1 cost=4}");
    let first = f.root.join("candidate_a");
    let result = f.run(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        first.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        !first
            .join("interface/zzzz_tte_native_portrait.gui")
            .exists(),
        "Preserve the source UI's scientist portrait"
    );
    let proof: Value =
        serde_json::from_slice(&fs::read(first.join("tte-build.json")).unwrap()).unwrap();
    assert_eq!(proof["technologies"], 3);
    assert_eq!(proof["structure"]["sharedFieldTreeClasses"], 6);
    assert_eq!(proof["structure"]["perTechnologyRouteWindows"], 0);
    assert_eq!(proof["sourceFieldProof"].as_array().unwrap().len(), 3);
    assert_eq!(proof["nativeValidation"], "NOT RUN");
    let tree = fs::read_dir(first.join("interface"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "gui"))
        .map(|p| fs::read_to_string(p).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(tree.contains("New_Unknown"));
    assert!(!tree.contains("horizontalScrollbar"));
    assert!(tree.contains("tier_column_5"));
    assert!(tree.contains("verticalScrollbar"));
    // Card clicks must only highlight in the existing tree. Opening a native
    // visible event from a GUI effect button stacks windows and loses scrolling.
    let buttons = fs::read_dir(first.join("common/button_effects"))
        .unwrap()
        .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(buttons.contains("hidden_effect={tte_select_node_New_Unknown=yes}"));
    assert!(tree.contains("effect=tte_node_New_Unknown"));
    assert!(tree.contains("[From.GetTTECard0_New_Unknown]"));
    assert!(!tree.contains("node_locked_New_Unknown"));
    assert_eq!(fs::read_dir(f.docs.join("mod")).unwrap().count(), 0);
    let report = f.report();
    assert_eq!(
        report["technologies"]["New_Unknown"]["condition"],
        json!(["tech", "base"])
    );
    fs::remove_file(f.root.join("result.json")).unwrap();
    write(&f.source.join("common/technology/extra.txt"),"New_Unknown={area=engineering tier=1 cost=321 prerequisites={Other_Unknown} levels=-1 ai_weight={weight=2}} Other_Unknown={area=society tier=1 cost=4}");
    let second = f.root.join("candidate_b");
    let result = f.run(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        second.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        f.report()["technologies"]["New_Unknown"]["condition"],
        json!(["tech", "Other_Unknown"])
    );
    let effects = fs::read_dir(second.join("events"))
        .unwrap()
        .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(effects.contains("tte_view_visit_Other_Unknown=yes"));
}

#[test]
fn explicit_registration_creates_one_new_file_without_changing_playset() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    let before = fs::read(f.docs.join("launcher-v2.sqlite")).unwrap();
    let selection = fs::read(f.docs.join("dlc_load.json")).unwrap();
    let output = f.root.join("local_candidate");
    let result = f.run(&[
        "--experimental-build",
        output.to_str().unwrap(),
        "--register-local",
        f.docs.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let entries: Vec<_> = fs::read_dir(f.docs.join("mod"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1);
    let text = fs::read_to_string(&entries[0]).unwrap();
    assert!(text.contains("path="));
    assert!(!text.contains("archive="));
    assert!(!text.contains("remote_file_id"));
    assert_eq!(before, fs::read(f.docs.join("launcher-v2.sqlite")).unwrap());
    assert_eq!(selection, fs::read(f.docs.join("dlc_load.json")).unwrap());
}

#[test]
fn failed_build_never_registers_or_claims_native_acceptance() {
    let f = Fixture::new();
    let output = f.root.join("failed_candidate");
    let result = f.run(&[
        "--experimental-build",
        output.to_str().unwrap(),
        "--register-local",
        f.docs.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(!output.exists());
    assert_eq!(fs::read_dir(f.docs.join("mod")).unwrap().count(), 0);
    assert!(!f.root.join("result.json").exists());
}

#[test]
fn source_category_and_quoted_ai_weight_cannot_inject_generated_commands() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(&f.game.join("common/technology/base.txt"),"base={area=physics tier=0 cost=10 category={\"x hidden_effect={add_resource={energy=99999}}\"} ai_weight={weight=\"1 modifier={factor=0 always=yes}\"}}");
    let output = f.root.join("guarded");
    let result = f.run(&["--experimental-build", output.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let events = fs::read_dir(output.join("events"))
        .unwrap()
        .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!events.contains("add_resource"));
    for entry in fs::read_dir(output.join("localisation")).unwrap() {
        for path in fs::read_dir(entry.unwrap().path()).unwrap() {
            let text = fs::read_to_string(path.unwrap().path()).unwrap();
            assert!(!text.contains("add_resource"));
        }
    }
    let weights = fs::read_to_string(output.join("common/technology/base.txt")).unwrap();
    assert!(weights.contains("factor = \"1 modifier={factor=0 always=yes}\""));
}

#[test]
fn different_filename_ui_definition_is_found_and_duplicates_fail_closed() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    let original = fs::read_to_string(f.game.join("interface/technology_view.gui")).unwrap();
    fs::remove_file(f.game.join("interface/technology_view.gui")).unwrap();
    let source = format!(
        "@owned_constant=37\n{}",
        original.replace(
            "guiTypes={",
            "guiTypes={positionType={name=owned_sibling position={x=@owned_constant y=2}} "
        )
    );
    write(&f.source.join("interface/custom_ui.gui"), &source);
    let output = f.root.join("custom_ui");
    let result = f.run(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        output.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let research = fs::read_to_string(output.join("interface/custom_ui.gui")).unwrap();
    assert!(research.contains("tte_open_tree"));
    assert!(research.contains("@owned_constant=37"));
    assert!(research.contains("name=owned_sibling position={x=@owned_constant y=2}"));
    assert!(!output
        .join("interface/zzzz_tte_native_research.gui")
        .exists());
    fs::remove_file(f.root.join("result.json")).unwrap();
    write(&f.game.join("interface/technology_view.gui"), &original);
    let other = f.root.join("ambiguous");
    let result = f.run(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        other.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("cross-file precedence is not guessed")
    );
    assert!(!other.exists());
}

#[test]
fn file_local_swap_variables_are_resolved_into_generated_rules() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(&f.game.join("common/technology/base.txt"),"@origin=origin_owned_fixture @price=10 base={area=physics tier=0 cost=@price technology_swap={name=owned_variant area=society trigger={has_origin=@origin}}}");
    let output = f.root.join("variables");
    let result = f.run(&["--experimental-build", output.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let triggers = fs::read_dir(output.join("common/scripted_triggers"))
        .unwrap()
        .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(triggers.contains("has_origin = origin_owned_fixture"));
    assert!(!triggers.contains("@origin"));
}

#[test]
fn traditional_preference_shares_layouts_and_protects_native_keys() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(&f.game.join("localisation/simp_chinese/owned_l_simp_chinese.yml"),"l_simp_chinese:\n base:0 \"头发研究\"\n base_desc:0 \"$owned_nested$ [Root.GetName] £physics£\"\n owned_nested:0 \"后台发展\"\n computing:0 \"计算技术\"\n");
    write(&f.game.join("localisation/english/owned_l_english.yml"),"l_english:\n base:0 \"English Base\"\n base_desc:0 \"$owned_nested$ [Root.GetName] £physics£\"\n owned_nested:0 \"English detail\"\n computing:0 \"Computing\"\n");
    let built = f.root.join("traditional");
    f.ok(&["--experimental-build", built.to_str().unwrap()]);
    let proof: Value =
        serde_json::from_slice(&fs::read(built.join("tte-build.json")).unwrap()).unwrap();
    assert_eq!(proof["structure"]["sharedFieldTreeClasses"], 6);
    let zh =
        fs::read_to_string(built.join("localisation/simp_chinese/tte_native_l_simp_chinese.yml"))
            .unwrap();
    assert!(zh.contains("tte_hant_base:0 \"頭髮研究\""));
    assert!(zh.contains("$tte_hant_owned_nested$ [Root.GetName] £physics£"));
    assert!(zh.contains("tte_hant_owned_nested:0 \"後臺發展\""));
    assert!(!zh.contains("\n base:0"));
    let en =
        fs::read_to_string(built.join("localisation/english/tte_native_l_english.yml")).unwrap();
    assert!(en.contains("tte_hant_base:0 \"$base$\""));
    let read_folder = |name: &str| {
        fs::read_dir(built.join(name))
            .unwrap()
            .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let events = read_folder("events");
    assert!(events.contains("set_country_flag=tte105_traditional_chinese"));
    assert!(events.contains("remove_country_flag=tte105_traditional_chinese"));
    let getters = read_folder("common/scripted_loc");
    assert!(!getters.contains("name=GetTTEGoalEntry"));
    assert!(getters.contains("localization_key=tte_hant_tte_caption_done_base"));
    assert!(getters.contains("default=tte_caption_locked_base"));
    let gui = read_folder("interface");
    assert!(!gui.contains("tte105h_tte_graph"));
    assert!(gui.contains("[From.GetTTEText_"));
    assert!(gui.contains("name=tte_advice_heading"));
    assert!(zh.contains("tte_hant_computing:0 \"計算技術\""));
    assert!(gui.contains("alwaysTransparent=yes"));
}

#[test]
fn chinese_only_keys_do_not_abort_and_native_category_widgets_are_untouched() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(&f.game.join("localisation/simp_chinese/only_l_simp_chinese.yml"),"l_simp_chinese:\n base:0 \"只有中文科技\"\n base_desc:0 \"发展经济\"\n computing:0 \"计算技术\"\n");
    let p = f.game.join("interface/technology_view.gui");
    let raw=fs::read_to_string(&p).unwrap().replace("containerWindowType={name=select_window}","containerWindowType={name=select_window} instantTextBoxType={name=category_native text=\"$base$\" maxWidth=99 alwaysTransparent=yes}");
    write(&p, &raw);
    let built = f.root.join("chinese_only");
    f.ok(&["--experimental-build", built.to_str().unwrap()]);
    let patched = fs::read_to_string(built.join("interface/technology_view.gui")).unwrap();
    assert!(patched.contains("instantTextBoxType={name=category_native text=\"$base$\" maxWidth=99 alwaysTransparent=yes}"));
    let zh =
        fs::read_to_string(built.join("localisation/simp_chinese/tte_native_l_simp_chinese.yml"))
            .unwrap();
    assert!(zh.contains("tte_hant_computing:0 \"計算技術\""));
}
#[test]
fn source_technology_with_planner_like_prefix_keeps_its_native_identity() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(
        &f.source.join("common/technology/owned.txt"),
        "tte_other_source={area=physics tier=0 cost=12 category={computing}}\ntte_unlocalised_source={area=physics tier=0 cost=12 category={computing}}",
    );
    write(
        &f.source
            .join("localisation/simp_chinese/owned_l_simp_chinese.yml"),
        "l_simp_chinese:\n tte_other_source:0 \"其他来源的科技\"\n",
    );
    let built = f.root.join("prefix");
    let r = f.ok(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        built.to_str().unwrap(),
    ]);
    assert_eq!(
        r["technologies"]["tte_other_source"]["id"],
        "tte_other_source"
    );
    assert_eq!(
        r["technologies"]["tte_unlocalised_source"]["id"],
        "tte_unlocalised_source"
    );
}

#[test]
fn unlisted_inline_and_local_variables_need_no_hand_list() {
    let f = Fixture::new();
    write(
        &f.source.join("common/technology/new.txt"),
        "@price=17\nNeverListed_ABC={inline_script={script=probe name=base} tier=1 cost=@price}",
    );
    write(
        &f.source.join("common/inline_scripts/probe.txt"),
        "area=engineering category={industry} prerequisites={$name$}",
    );
    let v = f.ok(&["--mod", f.source.to_str().unwrap()]);
    assert_eq!(
        v["technologies"]["NeverListed_ABC"]["condition"],
        json!(["tech", "base"])
    );
    assert_eq!(v["technologies"]["NeverListed_ABC"]["cost"], 17.0);
    assert_eq!(v["sources"][1]["effectiveTechnologies"], 1);
    assert_eq!(v["capabilities"]["gameScriptGeneration"], false);
}
#[test]
fn source_override_and_replace_path_are_applied_before_graph() {
    let f = Fixture::new();
    write(
        &f.source.join("descriptor.mod"),
        "name=\"Unknown standard source\" replace_path=\"common/technology\"",
    );
    write(
        &f.source.join("common/technology/new.txt"),
        "new={area=society tier=0 cost=5}",
    );
    let v = f.ok(&["--mod", f.source.to_str().unwrap()]);
    assert!(v["technologies"].get("base").is_none());
    assert_eq!(v["technologies"].as_object().unwrap().len(), 1);
    assert_eq!(v["replacePaths"][0]["removedFiles"], 1);
}
#[test]
fn swapping_file_uses_winning_cost_and_tracks_source() {
    let f = Fixture::new();
    write(
        &f.source.join("common/technology/base.txt"),
        "base={area=physics tier=0 cost=123}",
    );
    let v = f.ok(&["--mod", f.source.to_str().unwrap()]);
    assert_eq!(v["technologies"]["base"]["cost"], 123.0);
    assert_eq!(v["technologies"]["base"]["sourceLayer"], 1);
    assert_eq!(v["fileCollisions"].as_array().unwrap().len(), 1);
}
#[test]
fn missing_and_alias_references_stay_unknown() {
    let f = Fixture::new();
    write(&f.source.join("common/technology/extra.txt"),"owner={area=society cost=1 technology_swap={name=display_only trigger={is_ai=no}}} child={area=engineering cost=1 prerequisites={OR={display_only base}}}");
    let v = f.ok(&["--mod", f.source.to_str().unwrap()]);
    assert!(v["technologies"].get("display_only").is_none());
    assert_eq!(
        v["technologies"]["child"]["missingPrerequisites"],
        json!(["display_only"])
    );
    assert_eq!(
        v["technologyAliases"]["byName"]["display_only"][0]["engineLookupVerified"],
        false
    );
}
#[test]
fn cycles_and_malformed_input_do_not_write_result() {
    let f = Fixture::new();
    write(
        &f.source.join("common/technology/extra.txt"),
        "a={area=physics prerequisites={b}} b={area=engineering prerequisites={a}}",
    );
    let p = f.run(&["--mod", f.source.to_str().unwrap()]);
    assert!(!p.status.success());
    assert!(!f.root.join("result.json").exists());
}
#[test]
fn inline_cycle_is_rejected() {
    let f = Fixture::new();
    write(
        &f.source.join("common/technology/extra.txt"),
        "a={inline_script=loop}",
    );
    write(
        &f.source.join("common/inline_scripts/loop.txt"),
        "inline_script=loop",
    );
    let p = f.run(&["--mod", f.source.to_str().unwrap()]);
    assert!(!p.status.success());
    assert!(String::from_utf8_lossy(&p.stderr).contains("cycle"));
}
#[test]
fn playset_reads_only_enabled_sources_and_excludes_own_zip() {
    let f = Fixture::new();
    f.enable("mod/own.mod", &f.root.join("does_not_exist"), 0, true);
    f.enable("mod/source.mod", &f.source, 1, false);
    write(
        &f.source.join("common/technology/extra.txt"),
        "novel={area=engineering tier=1 cost=20 prerequisites={base}}",
    );
    let db_path = f.docs.join("launcher-v2.sqlite");
    let before = fs::read(&db_path).unwrap();
    let load = fs::read(f.docs.join("dlc_load.json")).unwrap();
    let v = f.ok(&["--documents", f.docs.to_str().unwrap()]);
    assert_eq!(v["technologies"].as_object().unwrap().len(), 2);
    assert_eq!(v["selection"]["excluded"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read(db_path).unwrap(), before);
    assert_eq!(fs::read(f.docs.join("dlc_load.json")).unwrap(), load);
}
#[test]
fn archived_source_generates_full_planner_and_follows_unlisted_changes() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(
        &f.source.join("common/technology/new.txt"),
        "inline_script={script=novel id=Unlisted_Zip prerequisite=base}",
    );
    write(
        &f.source.join("common/inline_scripts/novel.txt"),
        "$id$={area=physics tier=1 cost=33 prerequisites={$prerequisite$}}",
    );
    write(
        &f.source.join("localisation/english/new_l_english.yml"),
        "l_english:\n Unlisted_Zip:0 \"Archive discovery\"\n",
    );
    write(
        &f.source
            .join("gfx/interface/icons/technologies/Unlisted_Zip.dds"),
        "opaque authored asset",
    );
    let dir = f.root.join("directory_build");
    let a = f.ok(&[
        "--mod",
        f.source.to_str().unwrap(),
        "--experimental-build",
        dir.to_str().unwrap(),
    ]);
    fs::remove_file(f.root.join("result.json")).unwrap();
    let archive = f.root.join("source.zip");
    zip_source(&f.source, &archive);
    let built = f.root.join("zip_build");
    let b = f.ok(&[
        "--mod",
        archive.to_str().unwrap(),
        "--experimental-build",
        built.to_str().unwrap(),
    ]);
    assert_eq!(a["technologies"], b["technologies"]);
    assert_eq!(a["localisation"], b["localisation"]);
    assert_eq!(b["sources"][1]["kind"], "zip");
    assert_eq!(b["sources"][1]["archiveSha256"].as_str().unwrap().len(), 64);
    let gfx = fs::read_dir(built.join("interface"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|s| s == "gfx"))
        .map(|p| fs::read_to_string(p).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(gfx.contains("gfx/interface/icons/technologies/Unlisted_Zip.dds"));
    assert!(built.join("tte-build.json").is_file());
    assert!(built.join("common/scripted_effects").is_dir());
    // Change source data only, never the generator or an ID allowlist.
    fs::remove_file(f.root.join("result.json")).unwrap();
    write(
        &f.source.join("common/technology/new.txt"),
        "New_After_Update={area=engineering tier=2 cost=44 prerequisites={base}}",
    );
    zip_source(&f.source, &archive);
    let changed = f.ok(&["--mod", archive.to_str().unwrap()]);
    assert!(changed["technologies"].get("Unlisted_Zip").is_none());
    assert_eq!(
        changed["technologies"]["New_After_Update"]["condition"],
        json!(["tech", "base"])
    );
    fs::remove_file(f.root.join("result.json")).unwrap();
    write(&f.source.join("common/technology/new.txt"),"Anchor={area=engineering tier=1 cost=25 prerequisites={base}} New_After_Update={area=engineering tier=2 cost=44 prerequisites={Anchor}}");
    zip_source(&f.source, &archive);
    let changed = f.ok(&["--mod", archive.to_str().unwrap()]);
    assert_eq!(
        changed["technologies"]["New_After_Update"]["condition"],
        json!(["tech", "Anchor"])
    );
}
#[test]
fn launcher_archives_are_read_only_and_stale_directory_is_not_guessed() {
    let f = Fixture::new();
    let archive = f.root.join("source.zip");
    zip_source(&f.source, &archive);
    f.enable("mod/zip.mod", &f.source, 0, false);
    write(
        &f.docs.join("mod/zip.mod"),
        &format!(
            "name=\"Unknown standard source\" archive={}",
            json!(archive.to_string_lossy().replace('\\', "/"))
        ),
    );
    let db = Connection::open(f.docs.join("launcher-v2.sqlite")).unwrap();
    db.execute(
        "update mods set dirPath=null,archivePath=?1",
        [archive.to_str().unwrap()],
    )
    .unwrap();
    drop(db);
    let before = fs::read(f.docs.join("launcher-v2.sqlite")).unwrap();
    let registration = fs::read(f.docs.join("mod/zip.mod")).unwrap();
    let b = f.ok(&["--documents", f.docs.to_str().unwrap()]);
    assert_eq!(b["sources"][1]["kind"], "zip");
    assert_eq!(fs::read(f.docs.join("launcher-v2.sqlite")).unwrap(), before);
    assert_eq!(fs::read(f.docs.join("mod/zip.mod")).unwrap(), registration);
    fs::remove_file(f.root.join("result.json")).unwrap();
    let db = Connection::open(f.docs.join("launcher-v2.sqlite")).unwrap();
    db.execute("update mods set dirPath=?1", [f.source.to_str().unwrap()])
        .unwrap();
    drop(db);
    let failure = f.run(&["--documents", f.docs.to_str().unwrap()]);
    assert!(!failure.status.success());
    assert!(String::from_utf8_lossy(&failure.stderr).contains("conflicting path/archive"));
}
#[test]
fn duplicate_order_is_never_guessed() {
    let f = Fixture::new();
    f.enable("mod/a.mod", &f.source, 0, false);
    let db = Connection::open(f.docs.join("launcher-v2.sqlite")).unwrap();
    db.execute("insert into playsets_mods values('p','mod0',1,0)", [])
        .unwrap();
    drop(db);
    assert!(!f
        .run(&["--documents", f.docs.to_str().unwrap()])
        .status
        .success());
}
#[test]
fn existing_output_is_not_overwritten() {
    let f = Fixture::new();
    write(&f.root.join("result.json"), "keep me");
    assert!(!f.run(&[]).status.success());
    assert_eq!(
        fs::read_to_string(f.root.join("result.json")).unwrap(),
        "keep me"
    );
}
#[test]
fn references_and_source_files_are_data_not_commands() {
    let f = Fixture::new();
    let raw = "base={area=physics tier=0 cost=10 effect={log=\"$(create badfile)\"}}";
    write(&f.game.join("common/technology/base.txt"), raw);
    let v = f.ok(&[]);
    assert_eq!(v["technologies"]["base"]["cost"], 10.0);
    assert_eq!(
        fs::read_to_string(f.game.join("common/technology/base.txt")).unwrap(),
        raw
    );
}
#[test]
fn localisation_replace_layer_and_dollars() {
    let f = Fixture::new();
    write(
        &f.game.join("localisation/english/base_l_english.yml"),
        "\u{feff}l_english:\n base:0 \"$common$ name\"\n common:0 \"Initial\"\n",
    );
    write(
        &f.source
            .join("localisation/english/replace/a_l_english.yml"),
        "\u{feff}l_english:\n common:0 \"Override\"\n",
    );
    let v = f.ok(&["--mod", f.source.to_str().unwrap()]);
    assert_eq!(
        v["localisation"]["english"]["base"]["name"],
        "Override name"
    );
}
#[test]
fn source_tree_is_unchanged_after_collection() {
    let f = Fixture::new();
    let files = BTreeMap::from([(
        f.game.join("common/technology/base.txt"),
        fs::read(f.game.join("common/technology/base.txt")).unwrap(),
    )]);
    f.ok(&[]);
    for (path, bytes) in files {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn wizard_does_not_silently_omit_source_mod_selection() {
    use std::{io::Write, process::Stdio};
    let f = Fixture::new();
    f.enable("mod/source.mod", &f.source, 0, false);
    write(
        &f.source.join("common/technology/new.txt"),
        "unlisted={area=society tier=0 cost=9}",
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_tte-core"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}\n{}\n\n{}",
        f.game.display(),
        f.docs.display(),
        f.root.join("result.json").display()
    )
    .unwrap();
    let p = child.wait_with_output().unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    assert_eq!(f.report()["technologies"].as_object().unwrap().len(), 2);
    assert!(String::from_utf8_lossy(&p.stderr).contains("NO source mods"));
}

fn automatic_wizard(f: &Fixture, output: &Path) -> std::process::Child {
    use std::process::Stdio;
    Command::new(env!("CARGO_BIN_EXE_tte-core"))
        .args(["--build-wizard", "--game"])
        .arg(&f.game)
        .arg("--documents")
        .arg(&f.docs)
        .arg("--output-parent")
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn workshop_fixture(f: &Fixture) -> (PathBuf, PathBuf) {
    let root = f.root.join("中文 Workshop/3818261596");
    let original = f.root.join("authored original planner");
    write(
        &original.join("descriptor.mod"),
        "name=\"Tech Tree & Research Planner\"\nremote_file_id=\"3818261596\"\nversion=\"1.2.0\"\n",
    );
    write(&original.join("events/old.txt"), "old=yes");
    write(&original.join("thumbnail.png"), "authored picture bytes");
    fs::create_dir_all(&root).unwrap();
    zip_source(&original, &root.join("tte_runtime.zip"));
    let bytes = fs::read(root.join("tte_runtime.zip")).unwrap();
    use sha2::{Digest, Sha256};
    write(&root.join("technology-adapter/WORKSHOP-PACKAGE.json"),&json!({"kind":"tte-workshop-adapter","itemId":"3818261596","archive":"tte_runtime.zip","archiveSha256":format!("{:x}",Sha256::digest(&bytes))}).to_string());
    let exe = root
        .join("technology-adapter/test-host")
        .join(if cfg!(windows) {
            "tte-core.exe"
        } else {
            "tte-core"
        });
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_tte-core"), &exe).unwrap();
    f.enable("mod/own.mod", &root, 99, true);
    (root, exe)
}

fn workshop_wizard(f: &Fixture, exe: &Path, output: &str) -> std::process::Child {
    Command::new(exe)
        .arg("--workshop-wizard")
        .arg("--game")
        .arg(&f.game)
        .arg("--documents")
        .arg(&f.docs)
        .arg("--output-parent")
        .arg(f.root.join(output))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

fn answer_enter(mut child: std::process::Child) -> Output {
    send_input(&mut child, b"\n");
    child.wait_with_output().unwrap()
}
fn send_input(child: &mut std::process::Child, input: &[u8]) {
    use std::io::Write;
    if let Err(e) = child.stdin.take().unwrap().write_all(input) {
        // An intentionally rejected argument/marker can exit before stdin is
        // written on Unix. Assertions still check its nonzero result/output.
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe);
    }
}

#[test]
fn same_workshop_mod_updates_in_place_and_refreshes_unlisted_prerequisites() {
    use std::io::Read;
    let f = Fixture::new();
    owned_gui_fixture(&f);
    write(
        &f.source.join("common/technology/new.txt"),
        "NeverListed={area=physics tier=1 cost=22 prerequisites={base}}",
    );
    f.enable("mod/source.mod", &f.source, 0, false);
    let (root, exe) = workshop_fixture(&f);
    let old = fs::read(root.join("tte_runtime.zip")).unwrap();
    let descriptor = fs::read(f.root.join("authored original planner/descriptor.mod")).unwrap();
    let db = fs::read(f.docs.join("launcher-v2.sqlite")).unwrap();
    let dlc = fs::read(f.docs.join("dlc_load.json")).unwrap();
    let registrations = fs::read_dir(f.docs.join("mod")).unwrap().count();
    for (output, parent) in [("first work", "base"), ("second work", "OtherNewParent")] {
        if parent != "base" {
            write(&f.source.join("common/technology/new.txt"),"OtherNewParent={area=physics tier=0 cost=5} NeverListed={area=physics tier=1 cost=22 prerequisites={OtherNewParent}}");
        }
        let result = answer_enter(workshop_wizard(&f, &exe, output));
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report_path = fs::read_dir(f.root.join(output))
            .unwrap()
            .map(|r| r.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "json"))
            .unwrap();
        let report: Value = serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
        assert_eq!(
            report["technologies"]["NeverListed"]["condition"],
            json!(["tech", parent])
        );
        assert_eq!(fs::read(f.docs.join("launcher-v2.sqlite")).unwrap(), db);
        assert_eq!(fs::read(f.docs.join("dlc_load.json")).unwrap(), dlc);
        assert_eq!(
            fs::read_dir(f.docs.join("mod")).unwrap().count(),
            registrations
        );
        let mut zip =
            zip::ZipArchive::new(fs::File::open(root.join("tte_runtime.zip")).unwrap()).unwrap();
        let mut actual = Vec::new();
        zip.by_name("descriptor.mod")
            .unwrap()
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(actual, descriptor);
        assert!(zip.by_name("events/old.txt").is_err());
        assert!(zip.by_name("interface/technology_view.gui").is_ok());
        assert!(zip.by_name("tte-adapter-state.json").is_ok());
    }
    let backup = fs::read_dir(f.root.join("first work/Workshop-backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read(backup).unwrap(), old);
    assert_ne!(fs::read(root.join("tte_runtime.zip")).unwrap(), old);
    assert!(!fs::read_dir(root).unwrap().any(|p| p
        .unwrap()
        .path()
        .extension()
        .is_some_and(|e| e == "tmp")));
}

#[test]
fn same_mod_cancel_invalid_marker_or_generation_failure_keeps_original_zip() {
    use std::io::Write;
    let f = Fixture::new();
    owned_gui_fixture(&f);
    let (root, exe) = workshop_fixture(&f);
    let old = fs::read(root.join("tte_runtime.zip")).unwrap();
    let denied = answer_enter(workshop_wizard(
        &f,
        &exe,
        "中文 Workshop/3818261596/must-not-generate",
    ));
    assert!(!denied.status.success());
    assert!(!root.join("must-not-generate").exists());
    let mut child = workshop_wizard(&f, &exe, "cancelled");
    child.stdin.take().unwrap().write_all(b"q\n").unwrap();
    assert!(!child.wait_with_output().unwrap().status.success());
    assert!(!f.root.join("cancelled").exists());
    write(&f.game.join("common/technology/base.txt"), "malformed={");
    assert!(!answer_enter(workshop_wizard(&f, &exe, "failure"))
        .status
        .success());
    assert_eq!(fs::read(root.join("tte_runtime.zip")).unwrap(), old);
    write(
        &root.join("technology-adapter/WORKSHOP-PACKAGE.json"),
        "{\"kind\":\"another-mod\"}",
    );
    assert!(!answer_enter(workshop_wizard(&f, &exe, "wrong marker"))
        .status
        .success());
    assert!(!f.root.join("wrong marker").exists());
    assert_eq!(fs::read(root.join("tte_runtime.zip")).unwrap(), old);
}

#[test]
fn workshop_update_during_confirm_never_gets_overwritten() {
    use std::io::{Read, Write};
    let f = Fixture::new();
    owned_gui_fixture(&f);
    let (root, exe) = workshop_fixture(&f);
    let mut child = workshop_wizard(&f, &exe, "stale");
    let mut stderr = child.stderr.take().unwrap();
    let mut preview = Vec::new();
    let mut byte = [0];
    while !preview.ends_with(b"Enter = build") {
        if stderr.read(&mut byte).unwrap() == 0 {
            break;
        }
        preview.push(byte[0]);
    }
    assert!(
        preview.ends_with(b"Enter = build"),
        "{}",
        String::from_utf8_lossy(&preview)
    );
    let mut archive = fs::read(root.join("tte_runtime.zip")).unwrap();
    archive.extend(b"Steam update during confirmation");
    fs::write(root.join("tte_runtime.zip"), &archive).unwrap();
    child.stdin.take().unwrap().write_all(b"\n").unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read(root.join("tte_runtime.zip")).unwrap(), archive);
    assert!(!f.docs.join("mod").read_dir().unwrap().any(|p| p
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("tte_native_generated_")));
}

#[test]
fn one_enter_builds_current_playset_and_registers_without_changing_selection() {
    use std::io::Write;
    let f = Fixture::new();
    owned_gui_fixture(&f);
    f.enable("mod/source.mod", &f.source, 0, false);
    write(
        &f.source.join("common/technology/new.txt"),
        "NeverListedByInstaller={area=physics cost=10 tier=1 prerequisites={base}}",
    );
    let database = fs::read(f.docs.join("launcher-v2.sqlite")).unwrap();
    let selection = fs::read(f.docs.join("dlc_load.json")).unwrap();
    let output = f.root.join("automatic output");
    let mut child = automatic_wizard(&f, &output);
    child.stdin.take().unwrap().write_all(b"\n").unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let reports: Vec<_> = fs::read_dir(&output)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    assert_eq!(reports.len(), 1);
    let report: Value = serde_json::from_slice(&fs::read(&reports[0]).unwrap()).unwrap();
    assert_eq!(report["selection"]["playsetId"], "p");
    assert_eq!(
        report["technologies"]["NeverListedByInstaller"]["condition"],
        json!(["tech", "base"])
    );
    assert_eq!(
        database,
        fs::read(f.docs.join("launcher-v2.sqlite")).unwrap()
    );
    assert_eq!(selection, fs::read(f.docs.join("dlc_load.json")).unwrap());
    assert_eq!(
        fs::read_dir(f.docs.join("mod"))
            .unwrap()
            .filter(|p| p
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("tte_native_generated_"))
            .count(),
        1
    );
}

#[test]
fn automatic_wizard_eof_cancel_and_source_output_leave_no_candidate() {
    let f = Fixture::new();
    owned_gui_fixture(&f);
    for (index, input) in [b"".as_slice(), b"q\n".as_slice()].into_iter().enumerate() {
        let output = f.root.join(format!("cancelled{index}"));
        let mut child = automatic_wizard(&f, &output);
        send_input(&mut child, input);
        assert!(!child.wait_with_output().unwrap().status.success());
        assert!(!output.exists());
    }
    let output = f.game.join("must_not_create");
    let mut child = automatic_wizard(&f, &output);
    send_input(&mut child, b"\n");
    assert!(!child.wait_with_output().unwrap().status.success());
    assert!(!output.exists());
    assert_eq!(fs::read_dir(f.docs.join("mod")).unwrap().count(), 0);
}

#[test]
fn browser_wizard_cancel_eof_and_source_output_are_read_only() {
    use std::process::Stdio;
    let f = Fixture::new();
    let before_db = fs::read(f.docs.join("launcher-v2.sqlite")).unwrap();
    for (index, input) in [b"".as_slice(), b"q\n".as_slice()].into_iter().enumerate() {
        let output = f.root.join(format!("web-cancel-{index}"));
        let mut child = Command::new(env!("CARGO_BIN_EXE_tte-core"))
            .arg("--web-wizard")
            .arg("--no-open")
            .arg("--game")
            .arg(&f.game)
            .arg("--documents")
            .arg(&f.docs)
            .arg("--output-parent")
            .arg(&output)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        send_input(&mut child, input);
        assert!(!child.wait_with_output().unwrap().status.success());
        assert!(!output.exists());
    }
    let output = f.game.join("must-not-create-web-output");
    let result = Command::new(env!("CARGO_BIN_EXE_tte-core"))
        .arg("--web-wizard")
        .arg("--no-open")
        .arg("--game")
        .arg(&f.game)
        .arg("--documents")
        .arg(&f.docs)
        .arg("--output-parent")
        .arg(&output)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists());
    assert_eq!(
        fs::read(f.docs.join("launcher-v2.sqlite")).unwrap(),
        before_db
    );
    assert_eq!(fs::read_dir(f.docs.join("mod")).unwrap().count(), 0);
}

#[test]
fn changing_playset_after_preview_requires_new_confirmation() {
    use std::io::{Read, Write};
    let f = Fixture::new();
    owned_gui_fixture(&f);
    let output = f.root.join("must_reconfirm");
    let mut child = automatic_wizard(&f, &output);
    let mut stderr = child.stderr.take().unwrap();
    let mut preview = Vec::new();
    let mut byte = [0u8; 1];
    while stderr.read(&mut byte).unwrap() != 0 {
        preview.push(byte[0]);
        if String::from_utf8_lossy(&preview).contains("Enter = build") {
            break;
        }
    }
    let db = Connection::open(f.docs.join("launcher-v2.sqlite")).unwrap();
    db.execute("update playsets set name='changed after preview'", [])
        .unwrap();
    drop(db);
    child.stdin.take().unwrap().write_all(b"\n").unwrap();
    assert!(!child.wait_with_output().unwrap().status.success());
    assert!(String::from_utf8_lossy(&preview).contains("Enter = build"));
    assert!(!output.exists());
}

#[test]
fn inline_comment_cannot_hide_caller_fields() {
    let f = Fixture::new();
    write(
        &f.source.join("common/inline_scripts/probe.txt"),
        "area=physics # no final newline",
    );
    write(
        &f.source.join("common/technology/new.txt"),
        "new={inline_script=probe cost=5 tier=1}",
    );
    assert_eq!(
        f.ok(&["--mod", f.source.to_str().unwrap()])["technologies"]["new"]["cost"],
        5.0
    );
}

#[test]
fn unsupported_inline_conditionals_fail_instead_of_applying_both_branches() {
    let f = Fixture::new();
    write(
        &f.source.join("common/inline_scripts/probe.txt"),
        "area=physics [[!skip] prerequisites={base} ]",
    );
    write(
        &f.source.join("common/technology/new.txt"),
        "new={inline_script={script=probe skip=yes} cost=5}",
    );
    let p = f.run(&["--mod", f.source.to_str().unwrap()]);
    assert!(!p.status.success());
    assert!(String::from_utf8_lossy(&p.stderr).contains("conditional/default"));
    assert!(!f.root.join("result.json").exists());
}

#[test]
fn numeric_block_does_not_silently_become_default() {
    let f = Fixture::new();
    write(
        &f.game.join("common/technology/base.txt"),
        "base={area=physics tier={} cost=5}",
    );
    let p = f.run(&[]);
    assert!(!p.status.success());
    assert!(String::from_utf8_lossy(&p.stderr).contains("numeric block without"));
}

#[test]
fn dynamic_numeric_block_and_unknown_gates_are_retained() {
    let f = Fixture::new();
    write(&f.game.join("common/technology/base.txt"), "base={area=physics cost={factor=31 modifier={factor=2 is_ai=yes}} potential={is_ai=no} is_reward=yes}");
    let v = f.ok(&[]);
    let t = &v["technologies"]["base"];
    assert_eq!(t["cost"], 31.0);
    assert!(!t["numericBlocks"]["cost"].is_null());
    assert!(t["rawDefinitionFields"].to_string().contains("potential"));
}

#[test]
fn duplicate_definition_policy_is_explicitly_unverified() {
    let f = Fixture::new();
    write(
        &f.source.join("common/technology/z.txt"),
        "base={area=physics cost=44}",
    );
    let v = f.ok(&["--mod", f.source.to_str().unwrap()]);
    assert_eq!(v["technologies"]["base"]["cost"], 44.0);
    assert!(v["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["issue"] == "duplicate_technology_definition"
            && w["enginePrecedenceVerified"] == false));
    assert_eq!(
        v["technologies"]["base"]["sourceRelativePath"],
        "common/technology/z.txt"
    );
}

#[test]
fn or_candidate_closure_is_not_labelled_required() {
    let f = Fixture::new();
    write(
        &f.source.join("common/technology/new.txt"),
        "a={area=physics} b={area=physics} goal={area=physics prerequisites={OR={a b}}}",
    );
    let v = f.ok(&["--mod", f.source.to_str().unwrap(), "--goal", "goal"]);
    assert!(v["route"].get("needed").is_none());
    assert_eq!(v["route"]["candidateClosure"], json!(["a", "b", "goal"]));
    assert_eq!(
        v["route"]["goalCondition"],
        json!(["any", ["tech", "a"], ["tech", "b"]])
    );
}

#[test]
fn expansion_fanout_has_a_call_budget() {
    let f = Fixture::new();
    for i in 0..14 {
        write(
            &f.source.join(format!("common/inline_scripts/n{i}.txt")),
            &format!("inline_script=n{} inline_script=n{}", i + 1, i + 1),
        );
    }
    write(
        &f.source.join("common/inline_scripts/n14.txt"),
        "effect={log=benign}",
    );
    write(
        &f.source.join("common/technology/new.txt"),
        "new={area=physics inline_script=n0}",
    );
    let p = f.run(&["--mod", f.source.to_str().unwrap()]);
    assert!(!p.status.success());
    assert!(String::from_utf8_lossy(&p.stderr).contains("call budget"));
}

#[test]
fn wal_database_is_read_from_private_copy_and_disabled_rows_are_ignored() {
    let f = Fixture::new();
    f.enable("mod/source.mod", &f.source, 0, false);
    let dbpath = f.docs.join("launcher-v2.sqlite");
    let db = Connection::open(&dbpath).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    db.execute("update playsets set name='WAL selection'", [])
        .unwrap();
    db.execute_batch("insert into mods values('disabled','mod/nonexistent.mod','absent.zip','absent.zip','not_ready');insert into playsets_mods values('p','disabled',0,1);").unwrap();
    let files: Vec<_> = [
        "launcher-v2.sqlite",
        "launcher-v2.sqlite-wal",
        "launcher-v2.sqlite-shm",
    ]
    .into_iter()
    .map(|s| f.docs.join(s))
    .collect();
    let before: Vec<_> = files.iter().map(|p| fs::read(p).unwrap()).collect();
    let v = f.ok(&["--documents", f.docs.to_str().unwrap()]);
    assert_eq!(v["selection"]["playsetName"], "WAL selection");
    for (path, bytes) in files.iter().zip(before) {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    drop(db);
}

#[test]
fn duplicate_semantic_fields_are_not_silently_first_wins() {
    let f = Fixture::new();
    write(&f.game.join("common/technology/base.txt"),"base={area=physics tier=1 prerequisites={a} prerequisites={b}} a={area=physics} b={area=physics}");
    let p = f.run(&[]);
    assert!(!p.status.success());
    assert!(String::from_utf8_lossy(&p.stderr).contains("ambiguous duplicate prerequisites"));
    assert!(!f.root.join("result.json").exists());
}

#[test]
fn inline_caller_duplicate_override_is_explicitly_ambiguous() {
    let f = Fixture::new();
    write(
        &f.source.join("common/inline_scripts/probe.txt"),
        "area=physics cost=10",
    );
    write(
        &f.source.join("common/technology/new.txt"),
        "new={inline_script=probe area=society cost=99}",
    );
    let p = f.run(&["--mod", f.source.to_str().unwrap()]);
    assert!(!p.status.success());
    assert!(String::from_utf8_lossy(&p.stderr).contains("ambiguous duplicate area"));
}

#[test]
fn inline_quoted_structure_and_nested_markers_are_never_guessed() {
    for args in [
        "p=\"a b\"",
        "p=\"x} cost=1\"",
        "p=\"$b$\" b=base",
        "b=base p=\"$b$\"",
    ] {
        let f = Fixture::new();
        write(
            &f.source.join("common/inline_scripts/probe.txt"),
            "area=physics prerequisites={$p$}",
        );
        write(
            &f.source.join("common/technology/new.txt"),
            &format!("new={{inline_script={{script=probe {args}}}}}"),
        );
        let p = f.run(&["--mod", f.source.to_str().unwrap()]);
        assert!(!p.status.success());
        assert!(String::from_utf8_lossy(&p.stderr).contains("Unsupported inline argument"));
        assert!(!f.root.join("result.json").exists());
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_path_argument_does_not_panic() {
    use std::os::unix::ffi::OsStringExt;
    let p = Command::new(env!("CARGO_BIN_EXE_tte-core"))
        .arg("--game")
        .arg(std::ffi::OsString::from_vec(vec![b'/', 0xff]))
        .arg("--output")
        .arg("irrelevant.json")
        .output()
        .unwrap();
    assert!(!p.status.success());
    assert!(String::from_utf8_lossy(&p.stderr).contains("FAILED"));
    assert!(!String::from_utf8_lossy(&p.stderr).contains("panicked"));
}
