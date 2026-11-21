//! Local, player-initiated support reports. Inventory collection must still
//! work when the adapter rejects an early entry in the active playset.
use crate::{atomic_report, catalog, dbsnapshot, script, sources, web, wizard, VERSION};
use rusqlite::{Connection, Row};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU8, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) static PHASE: AtomicU8 = AtomicU8::new(1);
const LAST: &str = "last-adapter-failure.json";

fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn small_text(path: &Path, limit: u64) -> Option<String> {
    if !fs::symlink_metadata(path).ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    String::from_utf8(bytes)
        .ok()
        .map(|s| s.trim_start_matches('\u{feff}').to_owned())
}
fn small_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&small_text(path, 1024 * 1024)?).ok()
}
fn registry(s: &str) -> Option<String> {
    let name = sources::relative(s).ok()?;
    (name.starts_with("mod/")
        && name.ends_with(".mod")
        && name.split('/').count() == 2
        && !name.chars().any(|c| c.is_control()))
    .then_some(name)
}
fn message(raw: &str, roots: &[PathBuf]) -> String {
    let mut text = raw
        .replace('\\', "/")
        .replace("//?/UNC/", "//")
        .replace("//?/", "");
    let mut prefixes: Vec<_> = roots
        .iter()
        .filter(|p| p.is_absolute() && p.parent().is_some())
        .map(|p| sources::path_json(p))
        .collect();
    prefixes.sort_by_key(|p| std::cmp::Reverse(p.len()));
    for p in prefixes {
        // ASCII folding keeps the byte offsets valid for non-ASCII paths.
        for (at, _) in text
            .to_ascii_lowercase()
            .match_indices(&p.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            text.replace_range(at..at + p.len(), "<local>");
        }
    }
    let lower = text.to_ascii_lowercase();
    if [
        "password",
        "token=",
        "token =",
        "secret=",
        "api_key",
        "api-key",
        "authorization",
        "bearer ",
        "://",
        "sk-",
        ":/",
        "//",
        "/users/",
        "/home/",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        return "[private path or credential-like text omitted]".into();
    }
    text.chars()
        .filter(|c| !c.is_control())
        .take(2048)
        .collect()
}

// Public comments are short and deliberately separate from the local inventory.
// Work with the failing operation's own context, never pick a mod from a list of
// installed mods, warnings, or filenames that happen to match.
fn comment_field(raw: &str, limit: usize, roots: &[PathBuf]) -> String {
    let first = raw.lines().next().unwrap_or("").trim();
    let lower = first.to_ascii_lowercase();
    let absolute_path = first.match_indices('/').any(|(i, _)| {
        !first[i + 1..].starts_with(' ')
            && (i == 0
                || first[..i].ends_with(|c: char| c.is_whitespace() || "=:\"'([{~".contains(c)))
    });
    let cleaned = message(first, roots);
    if absolute_path
        || cleaned.contains("<local>")
        || cleaned.starts_with("[private path")
        || first.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        || [
            "token:",
            "passwd",
            "secret:",
            "secret =",
            "api key",
            "apikey",
            "private key",
            "ssh-",
            "cookie:",
        ]
        .iter()
        .any(|s| lower.contains(s))
        || first.split_whitespace().any(|s| {
            s.split_once('@')
                .is_some_and(|(u, d)| !u.is_empty() && d.contains('.'))
        })
    {
        if let Some((_, code)) = first.rsplit_once("(os error ") {
            if let Some(n) = code.strip_suffix(')').and_then(|s| s.parse::<i32>().ok()) {
                return format!("OS error {n}");
            }
        }
        return "(details omitted)".into();
    }
    if cleaned.len() <= limit {
        cleaned
    } else {
        format!("{}…", &cleaned[..cleaned.floor_char_boundary(limit - 3)])
    }
}

