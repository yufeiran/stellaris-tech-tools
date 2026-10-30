//! Auto-filled full-build entry, with manual fallback for unknown installations.
use crate::{script, Args};
use std::{
    env,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn home() -> Option<PathBuf> {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}
#[cfg(windows)]
fn documents() -> Option<PathBuf> {
    // Use the redirected Windows Known Folder, rather than assuming C:/Users.
    #[repr(C)]
    struct Guid {
        a: u32,
        b: u16,
        c: u16,
        d: [u8; 8],
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHGetKnownFolderPath(
            id: *const Guid,
            flags: u32,
            token: *mut std::ffi::c_void,
            path: *mut *mut u16,
        ) -> i32;
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(memory: *mut std::ffi::c_void);
    }
    let id = Guid {
        a: 0xFDD39AD0,
        b: 0x238F,
        c: 0x46AF,
        d: [0xAD, 0xB4, 0x6C, 0x85, 0x48, 0x03, 0x69, 0xC7],
    };
    let mut raw = std::ptr::null_mut();
    // Windows owns this null-terminated allocation; always free it after copy.
    unsafe {
        if SHGetKnownFolderPath(&id, 0, std::ptr::null_mut(), &mut raw) < 0 || raw.is_null() {
            CoTaskMemFree(raw.cast());
            return None;
        }
        let mut n = 0;
        while n < 32768 && *raw.add(n) != 0 {
            n += 1;
        }
        let result = if n < 32768 {
            String::from_utf16(std::slice::from_raw_parts(raw, n))
                .ok()
                .map(PathBuf::from)
        } else {
            None
        };
        CoTaskMemFree(raw.cast());
        result
    }
}
#[cfg(target_os = "macos")]
fn documents() -> Option<PathBuf> {
    home().map(|p| p.join("Documents"))
}
#[cfg(all(not(windows), not(target_os = "macos")))]
fn documents() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home().map(|p| p.join(".local/share")))
}

fn library_paths(raw: &str) -> script::Result<Vec<PathBuf>> {
    let ast = script::parse_vdf(raw)?;
    let root = ast
        .first("libraryfolders")
        .ok_or("No libraryfolders VDF root")?;
    Ok(root
        .entries()
        .iter()
        .filter(|e| {
            e.key
                .as_deref()
                .is_some_and(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_digit()))
        })
        .filter_map(|e| e.value.scalar("path").or_else(|| e.value.atom()))
        .map(PathBuf::from)
        .collect())
}

