#[allow(dead_code)]
mod advice;
mod archive;
mod atomic_report;
mod catalog;
mod dbsnapshot;
#[allow(dead_code)]
mod diagnostics;
#[allow(dead_code)] // Not exposed until full native generation is accepted.
mod emitter;
#[allow(dead_code)]
mod gui;
#[allow(dead_code)]
mod history;
mod languages;
mod metadata;
#[allow(dead_code)]
mod partition;
#[allow(dead_code)]
mod planner_emit;
#[allow(dead_code)] // Shared native planning emitter is being migrated/tested.
mod planning;
mod references;
#[allow(dead_code)]
mod runtime;
mod script;
#[allow(dead_code)]
mod selection;
mod sources;
mod traditional;
#[allow(dead_code)]
mod view;
mod wizard;
mod workshop;

use script::Result;
use serde_json::json;
use std::{
    collections::BTreeSet,
    env,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Instant,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
fn safe(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

fn help() {
    println!(
        r#"TTE native source/graph core {VERSION}
Usage: tte-core --game PATH [--mod PATH ... | --documents PATH [--playset ID]] --output FILE
                [--goal ID] [--owned ID ...] [--language english ...]
                [--experimental-build NEW_DIRECTORY [--register-local DOCUMENTS]]
       tte-core --build-wizard [--game PATH] [--documents PATH] [--output-parent PATH]
       tte-core --workshop-wizard [--game PATH] [--documents PATH] [--output-parent PATH]

--workshop-wizard is the included Workshop entry: updates only this Mod's own
runtime ZIP after a verified backup; no new Local Mod. Close game/launcher first.

--build-wizard detects the installed game and current playset, previews paths,
then Enter builds and registers a NEW local planner. It never enables the Mod.
The optional paths above override detected defaults; no other flags may be mixed.

This experimental binary has NO Python runtime. It reads source data and emits
a catalog/dependency report by default. --experimental-build explicitly creates
a local planner candidate whose native-game acceptance is pending. An optional
--register-local creates a NEW local .mod file only; it never changes playsets.
It never launches Python, reads saves, or uploads anything.

No mod ID allowlist. --mod accepts a directory or root-descriptor ZIP.
Supports ordered overlays/replace_path, inline scripts,
scripted numbers, AND/OR prerequisites, source diagnostics and localised names.
Unsupported/ambiguous conditions remain explicit. Use a NEW output filename.
Source roots are read-only. No network, memory access or mod-code execution.
"#
    );
}

#[derive(Default)]
struct Args {
    game: Option<PathBuf>,
    mods: Vec<PathBuf>,
    docs: Option<PathBuf>,
    playset: Option<String>,
    output: Option<PathBuf>,
    goal: Option<String>,
    owned: BTreeSet<String>,
    languages: Vec<String>,
    build: Option<PathBuf>,
    register: Option<PathBuf>,
    selection_guard: dbsnapshot::InputHashes,
    previewed_mods: Option<Vec<PathBuf>>,
    workshop: Option<workshop::Target>,
}

fn ask(label: &str) -> Result<String> {
    eprint!("{label}: ");
    io::stderr().flush().map_err(|e| e.to_string())?;
    let mut value = String::new();
    if io::stdin()
        .read_line(&mut value)
        .map_err(|e| e.to_string())?
        == 0
    {
        return Err("No interactive input. Pass --game and --output explicitly".into());
    }
    let value = value.trim().trim_matches('"').to_owned();
    if value.is_empty() {
        Err("Empty input".into())
    } else {
        Ok(value)
    }
}

fn ask_optional(label: &str) -> Result<Option<String>> {
    eprint!("{label}: ");
    io::stderr().flush().map_err(|e| e.to_string())?;
    let mut value = String::new();
    if io::stdin()
        .read_line(&mut value)
        .map_err(|e| e.to_string())?
        == 0
    {
        return Err("No interactive input; pass explicit command-line choices".into());
    }
    let value = value.trim().trim_matches('"').to_owned();
    Ok((!value.is_empty()).then_some(value))
}

fn parse_args() -> Result<Option<Args>> {
    let mut args = Args::default();
    let wizard = env::args_os().len() == 1;
    let mut build_wizard = false;
    let mut workshop_wizard = false;
    let mut wizard_parent = None;
    let mut input = env::args_os().skip(1);
    while let Some(flag_os) = input.next() {
        let flag = flag_os.to_str().ok_or("Option names must be UTF-8")?;
        if flag == "--help" || flag == "-h" {
            help();
            return Ok(None);
        }
        if flag == "--version" {
            println!("tte-core {VERSION} (compiled Rust; no Python runtime)");
            return Ok(None);
        }
        if flag == "--build-wizard" {
            build_wizard = true;
            continue;
        }
        if flag == "--workshop-wizard" {
            workshop_wizard = true;
            continue;
        }
        let value = input
            .next()
            .ok_or_else(|| format!("Missing value for {flag}"))?;
        match flag {
            "--game" => args.game = Some(value.into()),
            "--output-parent" => wizard_parent = Some(PathBuf::from(value)),
            "--mod" => args.mods.push(value.into()),
            "--documents" => args.docs = Some(value.into()),
            "--playset" => {
                args.playset = Some(
                    value
                        .into_string()
                        .map_err(|_| "Playset ID must be UTF-8")?,
                )
            }
            "--output" => args.output = Some(value.into()),
            "--experimental-build" => args.build = Some(value.into()),
            "--register-local" => args.register = Some(value.into()),
            "--goal" => args.goal = Some(value.into_string().map_err(|_| "Goal ID must be UTF-8")?),
            "--owned" => {
                args.owned
                    .insert(value.into_string().map_err(|_| "Owned ID must be UTF-8")?);
            }
            "--language" => args
                .languages
                .push(value.into_string().map_err(|_| "Language must be UTF-8")?),
            _ => return Err(format!("Unknown option: {flag}")),
        }
    }
    if build_wizard || workshop_wizard {
        if build_wizard && workshop_wizard {
            return Err("Choose one wizard mode".into());
        }
        if !args.mods.is_empty()
            || args.playset.is_some()
            || args.output.is_some()
            || args.build.is_some()
            || args.register.is_some()
            || args.goal.is_some()
            || !args.owned.is_empty()
            || !args.languages.is_empty()
        {
            return Err("Build wizard accepts only optional --game, --documents and --output-parent defaults".into());
        }
        return wizard::run(
            args.game.as_deref(),
            args.docs.as_deref(),
            wizard_parent.as_deref(),
            workshop_wizard,
        )
        .map(Some);
    }
    if wizard_parent.is_some() {
        return Err("--output-parent requires --build-wizard".into());
    }
    if args.docs.is_some() && !args.mods.is_empty() {
        return Err("Choose --documents playset OR explicit --mod roots".into());
    }
    if args.playset.is_some() && args.docs.is_none() {
        return Err("--playset requires --documents".into());
    }
    if args.register.is_some() && args.build.is_none() {
        return Err("--register-local requires --experimental-build".into());
    }
    if args.game.is_none() {
        if wizard {
            eprintln!("EXPERIMENTAL collector only / 实验性采集器：只输出科技数据，不生成游戏 Mod。\nSources stay unchanged. No Python is used. / 不修改来源，不使用 Python。");
        }
        args.game = Some(ask("Game data folder / 游戏数据目录")?.into());
    }
    if wizard {
        args.docs = ask_optional("Stellaris Documents folder (Enter = game only, NO source mods) / 群星用户目录（回车 = 仅本体，不收录 Mod）")?.map(PathBuf::from);
        if args.docs.is_some() {
            args.playset = ask_optional(
                "Playset ID (Enter = active playset) / 播放集 ID（回车 = 当前播放集）",
            )?;
        } else {
            eprintln!("Game-only scan explicitly chosen / 已选择仅采集本体科技。");
        }
    }
    if args.output.is_none() {
        args.output = Some(ask("NEW catalog output file / 新结果文件路径")?.into());
    }
    if args.languages.is_empty() {
        args.languages = vec!["english".into(), "simp_chinese".into()];
    }
    Ok(Some(args))
}

fn run(args: Args) -> Result<()> {
    let started = Instant::now();
    dbsnapshot::verify(&args.selection_guard)?;
    let game = sources::canonical(args.game.as_ref().unwrap())?;
    if !game.join("common/technology").is_dir() {
        return Err("Game data folder needs common/technology".into());
    }
    if let Some(docs) = &args.register {
        if !docs.join("mod").is_dir() {
            return Err("Registration needs an existing Stellaris user-data/mod folder".into());
        }
    }
    let (mods, selection, selection_hashes) = if let Some(docs) = &args.docs {
        eprintln!("[1/4] Read selected playset / 读取播放集");
        sources::selection(docs, args.playset.as_deref())?
    } else {
        (
            args.mods,
            json!({"kind":"explicit-ordered-directories"}),
            Vec::new(),
        )
    };
    dbsnapshot::verify(&args.selection_guard)?;
    if args.previewed_mods.as_ref().is_some_and(|p| p != &mods) {
        return Err("Selected sources differ from the confirmed preview; run again".into());
    }
    let output = args.output.unwrap();
    if output
        .extension()
        .and_then(|s| s.to_str())
        .is_none_or(|s| !s.eq_ignore_ascii_case("json"))
    {
        return Err("Catalog output must use a .json filename".into());
    }
    if output.exists() {
        return Err("Output exists; choose a fresh result file".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err("Create the result's parent directory first".into());
    }
    let parent = sources::canonical(parent)?;
    let output = parent.join(output.file_name().ok_or("Invalid result filename")?);
    for docs in args.docs.iter().chain(args.register.iter()) {
        if output.starts_with(sources::canonical(docs)?) {
            return Err("Report output must be outside the Stellaris user-data directory".into());
        }
    }
    for root in std::iter::once(&game).chain(mods.iter()) {
        let root = sources::canonical(root)?;
        if output.starts_with(&root) {
            return Err("Output must be outside every source root".into());
        }
    }
    let build = if let Some(path) = &args.build {
        if path.exists() {
            return Err("Build output exists; choose a fresh directory".into());
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let resolved =
            sources::canonical(parent)?.join(path.file_name().ok_or("Invalid build directory")?);
        if output.starts_with(&resolved) {
            return Err("Catalog report must be outside the candidate runtime".into());
        }
        for root in std::iter::once(&game)
            .chain(mods.iter())
            .chain(args.docs.iter())
            .chain(args.register.iter())
        {
            if resolved.starts_with(sources::canonical(root)?) {
                return Err("Candidate output must be outside source/user-data roots".into());
            }
        }
        Some(resolved)
    } else {
        None
    };
    atomic_report::check_parent(&parent).map_err(|e| e.to_string())?;
    if let Some(path) = &build {
        atomic_report::check_parent(path.parent().ok_or("Build parent missing")?)
            .map_err(|e| e.to_string())?;
    }
    eprintln!("[2/4] Collect source overlays / 收录来源与覆盖关系");
    let mut languages = args.languages;
    if build.is_some() {
        for language in languages::LANGUAGES {
            if !languages.iter().any(|s| s == language) {
                languages.push(language.into());
            }
        }
    }
    let mut sources = sources::Sources::load(&game, &mods, languages, build.is_some())?;
    eprintln!("[3/4] Parse technology graph / 解析科技与前置关系");
    let mut report = catalog::collect(&mut sources)?;
    if build.is_some() {
        metadata::enrich(&mut sources, &mut report)?;
    }
    report["selection"] = selection;
    report["toolVersion"] = json!(VERSION);
    if let Some(goal) = args.goal {
        report["route"] = catalog::route(&report, &goal, &args.owned)?;
    }
    let generated = if build.is_some() {
        Some(runtime::assemble(&mut sources, &report)?)
    } else {
        None
    };
    eprintln!("[4/4] Verify unchanged sources / 核对来源未改变");
    sources.verify()?;
    dbsnapshot::verify(&selection_hashes)?;
    dbsnapshot::verify(&args.selection_guard)?;
    if let (Some(path), Some(generated)) = (&build, generated) {
        eprintln!("[build 6/6] Write generated planner data / 写入生成的规划器数据");
        let proof = runtime::write(path, generated)?;
        report["generatedCandidate"] = json!({"path":sources::path_json(path),"buildStatus":proof["buildStatus"],"nativeValidation":"NOT RUN","technologies":proof["technologies"]});
        report["capabilities"]["gameScriptGeneration"] = json!(true);
        report["registrationRequested"] = json!(args.register.is_some());
    }
    for row in report["sources"].as_array().unwrap() {
        eprintln!(
            "  {}: {} technologies / 项科技",
            safe(row["name"].as_str().unwrap()),
            row["effectiveTechnologies"]
        );
    }
    let count = report["technologies"].as_object().unwrap().len();
    report["elapsedSeconds"] = json!(started.elapsed().as_secs_f64());
    let raw = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    atomic_report::publish(&output, &raw).map_err(|e| e.to_string())?;
    // Registration is the final fallible side effect. A report publication
    // failure must never leave a newly registered mod behind.
    if let (Some(path), Some(documents)) = (&build, &args.register) {
        runtime::register(path, documents)?;
    }
    if let (Some(path), Some(target)) = (&build, &args.workshop) {
        target.install(path, || {
            sources.verify()?;
            dbsnapshot::verify(&selection_hashes)?;
            dbsnapshot::verify(&args.selection_guard)
        })?;
    }
    println!(
        "COMPLETE / 采集完成: {count} technologies, {:.3}s",
        started.elapsed().as_secs_f64()
    );
    println!("{}", safe(&sources::path_json(&output)));
    if args.workshop.is_some() {
        println!("WORKSHOP MOD UPDATED / 已更新同一个工坊 Mod。Keep it enabled after its source Mods; no Local Mod or playset switch is needed. / 保持此 Mod 启用并排在来源 Mod 后，无需切换本地版本。");
    } else if build.is_some() {
        println!("LOCAL CANDIDATE GENERATED. Native game QA pending; no public upload. / 本地候选已生成，仍需游戏内验收，未公开上传。");
        println!("Restart the launcher. Enable the generated local planner AFTER its source mods; disable other planner versions. / 重开启动器，把新生成的本地规划器排在来源 Mod 后面，关闭其他版本的规划器。");
    } else {
        println!("Native source/graph prototype only. No game Mod generated. / 原生采集核心原型，尚未生成游戏 Mod。");
    }
    Ok(())
}

fn main() {
    match parse_args().and_then(|args| args.map(run).transpose()) {
        Ok(_) => (),
        Err(e) => {
            eprintln!("FAILED / 失败: {}", safe(&e));
            std::process::exit(1);
        }
    }
}