fn workshop_comment(label: &str, phase: &str, raw: &str, roots: &[PathBuf]) -> String {
    let mut source = None;
    let mut file = None;
    let mut tech = None;
    let mut reason = raw;
    // Only recognize the adapter's source/file[/technology]: error and
    // launcher registration: error formats. Source titles can contain colons.
    for (at, _) in raw.match_indices(" / ") {
        let Some((location, detail)) = raw[at + 3..].split_once(": ") else {
            continue;
        };
        let (path, id) = location
            .split_once(" / ")
            .map_or((location, None), |(p, i)| (p, Some(i)));
        let folder = path.split('/').next().unwrap_or_default();
        if !["common", "interface", "localisation", "localisation_synced"]
            .iter()
            .any(|known| folder.eq_ignore_ascii_case(known))
            || !path.contains('/')
            || sources::relative(path).is_err()
        {
            continue;
        }
        source = Some(&raw[..at]);
        file = Some(path);
        tech = id.filter(|i| catalog::identifier(i));
        reason = detail;
        break;
    }
    if source.is_none() {
        if let Some((registration, detail)) = raw.split_once(": ") {
            if registry(registration).is_some() {
                source = Some(registration);
                reason = detail;
            }
        }
    }
    // Some engine/UI diagnostics append a Debug list of files. The local log
    // retains it; the public excerpt only needs the error before the list.
    let reason = reason
        .split_once("[\"")
        .map_or(reason, |(head, _)| head.trim_end().trim_end_matches(" in"));
    let mut lines = vec![
        format!("TTE {label} | {phase} | {}", env::consts::OS),
        format!(
            "Source: {}",
            source
                .map(|s| comment_field(s, 100, roots))
                .unwrap_or_else(|| "undetermined".into())
        ),
    ];
    if let Some(path) = file {
        lines.push(format!("File: {}", comment_field(path, 120, roots)));
    }
    if let Some(id) = tech {
        lines.push(format!("Tech: {}", comment_field(id, 80, roots)));
    }
    lines.push(format!("Error: {}", comment_field(reason, 240, roots)));
    lines.join("\n")
}

fn print_comment(comment: &str) {
    eprintln!("Copy the lines below into a Workshop comment / 反馈时请复制下面几行：\n---\n{comment}\n---");
}

#[cfg(test)]
mod comment_tests {
    use super::*;

    #[test]
    fn only_the_registration_or_failing_file_appears_in_a_comment() {
        let s = workshop_comment(
            "adapter",
            "step 1/4",
            r"mod/External Pack.mod: C:\Users\PRIVATE\broken.zip: file not found (os error 2)",
            &[],
        );
        assert!(
            s.contains("Source: mod/External Pack.mod\nError: OS error 2"),
            "{s}"
        );
        let s = workshop_comment(
            "adapter",
            "step 3/4",
            "GUI type research: found 2 in [\"mod/private one\", \"mod/private two\"]",
            &[],
        );
        assert!(
            s.contains("Source: undetermined") && s.contains("found 2") && !s.contains("private")
        );
        let s = workshop_comment("adapter", "step 3/4",
            "Secrets Beyond the Gates: UI (#2) / common/technology/test.txt / tech:science: Unresolved scripted variable: @missing", &[]);
        assert!(s.contains("Source: Secrets Beyond the Gates: UI (#2)\nFile: common/technology/test.txt\nTech: tech:science\nError: Unresolved scripted variable: @missing"));
    }

    #[test]
    fn public_text_is_bounded_and_private_error_details_stay_local() {
        for raw in [
            "/mnt/private/a",
            r"C:\Users\Someone\file",
            r"\\nas\share",
            "token: private",
            "person@example.com",
        ] {
            let s = workshop_comment(
                "adapter",
                "step 3/4",
                &format!("Pack / common/technology/x.txt / test: {raw}"),
                &[],
            );
            assert!(
                s.contains("Source: Pack") && s.ends_with("(details omitted)"),
                "{s}"
            );
        }
        let long = "测试".repeat(500);
        let s = workshop_comment(
            "adapter",
            "step 3/4",
            &format!("{long} / common/technology/{long}.txt / tech: {long}"),
            &[],
        );
        assert!(s.len() < 650 && s.ends_with('…') && s.lines().count() == 5);
    }
}