fn unique_game(steamapps: Vec<PathBuf>) -> Option<PathBuf> {
    let mut candidates = std::collections::BTreeSet::new();
    for library in steamapps {
        let p = library.join("common/Stellaris");
        if p.join("common/technology").is_dir() {
            if let Ok(p) = crate::sources::canonical(&p) {
                candidates.insert(p);
            }
        }
    }
    if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    }
}
fn game_guess() -> Option<PathBuf> {
    let mut steamapps = Vec::new();
    if let Ok(exe) = env::current_exe() {
        for p in exe.ancestors().take(10) {
            if p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("steamapps"))
            {
                steamapps.push(p.to_owned());
            }
        }
    }
    if let Some(h) = home() {
        for relative in [
            ".steam/steam/steamapps",
            ".local/share/Steam/steamapps",
            "Library/Application Support/Steam/steamapps",
        ] {
            steamapps.push(h.join(relative));
        }
    }
    for key in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(p) = env::var_os(key) {
            steamapps.push(PathBuf::from(p).join("Steam/steamapps"));
        }
    }
    let initial = steamapps.clone();
    for p in initial {
        let Ok(raw) = std::fs::read_to_string(p.join("libraryfolders.vdf")) else {
            continue;
        };
        if raw.len() > 1024 * 1024 {
            continue;
        }
        if let Ok(paths) = library_paths(&raw) {
            steamapps.extend(paths.into_iter().map(|p| p.join("steamapps")));
        }
    }
    unique_game(steamapps)
}
fn pasted_path(value: &str, unix: bool) -> script::Result<String> {
    let s = value.trim();
    if let Some(inner) = s
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
    {
        return Ok(inner.into());
    }
    if !unix {
        return Ok(s.into());
    }
    let mut chars = s.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' {
            chars
                .next()
                .ok_or("Incomplete escaped path; quote the full path")?
        } else {
            c
        });
    }
    Ok(out)
}
fn question(
    input: &mut impl BufRead,
    output: &mut impl Write,
    label: &str,
    default: Option<&Path>,
) -> script::Result<String> {
    write!(
        output,
        "{label}{}: ",
        default
            .map(|p| format!(" [{}]", p.display()))
            .unwrap_or_default()
    )
    .map_err(|e| e.to_string())?;
    output.flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    if input.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
        return Err("Interactive input ended; nothing was registered".into());
    }
    let value = pasted_path(&line, !cfg!(windows))?;
    if value.chars().any(|c| c.is_control()) {
        return Err("Control characters in input".into());
    }
    Ok(if value.is_empty() {
        default
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        value
    })
}
pub fn guided(
    input: &mut impl BufRead,
    output: &mut impl Write,
    guessed_game: Option<&Path>,
    guessed_docs: Option<&Path>,
    guessed_output: Option<&Path>,
    stamp: u128,
) -> script::Result<Args> {
    writeln!(output,"TTE native adapter PREVIEW / 通用科技适配工具预览\nNo Python required. Reads your selected sources; never edits them.\n无需 Python。仅收录选定来源，不修改原 Mod。\nGenerated game content still requires testing. / 生成内容仍需游戏内验证。").map_err(|e|e.to_string())?;
    let game = question(
        input,
        output,
        "Stellaris game folder / 群星游戏目录",
        guessed_game,
    )?;
    if game.is_empty() {
        return Err("Game folder is required".into());
    }
    let mode = question(
        input,
        output,
        "Sources: 1=current playset, 2=game only / 来源：1 当前播放集，2 仅本体",
        None,
    )?;
    let docs = match mode.as_str() {
        "1" => {
            let d = question(
                input,
                output,
                "Stellaris user-data folder / 群星用户目录",
                guessed_docs,
            )?;
            if d.is_empty() {
                return Err("Documents folder is required for playset collection".into());
            }
            Some(PathBuf::from(d))
        }
        "2" => None,
        _ => return Err("Choose 1 or 2 explicitly; no source mods were silently omitted".into()),
    };
    let parent = PathBuf::from(question(
        input,
        output,
        "Existing output folder / 已存在的输出目录",
        guessed_output,
    )?);
    if !parent.is_dir() {
        return Err("Output parent must be an existing directory".into());
    }
    let registration_docs = if docs.is_some() {
        docs.clone()
    } else {
        let value=question(input,output,"Optional Stellaris user-data folder for launcher registration (blank = skip) / 可选：登记 Mod 使用的群星用户目录（留空跳过）",None)?;
        if value.is_empty() {
            None
        } else {
            Some(PathBuf::from(value))
        }
    };
    let register = if let Some(docs) = &registration_docs {
        match question(input,output,"Add this NEW local Mod to the launcher library? y/n (does not enable it) / 登记新的本地 Mod？y/n（不会自动启用）",None)?.as_str(){
            "y"|"Y"=>Some(docs.clone()),"n"|"N"=>None,_=>return Err("Choose y or n explicitly".into())
        }
    } else {
        None
    };
    let prefix = format!("TTE-Adapter-{stamp}");
    Ok(Args {
        game: Some(game.into()),
        docs,
        output: Some(parent.join(format!("{prefix}.json"))),
        build: Some(parent.join(prefix)),
        register,
        languages: vec!["english".into(), "simp_chinese".into()],
        ..Args::default()
    })
}
pub fn automatic(
    input: &mut impl BufRead,
    output: &mut impl Write,
    guessed_game: Option<&Path>,
    guessed_docs: Option<&Path>,
    parent: &Path,
    stamp: u128,
    workshop: Option<crate::workshop::Target>,
) -> script::Result<Args> {
    writeln!(output,"Tech Tree & Research Planner — Technology Adapter / 科技适配工具\nNo Python required. / 无需安装 Python。") .map_err(|e|e.to_string())?;
    let game = match guessed_game {
        Some(path) => path.to_owned(),
        None => PathBuf::from(question(
            input,
            output,
            "Game folder not uniquely detected / 未找到唯一的群星安装，请输入游戏目录",
            None,
        )?),
    };
    if !game.join("common/technology").is_dir() {
        return Err("Game folder needs common/technology / 群星目录不正确".into());
    }
    let docs = match guessed_docs {
        Some(path) => path.to_owned(),
        None => PathBuf::from(question(
            input,
            output,
            "User-data folder not detected / 未找到用户目录，请输入群星用户目录",
            None,
        )?),
    };
    if !docs.join("launcher-v2.sqlite").is_file() || !docs.join("mod").is_dir() {
        return Err("Stellaris user-data/mod and launcher database are required. Start the game and launcher once first. / 请先启动一次群星和启动器，生成用户目录与播放集。".into());
    }
    let (mods, selection, guard) = crate::sources::selection(&docs, None)?;
    let id = selection["playsetId"]
        .as_str()
        .ok_or("Selected playset ID missing")?
        .to_owned();
    let ancestor = parent
        .ancestors()
        .find(|p| p.is_dir())
        .ok_or("Output folder has no existing parent")?;
    let suffix = parent.strip_prefix(ancestor).map_err(|e| e.to_string())?;
    if suffix.components().any(|c| {
        !matches!(
            c,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    }) {
        return Err("Use a simple absolute output folder without parent traversal".into());
    }
    let parent = crate::sources::canonical(ancestor)?.join(suffix);
    if workshop
        .as_ref()
        .is_some_and(|t| parent.starts_with(&t.root))
    {
        return Err(
            "Working output must be outside the Workshop Mod folder; nothing generated".into(),
        );
    }
    for root in std::iter::once(&game)
        .chain(std::iter::once(&docs))
        .chain(mods.iter())
    {
        if parent.starts_with(crate::sources::canonical(root)?) {
            return Err(
                "Adapter output must be outside game, source Mod and user-data folders".into(),
            );
        }
    }
    writeln!(output,"\nGame / 游戏：{}\nUser data / 用户目录：{}\nPlayset / 播放集：{}\nSource mods / 来源 Mod：{}\nWorking files / 生成记录位置：{}",
        crate::safe(&game.display().to_string()),crate::safe(&docs.display().to_string()),
        crate::safe(selection["playsetName"].as_str().unwrap_or(&id)),mods.len(),crate::safe(&parent.display().to_string())).map_err(|e|e.to_string())?;
    if let Some(target) = &workshop {
        writeln!(output,"Update THIS Workshop Mod / 更新当前工坊 Mod：{}\nNo new Local Mod; playset stays unchanged. Backup before replacing the planner archive.\n不创建本地 Mod、不切换播放集；替换前备份本 Mod 的数据包。", crate::safe(&target.root.display().to_string())).map_err(|e|e.to_string())?;
    } else {
        writeln!(output,"Action: build and register a NEW local planner; keep playset selection unchanged.\n操作：生成并登记本地规划器，不自动切换播放集。").map_err(|e|e.to_string())?;
    }
    for source in &mods {
        writeln!(output, "  {}", crate::safe(&source.display().to_string()))
            .map_err(|e| e.to_string())?;
    }
    for warning in selection["warnings"].as_array().into_iter().flatten() {
        writeln!(
            output,
            "  Note / 提示：{}",
            crate::safe(&warning.to_string())
        )
        .map_err(|e| e.to_string())?;
    }
    let answer = question(
        input,
        output,
        "Enter = build; e = edit; q = cancel / 回车开始，e 修改，q 取消",
        None,
    )?;
    if answer.eq_ignore_ascii_case("e") {
        if workshop.is_some() {
            let game = PathBuf::from(question(
                input,
                output,
                "Game folder / 游戏目录",
                Some(&game),
            )?);
            let docs = PathBuf::from(question(
                input,
                output,
                "User data / 用户目录",
                Some(&docs),
            )?);
            let parent = PathBuf::from(question(
                input,
                output,
                "Working files / 生成记录目录",
                Some(&parent),
            )?);
            return automatic(
                input,
                output,
                Some(&game),
                Some(&docs),
                &parent,
                stamp,
                workshop,
            );
        }
        return guided(
            input,
            output,
            Some(&game),
            Some(&docs),
            parent
                .is_dir()
                .then_some(parent.as_path())
                .or_else(|| parent.parent()),
            stamp,
        );
    }
    if !answer.is_empty() && !answer.eq_ignore_ascii_case("y") {
        return Err("Cancelled; no planner generated / 已取消，未生成适配包".into());
    }
    crate::dbsnapshot::verify(&guard)?;
    std::fs::create_dir_all(&parent).map_err(|e| format!("Create adapter output folder: {e}"))?;
    let prefix = format!("TTE-Adapter-{stamp}");
    Ok(Args {
        game: Some(game),
        docs: Some(docs.clone()),
        playset: Some(id),
        output: Some(parent.join(format!("{prefix}.json"))),
        build: Some(parent.join(prefix)),
        register: workshop.is_none().then_some(docs),
        selection_guard: guard,
        previewed_mods: Some(mods),
        workshop,
        languages: vec!["english".into(), "simp_chinese".into()],
        ..Args::default()
    })
}
pub fn run(
    game_override: Option<&Path>,
    docs_override: Option<&Path>,
    parent_override: Option<&Path>,
    in_workshop: bool,
) -> script::Result<Args> {
    let workshop = if in_workshop {
        Some(crate::workshop::Target::from_exe()?)
    } else {
        None
    };
    let g = game_override.map(Path::to_owned).or_else(game_guess);
    let d = docs_override.map(Path::to_owned).or_else(|| {
        documents()
            .map(|p| p.join("Paradox Interactive/Stellaris"))
            .filter(|p| p.join("launcher-v2.sqlite").is_file())
    });
    let o = parent_override
        .map(Path::to_owned)
        .or_else(|| home().map(|p| p.join("TTE-Research-Planner")))
        .ok_or("Home/output folder not detected")?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    automatic(
        &mut std::io::stdin().lock(),
        &mut std::io::stderr(),
        g.as_deref(),
        d.as_deref(),
        &o,
        stamp,
        workshop,
    )
}
/// Read-only counterpart of the adapter preview. No Target/install/register path.
pub fn web_automatic(
    input: &mut impl BufRead,
    output: &mut impl Write,
    guessed_game: Option<&Path>,
    guessed_docs: Option<&Path>,
    parent: &Path,
    stamp: u128,
    open_browser: bool,
) -> script::Result<Args> {
    writeln!(output, "Tech Tree & Research Planner — Browser tree / 浏览器科技树\nReads source definitions only; no save or planner changes. / 只读科技资料，不修改存档或游戏 Mod。")
        .map_err(|e| e.to_string())?;
    let game = match guessed_game {
        Some(path) => path.to_owned(),
        None => PathBuf::from(question(
            input,
            output,
            "Stellaris game folder / 群星游戏目录",
            None,
        )?),
    };
    if !game.join("common/technology").is_dir() {
        return Err("Game folder needs common/technology / 群星目录不正确".into());
    }
    let docs = match guessed_docs {
        Some(path) => path.to_owned(),
        None => PathBuf::from(question(
            input,
            output,
            "Stellaris user-data folder / 群星用户目录",
            None,
        )?),
    };
    let (mods, selection, guard) = crate::sources::selection(&docs, None)?;
    let id = selection["playsetId"]
        .as_str()
        .ok_or("Selected playset ID missing")?
        .to_owned();
    let ancestor = parent
        .ancestors()
        .find(|p| p.is_dir())
        .ok_or("Output folder has no existing parent")?;
    let suffix = parent.strip_prefix(ancestor).map_err(|e| e.to_string())?;
    if suffix.components().any(|c| {
        !matches!(
            c,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    }) {
        return Err("Use an absolute output folder without parent traversal".into());
    }
    let parent = crate::sources::canonical(ancestor)?.join(suffix);
    for source in std::iter::once(&game)
        .chain(std::iter::once(&docs))
        .chain(mods.iter())
    {
        if parent.starts_with(crate::sources::canonical(source)?) {
            return Err("Browser working files must be outside source/user-data folders".into());
        }
    }
    writeln!(
        output,
        "\nGame / 游戏：{}\nPlayset / 播放集：{}\nSource mods / 来源 Mod：{}",
        crate::safe(&game.display().to_string()),
        crate::safe(selection["playsetName"].as_str().unwrap_or(&id)),
        mods.len()
    )
    .map_err(|e| e.to_string())?;
    for source in &mods {
        writeln!(output, "  {}", crate::safe(&source.display().to_string()))
            .map_err(|e| e.to_string())?;
    }
    let answer = question(
        input,
        output,
        "Enter = open tree; e = edit; q = cancel / 回车打开，e 修改，q 取消",
        None,
    )?;
    if answer.eq_ignore_ascii_case("e") {
        let game = PathBuf::from(question(
            input,
            output,
            "Game folder / 游戏目录",
            Some(&game),
        )?);
        let docs = PathBuf::from(question(
            input,
            output,
            "User data / 用户目录",
            Some(&docs),
        )?);
        return web_automatic(
            input,
            output,
            Some(&game),
            Some(&docs),
            &parent,
            stamp,
            open_browser,
        );
    }
    if !answer.is_empty() && !answer.eq_ignore_ascii_case("y") {
        return Err("Cancelled; no browser server started / 已取消，未启动本地服务".into());
    }
    crate::dbsnapshot::verify(&guard)?;
    std::fs::create_dir_all(&parent).map_err(|e| format!("Create browser working folder: {e}"))?;
    Ok(Args {
        game: Some(game),
        docs: Some(docs),
        playset: Some(id),
        output: Some(parent.join(format!("TTE-Web-Atlas-{stamp}.json"))),
        selection_guard: guard,
        previewed_mods: Some(mods),
        languages: vec!["english".into(), "simp_chinese".into()],
        web: true,
        open_browser,
        ..Args::default()
    })
}

pub fn web_run(
    game: Option<&Path>,
    docs: Option<&Path>,
    parent: Option<&Path>,
    open_browser: bool,
) -> script::Result<Args> {
    let game = game.map(Path::to_owned).or_else(game_guess);
    let docs = docs.map(Path::to_owned).or_else(|| {
        documents()
            .map(|p| p.join("Paradox Interactive/Stellaris"))
            .filter(|p| p.join("launcher-v2.sqlite").is_file())
    });
    let parent = parent
        .map(Path::to_owned)
        .or_else(|| home().map(|p| p.join("TTE-Research-Planner")))
        .ok_or("Home/output folder not detected")?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    web_automatic(
        &mut std::io::stdin().lock(),
        &mut std::io::stderr(),
        game.as_deref(),
        docs.as_deref(),
        &parent,
        stamp,
        open_browser,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn several_installed_games_require_manual_choice_not_first_library() {
        let t = env::temp_dir().join(format!(
            "tte-wizard-libraries-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&t).unwrap();
        let a = t.join("a/steamapps");
        let b = t.join("b/steamapps");
        std::fs::create_dir_all(a.join("common/Stellaris/common/technology")).unwrap();
        assert_eq!(
            unique_game(vec![a.clone(), a.clone()]),
            crate::sources::canonical(&a.join("common/Stellaris")).ok()
        );
        std::fs::create_dir_all(b.join("common/Stellaris/common/technology")).unwrap();
        assert!(unique_game(vec![a, b]).is_none());
        let resolved = std::fs::canonicalize(&t).unwrap();
        assert_eq!(
            resolved.parent(),
            Some(std::fs::canonicalize(env::temp_dir()).unwrap().as_path())
        );
        assert!(resolved
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("tte-wizard-libraries-"));
        std::fs::remove_dir_all(resolved).unwrap();
    }
    #[test]
    fn terminal_drag_paths_are_decoded_as_data_only() {
        assert_eq!(
            pasted_path(r"/Games/中文\ path/\(test\)", true).unwrap(),
            "/Games/中文 path/(test)"
        );
        assert_eq!(
            pasted_path(r"E:\Games\中文 path", false).unwrap(),
            r"E:\Games\中文 path"
        );
        assert_eq!(
            pasted_path("'/tmp/$HOME $(example)'", true).unwrap(),
            "/tmp/$HOME $(example)"
        );
        assert!(pasted_path("/tmp/broken\\", true).is_err());
    }
    #[test]
    fn valve_libraries_use_pairs_and_support_legacy_paths() {
        let raw = r#"// Valve library config
        "libraryfolders" { "0" { "path" "E:\\SteamLibrary" "apps" { "281990" "123" } }
        "1" "/mnt/Games 中文" "contentstatsid" "ignored" }"#;
        assert_eq!(
            library_paths(raw).unwrap(),
            vec![
                PathBuf::from(r"E:\SteamLibrary"),
                PathBuf::from("/mnt/Games 中文")
            ]
        );
        assert!(library_paths("\"libraryfolders\" { \"0\" { \"path\" }").is_err());
        assert!(library_paths("libraryfolders={}").is_err());
    }
    #[test]
    fn registration_blank_really_skips_even_when_docs_were_detected() {
        let temp = env::temp_dir();
        let mut out = Vec::new();
        let a = guided(
            &mut std::io::Cursor::new(b"'game path'\n2\n\n\n"),
            &mut out,
            None,
            Some(&temp),
            Some(&temp),
            4,
        )
        .unwrap();
        assert_eq!(a.game, Some(PathBuf::from("game path")));
        assert!(a.register.is_none());
    }
    #[test]
    fn full_wizard_never_silently_chooses_vanilla_or_registration() {
        let temp = env::temp_dir();
        let mut out = Vec::new();
        assert!(guided(
            &mut std::io::Cursor::new(b"game\n\n"),
            &mut out,
            None,
            None,
            Some(&temp),
            1
        )
        .is_err());
        let a = guided(
            &mut std::io::Cursor::new(b"game\n1\ndocs\n\nn\n"),
            &mut out,
            None,
            None,
            Some(&temp),
            2,
        )
        .unwrap();
        assert!(a.build.is_some());
        assert!(a.register.is_none());
        assert_eq!(a.docs, Some(PathBuf::from("docs")));
        let b = guided(
            &mut std::io::Cursor::new(b"game\n2\n\n\n"),
            &mut out,
            None,
            None,
            Some(&temp),
            3,
        )
        .unwrap();
        assert!(b.docs.is_none());
        assert!(b.register.is_none());
        assert_ne!(a.output, b.output);
    }
}