fn support_home() -> Option<PathBuf> {
    wizard::home().map(|p| p.join("TTE-Research-Planner"))
}
fn os_version() -> Option<String> {
    #[cfg(windows)]
    let output = std::process::Command::new(
        PathBuf::from(env::var_os("SystemRoot")?).join("System32/cmd.exe"),
    )
    .args(["/d", "/c", "ver"])
    .output()
    .ok()?;
    #[cfg(target_os = "macos")]
    let output = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    #[cfg(any(windows, target_os = "macos"))]
    {
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        fs::read_to_string("/etc/os-release")
            .ok()?
            .lines()
            .find_map(|s| s.strip_prefix("PRETTY_NAME="))
            .map(|s| s.trim_matches('"').to_owned())
    }
}
fn ui_settings(docs: &Path) -> Option<Value> {
    let settings = script::parse(&small_text(&docs.join("settings.txt"), 256 * 1024)?).ok()?;
    let graphics = settings.first("graphics");
    let size = graphics.and_then(|g| g.first("size"));
    let n = |key| {
        size.and_then(|s| s.scalar(key))
            .and_then(|s| s.parse::<u32>().ok())
    };
    Some(json!({"language":settings.scalar("language"),
        "guiScale":settings.scalar("gui_scale").or_else(||graphics?.scalar("gui_scale")).and_then(|s|s.parse::<f64>().ok()),
        "configuredWidth":n("x"),"configuredHeight":n("y")}))
}
fn package_root() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let tools = exe.parent()?.parent()?;
    matches!(
        tools.file_name()?.to_str()?,
        "technology-adapter" | "technology-viewer"
    )
    .then(|| tools.parent().map(Path::to_owned))
    .flatten()
}
#[derive(Default)]
pub(crate) struct InputFolders {
    pub game: Option<PathBuf>,
    pub docs: Option<PathBuf>,
}

pub fn report_failure(error: &str, inputs: InputFolders) {
    let mode = match env::args_os()
        .nth(1)
        .and_then(|s| s.into_string().ok())
        .as_deref()
    {
        Some("--workshop-wizard" | "--build-wizard") => "adapter",
        Some("--web-wizard") => "browser",
        _ => return,
    };
    if error.starts_with("Cancelled;") || error.starts_with("Interactive input ended") {
        return;
    }
    if mode == "browser"
        && !package_root()
            .is_some_and(|r| r.join("technology-adapter/WORKSHOP-PACKAGE.json").is_file())
    {
        return;
    }
    let roots: Vec<_> = [
        package_root(),
        inputs.game.clone(),
        inputs.docs.clone(),
        wizard::home(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let stamp = now();
    let phase = PHASE.load(Ordering::Relaxed);
    let comment = workshop_comment(mode, &format!("step {phase}/4"), error, &roots);
    let record = json!({"timeUnixMs":stamp as u64,"mode":mode,"phase":phase,
        "message":message(error,&roots)});
    if let Some(dir) = support_home() {
        let _ = save_receipt(&dir, &record, stamp);
    }
    if mode == "browser" {
        print_comment(&comment);
        return;
    }
    // Use the failing invocation's error directly. A concurrent run may have
    // replaced the last-error receipt; rebuilding the graph could fail again.
    if let Err(e) = collect(inputs, None, Some((record, &comment))) {
        eprintln!(
            "Could not save the support log / 无法保存故障日志：{}",
            crate::safe(&e)
        );
    }
    print_comment(&comment);
}

fn save_receipt(dir: &Path, record: &Value, stamp: u128) -> std::io::Result<()> {
    let tmp = dir.join(format!(".failure-{}-{stamp}.tmp", std::process::id()));
    fs::create_dir_all(dir)?;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp)?;
    let result = file
        .write_all(record.to_string().as_bytes())
        .and_then(|_| file.sync_all());
    drop(file);
    let result = result.and_then(|_| fs::rename(&tmp, dir.join(LAST)));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn registration_files(docs: &Path) -> script::Result<Vec<String>> {
    let entries = fs::read_dir(docs.join("mod")).map_err(|e| e.to_string())?;
    let mut names = Vec::new();
    for file in entries {
        let file = file.map_err(|e| e.to_string())?;
        if let Some(name) = file
            .file_name()
            .to_str()
            .and_then(|n| registry(&format!("mod/{n}")))
        {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}
fn launcher(docs: &Path) -> script::Result<Value> {
    if !docs.join("launcher-v2.sqlite").is_file() {
        return Err("Launcher database missing".into());
    }
    let snapshot = dbsnapshot::Snapshot::new(&docs.join("launcher-v2.sqlite"))?;
    let query = || -> rusqlite::Result<Value> {
        let db = Connection::open(&snapshot.database)?;
        db.execute_batch("PRAGMA query_only=ON")?;
        let columns = db
            .prepare("SELECT name FROM pragma_table_info('mods')")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<BTreeSet<_>>>()?;
        let optional = [
            "displayName",
            "version",
            "requiredVersion",
            "source",
            "steamId",
        ]
        .map(|name| if columns.contains(name) { name } else { "NULL" })
        .join(",");
        let mut mods=db.prepare(&format!("SELECT gameRegistryId,status,
            coalesce(dirPath,'')!='',coalesce(archivePath,'')!='',{optional} FROM mods ORDER BY gameRegistryId"))?;
        let rows=mods.query_map([],|r|{
            let col=|i:usize|r.get::<_,Option<String>>(i).ok().flatten();
            let reg=col(0).as_deref().and_then(registry);
            let meta=reg.as_ref().and_then(|s|small_text(&docs.join(s),256*1024)).and_then(|s|script::parse(&s).ok());
            let field=|i,key| col(i).filter(|s|!s.is_empty()).or_else(||meta.as_ref()?.scalar(key).map(str::to_owned));
            Ok(json!({"registration":reg,"name":field(4,"name"),"version":field(5,"version"),
                "supportedVersion":field(6,"supported_version"),"source":col(7),"workshopId":field(8,"remote_file_id").filter(|s|!s.is_empty() && s.bytes().all(|c|c.is_ascii_digit())),
                "status":col(1),"directory":r.get::<_,bool>(2)?,"archive":r.get::<_,bool>(3)?}))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut active = db.prepare("SELECT name,id FROM playsets WHERE isActive=1 ORDER BY id")?;
        let active_sets = active
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut sets = Vec::new();
        for (name, id) in active_sets {
            let mut stmt=db.prepare("SELECT m.gameRegistryId,pm.position,pm.enabled FROM playsets_mods pm LEFT JOIN mods m ON pm.modId=m.id WHERE pm.playsetId=?1 ORDER BY pm.position")?;
            let entries = stmt
                .query_map([id], |r: &Row<'_>| {
                    Ok(json!({
                        "registration":r.get::<_,Option<String>>(0)?.as_deref().and_then(registry),
                        "position":r.get::<_,Option<i64>>(1)?,"enabled":r.get::<_,Option<bool>>(2)?
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            sets.push(json!({"name":name,"entries":entries}));
        }
        Ok(json!({"installed":rows,"activePlaysets":sets}))
    };
    query().map_err(|e| e.to_string())
}

// Keep engine error sites and counts; free-form game logs can contain chat,
// account names, URLs, machine paths, or text written by arbitrary mods.
fn game_errors(path: &Path) -> std::io::Result<Value> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() {
        return Err(std::io::Error::other("not a regular log file"));
    }
    let mut f = fs::File::open(path)?;
    let offset = meta.len().saturating_sub(65536);
    f.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    f.take(65536).read_to_end(&mut bytes)?;
    let mut sites = BTreeMap::<String, usize>::new();
    let mut files = BTreeSet::new();
    for line in String::from_utf8_lossy(&bytes)
        .lines()
        .skip(usize::from(offset > 0))
    {
        let Some((header, body)) = line.split_once("]:") else {
            continue;
        };
        let site = header.rsplit('[').next().unwrap_or_default();
        let Some((file, n)) = site.rsplit_once(':') else {
            continue;
        };
        if file.len() <= 96
            && file.ends_with(".cpp")
            && file
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            && !n.is_empty()
            && n.len() <= 10
            && n.bytes().all(|c| c.is_ascii_digit())
        {
            *sites.entry(site.into()).or_default() += 1;
            if let Some((_, tail)) = body.split_once("file: ") {
                if let Some(path) = tail.split_whitespace().next() {
                    if files.len() < 64
                        && path.len() <= 200
                        && path.ends_with(".txt")
                        && ["events/", "common/", "interface/"]
                            .iter()
                            .any(|p| path.starts_with(p))
                        && sources::relative(path).is_ok()
                        && path
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"/._-".contains(&c))
                    {
                        files.insert(path.to_owned());
                    }
                }
            }
        }
    }
    Ok(
        json!({"bytes":meta.len(),"sampledBytes":bytes.len(),"modifiedUnixSeconds":meta.modified().ok().and_then(|t|t.duration_since(UNIX_EPOCH).ok()).map(|d|d.as_secs()),"engineErrorSites":sites,"scriptFiles":files}),
    )
}

fn check(game: &Path, docs: &Path) -> (&'static str, script::Result<Value>) {
    let mut phase = "launcher selection";
    let result = (|| {
        let (mods, selection, _) = sources::selection(docs, None)?;
        phase = "source overlays";
        if !game.join("common/technology").is_dir() {
            return Err("Game technology directory not found".into());
        }
        let mut sources = sources::Sources::load(
            game,
            &mods,
            crate::languages::LANGUAGES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            true,
        )?;
        phase = "technology graph";
        let tree = catalog::collect(&mut sources)?;
        let mut counts = BTreeMap::<String, usize>::new();
        let mut examples = Vec::new();
        for w in &sources.warnings {
            let issue = w["issue"].as_str().unwrap_or("other");
            *counts.entry(issue.to_owned()).or_default() += 1;
            if examples.len() < 32
                && matches!(
                    issue,
                    "unresolved_technology_variables" | "unselected_declared_dependency"
                )
            {
                examples.push(json!({"issue":issue,"source":w["source"],"file":w["file"],"technology":w["id"],"dependency":w["dependency"],"details":w["details"]}));
            }
        }
        Ok(
            json!({"technologies":tree["technologies"].as_object().map(|t|t.len()),"selectionNotes":selection["warnings"],
            "warningCounts":counts,"examples":examples}),
        )
    })();
    (phase, result)
}

fn clean_values(v: &mut Value, roots: &[PathBuf]) {
    match v {
        Value::String(s) => *s = message(s, roots),
        Value::Object(map) => map.values_mut().for_each(|x| clean_values(x, roots)),
        Value::Array(list) => list.iter_mut().for_each(|x| clean_values(x, roots)),
        _ => (),
    }
}
pub fn run(mut args: impl Iterator<Item = OsString>) -> script::Result<()> {
    let (mut game, mut docs, mut output) = (None, None, None);
    while let Some(flag) = args.next() {
        let field = match flag.to_str() {
            Some("--game") => &mut game,
            Some("--documents") => &mut docs,
            Some("--output-parent") => &mut output,
            _ => {
                return Err("Support report accepts --game, --documents and --output-parent".into())
            }
        };
        *field = Some(PathBuf::from(
            args.next().ok_or("Missing support report argument")?,
        ));
    }
    collect(
        InputFolders {
            game: game.or_else(wizard::game_guess),
            docs: docs.or_else(wizard::documents_guess),
        },
        output,
        None,
    )
}

fn collect(
    inputs: InputFolders,
    output: Option<PathBuf>,
    failure: Option<(Value, &str)>,
) -> script::Result<()> {
    let InputFolders { game, docs } = inputs;
    let out = output
        .or_else(|| support_home().map(|p| p.join("Support")))
        .ok_or("Choose --output-parent for the support report")?;
    let roots: Vec<_> = [
        wizard::home(),
        game.clone(),
        docs.clone(),
        Some(out.clone()),
        package_root(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let stamp = now();
    let mut report = json!({"format":"tte-support-v1","collectedUnixMs":stamp as u64,"platform":env::consts::OS,"osVersion":os_version(),"architecture":env::consts::ARCH,
        "adapterCoreVersion":VERSION,"gameVersion":game.as_deref().map(web::installed_game_version),
        "packageSourceCommit":package_root().and_then(|r|small_json(&r.join("technology-adapter/WORKSHOP-PACKAGE.json")))
        .and_then(|m|m["sourceCommit"].as_str().filter(|s|s.len()==40 && s.bytes().all(|c|c.is_ascii_hexdigit())).map(str::to_owned))});
    report["lastRecordedFailure"]=failure.as_ref().map(|(record,_)| record.clone()).or_else(|| support_home().and_then(|p|small_json(&p.join(LAST))).map(|v|json!({
        "timeUnixMs":v["timeUnixMs"].as_u64(),"phase":v["phase"].as_u64(),"mode":v["mode"].as_str(),"message":v["message"].as_str()
    }))).unwrap_or(Value::Null);
    let section = |r: script::Result<Value>| r.unwrap_or_else(|e| json!({"unavailable":e}));
    if let Some(d) = &docs {
        report["gameSettings"] = json!(ui_settings(d));
        report["launcher"] = section(launcher(d));
        report["registrationFiles"] = section(registration_files(d).map(|v| json!(v)));
        report["lastLaunchRegistrations"] = small_json(&d.join("dlc_load.json"))
            .and_then(|v| {
                v["enabled_mods"].as_array().map(|a| {
                    a.iter()
                        .filter_map(|v| registry(v.as_str()?))
                        .collect::<Vec<_>>()
                })
            })
            .map(|v| json!(v))
            .unwrap_or(Value::Null);
        report["gameErrorLog"] =
            section(game_errors(&d.join("logs/error.log")).map_err(|e| e.to_string()));
    }
    clean_values(&mut report, &roots);
    let ancestor = out
        .ancestors()
        .find(|p| p.is_dir())
        .ok_or("Support output needs an existing parent")?;
    let out =
        sources::canonical(ancestor)?.join(out.strip_prefix(ancestor).map_err(|e| e.to_string())?);
    if out
        .components()
        .any(|c| c == std::path::Component::ParentDir)
        || [game.clone(), docs.clone(), package_root()]
            .into_iter()
            .flatten()
            .filter_map(|p| fs::canonicalize(p).ok())
            .any(|p| out.starts_with(p))
    {
        return Err("Choose a support output folder outside the game, Workshop item and Stellaris user data".into());
    }
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let stem = format!("TTE-Support-{stamp}-{}", std::process::id());
    let save = |p: &Path, v: &Value| {
        atomic_report::publish_with(p, |file| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            serde_json::to_writer_pretty(file, v).map_err(std::io::Error::other)
        })
        .map_err(|e| e.to_string())
    };
    if let Some((_, comment)) = failure {
        // The adapter has already observed the failure. Save one file without
        // probing a different playset or retrying an expensive failed parse.
        report["readOnlyCheck"] =
            json!({"status":"not run","reason":"Captured the original adapter failure"});
        let file = out.join(format!("{stem}.log"));
        report["workshopComment"] = json!(comment);
        save(&file, &report)?;
        eprintln!(
            "Full local log / 完整本地日志：{}",
            crate::safe(&sources::path_json(&file))
        );
        return Ok(());
    }
    let inventory = out.join(format!("{stem}-inventory.json"));
    let file = out.join(format!("{stem}.json"));
    report["readOnlyCheck"] = json!({"status":"not completed"});
    save(&inventory, &report)?;
    eprintln!(
        "Inventory saved / 已保存清单：{}",
        crate::safe(&sources::path_json(&inventory))
    );
    let mut probe = match (game.as_deref(), docs.as_deref()) {
        (Some(g), Some(d)) => {
            eprintln!("Checking this playset's technology graph / 正在检查当前播放集的科技数据…");
            let (phase, result) = check(g, d);
            match result {
                Ok(v) => json!({"status":"passed","phase":phase,"result":v}),
                Err(e) => json!({"status":"failed","phase":phase,"error":e}),
            }
        }
        _ => {
            json!({"status":"unavailable","reason":"Game or user data folder could not be detected"})
        }
    };
    let comment = if let Some(error) = probe["error"].as_str() {
        Some(workshop_comment(
            "diagnostic",
            probe["phase"].as_str().unwrap_or("check"),
            error,
            &roots,
        ))
    } else {
        let previous = &report["lastRecordedFailure"];
        let mode = match previous["mode"].as_str() {
            Some("adapter") => "previous adapter error",
            Some("browser") => "previous browser error",
            _ => "",
        };
        previous["phase"]
            .as_u64()
            .filter(|n| (1..=4).contains(n) && !mode.is_empty())
            .zip(previous["message"].as_str())
            .map(|(n, error)| workshop_comment(mode, &format!("step {n}/4"), error, &roots))
    };
    clean_values(&mut probe, &roots);
    report["readOnlyCheck"] = probe;
    if let Some(text) = &comment {
        report["workshopComment"] = json!(text);
        print_comment(text);
    }
    save(&file, &report)?;
    let _ = fs::remove_file(inventory);
    println!(
        "Full local report / 完整本地报告：{}",
        crate::safe(&sources::path_json(&file))
    );
    Ok(())
}
