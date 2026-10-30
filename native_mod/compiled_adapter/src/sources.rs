use crate::script::{self, Result, Value};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub const OWN_ID: &str = "3818261596";
const MAX_FILE: u64 = 64 * 1024 * 1024;
pub fn sha(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}
fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_FILE {
        return Err(format!("Source file exceeds 64 MiB: {}", path.display()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("Source grew beyond 64 MiB: {}", path.display()));
    }
    Ok(bytes)
}
pub fn text(path: &Path) -> Result<String> {
    let bytes = read_bytes(path)?;
    Ok(script::normalise(std::str::from_utf8(&bytes).map_err(
        |e| format!("{}: not UTF-8: {e}", path.display()),
    )?))
}
pub fn canonical(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))
}
pub fn relative(name: &str) -> Result<String> {
    let s = name.replace('\\', "/");
    let s = s.trim_end_matches('/');
    if s.is_empty()
        || s.starts_with('/')
        || s.contains(':')
        || s.split('/').any(|x| x == ".." || x == "." || x.is_empty())
    {
        return Err(format!("Unsafe source-relative path: {name}"));
    }
    Ok(s.to_owned())
}
pub fn path_json(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if let Some(unc) = s.strip_prefix("//?/UNC/") {
        return format!("//{unc}");
    }
    s.strip_prefix("//?/").unwrap_or(&s).to_owned()
}

#[derive(Clone)]
pub struct Layer {
    pub root: PathBuf,
    pub name: String,
    pub metadata: Value,
    pub descriptor_hash: Option<String>,
    pub archive_hash: Option<String>,
    pub archive_entries: Option<BTreeSet<String>>,
}
#[derive(Clone)]
pub struct Input {
    pub layer: usize,
    pub relative: String,
    pub raw: String,
    pub hash: String,
}
pub struct Sources {
    pub layers: Vec<Layer>,
    pub files: BTreeMap<String, Input>,
    pub collisions: Vec<Json>,
    pub replacements: Vec<Json>,
    pub warnings: Vec<Json>,
    pub inline_reads: BTreeSet<String>,
    pub languages: Vec<String>,
    pub include_ui: bool,
}

fn is_self(metadata: &Value, root: Option<&Path>) -> bool {
    metadata.scalar("remote_file_id") == Some(OWN_ID)
        || root.is_some_and(|p| {
            p.join("tte-build.json").is_file()
                || p.join("tte-companion.json").is_file()
                || p.join("tte-generating.json").is_file()
        })
}

pub fn selection(
    documents: &Path,
    playset: Option<&str>,
) -> Result<(Vec<PathBuf>, Json, crate::dbsnapshot::InputHashes)> {
    let docs = canonical(documents)?;
    let snapshot = crate::dbsnapshot::Snapshot::new(&docs.join("launcher-v2.sqlite"))?;
    let mut hashes = snapshot.hashes.clone();
    let db = Connection::open_with_flags(
        &snapshot.database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Read-only launcher database: {e}"))?;
    db.execute_batch("PRAGMA query_only=ON;")
        .map_err(|e| e.to_string())?;
    let sql = if playset.is_some() {
        "select id,name from playsets where id=?1"
    } else {
        "select id,name from playsets where isActive=1 and ?1 is null"
    };
    let selected: Vec<(String, String)> = db
        .prepare(sql)
        .map_err(|e| e.to_string())?
        .query_map([playset], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| e.to_string())?;
    if selected.len() != 1 {
        return Err("Choose exactly one active playset or specify --playset ID".into());
    }
    let (id, name) = &selected[0];
    let mut stmt=db.prepare("select m.gameRegistryId,m.dirPath,m.archivePath,m.status,pm.position from playsets_mods pm join mods m on m.id=pm.modId where pm.playsetId=?1 and pm.enabled=1 order by pm.position").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut roots = Vec::new();
    let mut excluded = Vec::new();
    let mut positions = BTreeSet::new();
    let mut registries = Vec::new();
    for row in rows {
        let (registry, directory, archive, status, position) = row.map_err(|e| e.to_string())?;
        if !positions.insert(position) {
            return Err("Ambiguous source load-order positions".into());
        }
        let rel = relative(&registry)?;
        if !rel.starts_with("mod/") || !rel.ends_with(".mod") || rel.split('/').count() != 2 {
            return Err(format!("Invalid launcher registration {rel}"));
        }
        let entry = canonical(&docs.join(&rel))?;
        if entry.parent() != Some(canonical(&docs.join("mod"))?.as_path()) {
            return Err("Launcher registration escapes Documents/mod".into());
        }
        crate::dbsnapshot::track(&entry, &mut hashes)?;
        let meta = script::parse(&text(&entry)?)?;
        for key in ["name", "remote_file_id", "path", "archive"] {
            if meta.all(key).count() > 1 {
                return Err(format!(
                    "{rel}: ambiguous duplicate registration field {key}"
                ));
            }
        }
        let root = meta.scalar("path").map(PathBuf::from);
        registries.push(rel.clone());
        if is_self(&meta, root.as_deref().filter(|p| p.is_absolute())) {
            excluded.push(json!({"registry":rel,"reason":"planner itself"}));
            continue;
        }
        if status != "ready_to_play" {
            return Err(format!("{rel}: source is not ready_to_play"));
        }
        let has_archive = meta.scalar("archive").is_some_and(|s| !s.is_empty())
            || archive.as_deref().is_some_and(|s| !s.is_empty());
        let (root, inner) = if has_archive {
            if root.as_ref().is_some_and(|p| !p.as_os_str().is_empty())
                || directory.as_deref().is_some_and(|s| !s.is_empty())
            {
                return Err(format!("{rel}: conflicting path/archive registration; refresh this source in the launcher before generating"));
            }
            let resolve = |s: &str| -> Result<PathBuf> {
                let p = PathBuf::from(s);
                canonical(&if p.is_absolute() {
                    p
                } else {
                    docs.join(relative(s)?)
                })
            };
            let root = resolve(
                meta.scalar("archive")
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| format!("{rel}: missing archive registration"))?,
            )?;
            let dbroot = resolve(
                archive
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| format!("{rel}: missing launcher archivePath"))?,
            )?;
            if root != dbroot || !root.is_file() {
                return Err(format!("{rel}: launcher/source ZIP mismatch"));
            }
            let zip = crate::archive::Archive::open(&root, |_| false)?;
            hashes.push((root.clone(), Some(zip.hash.clone())));
            (root, script::parse(&zip.descriptor()?)?)
        } else {
            let root = root.ok_or_else(|| format!("{rel}: missing absolute source path"))?;
            if !root.is_absolute() {
                return Err(format!("{rel}: source is not an absolute directory"));
            }
            let root = canonical(&root)?;
            if directory
                .as_deref()
                .map(Path::new)
                .map(canonical)
                .transpose()?
                .as_ref()
                != Some(&root)
                || !root.is_dir()
            {
                return Err(format!("{rel}: launcher/source directory mismatch"));
            }
            crate::dbsnapshot::track(&root.join("descriptor.mod"), &mut hashes)?;
            let inner = script::parse(&text(&root.join("descriptor.mod"))?)?;
            (root, inner)
        };
        if meta.scalar("name") != inner.scalar("name")
            || meta.scalar("remote_file_id") != inner.scalar("remote_file_id")
        {
            return Err(format!("{rel}: descriptor identity mismatch"));
        }
        for key in ["replace_path", "dependencies"] {
            let a: Vec<_> = meta.all(key).map(script::data).collect();
            let b: Vec<_> = inner.all(key).map(script::data).collect();
            if a != b {
                return Err(format!(
                    "{rel}: registration/descriptor {key} mismatch; refresh source registration"
                ));
            }
        }
        roots.push(root);
    }
    let mut warnings = Vec::new();
    let load = docs.join("dlc_load.json");
    crate::dbsnapshot::track(&load, &mut hashes)?;
    if load.is_file() {
        let last: Json = serde_json::from_str(&text(&load)?).map_err(|e| e.to_string())?;
        if last.get("enabled_mods") != Some(&json!(registries)) {
            warnings.push(
                "Last launched mod list differs; using the chosen playset, not running game state",
            );
        }
    }
    Ok((
        roots,
        json!({"kind":"launcher-playset","playsetId":id,"playsetName":name,"excluded":excluded,"warnings":warnings}),
        hashes,
    ))
}

fn enumerate(root: &Path, folder: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !folder.exists() {
        return Ok(());
    }
    for item in fs::read_dir(folder).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let p = item.path();
        let ty = item.file_type().map_err(|e| e.to_string())?;
        if ty.is_symlink() {
            let target = canonical(&p)?;
            if !target.starts_with(root) || target.is_dir() {
                return Err(format!(
                    "Unsupported/escaping source symlink: {}",
                    p.display()
                ));
            }
        }
        if ty.is_dir() {
            enumerate(root, &p, out)?;
        } else if p.is_file() {
            out.push(p);
        }
    }
    Ok(())
}

const SOURCE_FOLDERS: [&str; 14] = [
    "common/technology",
    "common/inline_scripts",
    "common/scripted_variables",
    "common/scripted_triggers",
    "common/buildings",
    "common/districts",
    "common/starbase_buildings",
    "common/starbase_modules",
    "common/armies",
    "common/megastructures",
    "common/component_templates",
    "interface",
    "localisation",
    "localisation_synced",
];
fn wanted(key: &str, languages: &[String], include_ui: bool) -> bool {
    if ![".txt", ".yml", ".gui", ".gfx"]
        .iter()
        .any(|suffix| key.ends_with(suffix))
    {
        return false;
    }
    if !SOURCE_FOLDERS.iter().any(|folder| {
        key.starts_with(&format!("{folder}/"))
            && (include_ui
                || (*folder != "interface" && !crate::metadata::is_object_folder(folder)))
    }) {
        return false;
    }
    if key.ends_with(".yml") {
        let known = [
            "english",
            "simp_chinese",
            "braz_por",
            "french",
            "german",
            "spanish",
            "polish",
            "russian",
            "japanese",
            "korean",
            "tchinese",
        ];
        if known
            .iter()
            .find(|lang| {
                key.contains(&format!("/{lang}/")) || key.ends_with(&format!("_l_{lang}.yml"))
            })
            .is_some_and(|lang| !languages.iter().any(|x| x == lang))
        {
            return false;
        }
    }
    true
}

impl Sources {
    pub fn load(
        game: &Path,
        mods: &[PathBuf],
        languages: Vec<String>,
        include_ui: bool,
    ) -> Result<Self> {
        let mut result = Self {
            layers: Vec::new(),
            files: BTreeMap::new(),
            collisions: Vec::new(),
            replacements: Vec::new(),
            warnings: Vec::new(),
            inline_reads: BTreeSet::new(),
            languages,
            include_ui,
        };
        let mut names = BTreeSet::new();
        for (index, root) in std::iter::once(game)
            .chain(mods.iter().map(PathBuf::as_path))
            .enumerate()
        {
            let root = canonical(root)?;
            if index == 0 && !root.is_dir() {
                return Err("Game source must be a directory".into());
            }
            let mut archive = if root.is_file() {
                Some(crate::archive::Archive::open(&root, |key| {
                    wanted(key, &result.languages, include_ui)
                })?)
            } else if root.is_dir() {
                None
            } else {
                return Err(format!("Not a source directory or ZIP: {}", root.display()));
            };
            let descriptor = root.join("descriptor.mod");
            let raw = if index > 0 {
                Some(match &archive {
                    Some(a) => a.descriptor()?,
                    None => text(&descriptor)?,
                })
            } else {
                None
            };
            let meta = raw
                .as_ref()
                .map(|s| script::parse(s))
                .transpose()?
                .unwrap_or(Value::Block(vec![]));
            if index > 0
                && (is_self(&meta, Some(&root))
                    || archive.as_ref().is_some_and(|a| {
                        a.entries.contains("tte-build.json")
                            || a.entries.contains("tte-companion.json")
                            || a.entries.contains("tte-generating.json")
                    }))
            {
                return Err("Do not import the planner itself".into());
            }
            if meta.first("archive").is_some() || meta.first("path").is_some() {
                return Err(
                    "Source descriptor.mod must not redirect to another path/archive".into(),
                );
            }
            let name = if index == 0 {
                "Stellaris".to_owned()
            } else {
                meta.scalar("name").unwrap_or("Unnamed mod").to_owned()
            };
            if !names.insert(name.clone()) {
                return Err(format!("Duplicate source name: {name}"));
            }
            for value in meta.all("replace_path") {
                let prefix =
                    relative(value.atom().ok_or("replace_path must be scalar")?)?.to_lowercase();
                let before = result.files.len();
                result
                    .files
                    .retain(|key, _| key != &prefix && !key.starts_with(&(prefix.clone() + "/")));
                result.replacements.push(
                    json!({"layer":index,"prefix":prefix,"removedFiles":before-result.files.len()}),
                );
            }
            result.layers.push(Layer {
                root: root.clone(),
                name,
                metadata: meta,
                descriptor_hash: raw.map(|s| sha(s.as_bytes())),
                archive_hash: archive.as_ref().map(|a| a.hash.clone()),
                archive_entries: archive.as_mut().map(|a| std::mem::take(&mut a.entries)),
            });
            let inputs = if let Some(a) = archive {
                a.texts
                    .into_values()
                    .filter(|(rel, _)| wanted(&rel.to_lowercase(), &result.languages, include_ui))
                    .collect::<Vec<_>>()
            } else {
                let mut candidates = Vec::new();
                for folder in SOURCE_FOLDERS {
                    if !include_ui
                        && (folder == "interface" || crate::metadata::is_object_folder(folder))
                    {
                        continue;
                    }
                    enumerate(&root, &root.join(folder), &mut candidates)?;
                }
                candidates.sort();
                let mut inputs = Vec::new();
                for path in candidates {
                    let rel = path
                        .strip_prefix(&root)
                        .map_err(|e| e.to_string())?
                        .to_string_lossy()
                        .replace('\\', "/");
                    if !wanted(&rel.to_lowercase(), &result.languages, include_ui) {
                        continue;
                    }
                    if !canonical(&path)?.starts_with(&root) {
                        return Err(format!("Source path escapes root: {rel}"));
                    }
                    inputs.push((rel, read_bytes(&path)?));
                }
                inputs
            };
            let mut seen = BTreeSet::new();
            for (rel, bytes) in inputs {
                let key = rel.to_lowercase();
                if !seen.insert(key.clone()) {
                    return Err(format!("Case-colliding source file: {rel}"));
                }
                let raw = script::normalise(
                    std::str::from_utf8(&bytes).map_err(|e| format!("{rel}: not UTF-8: {e}"))?,
                );
                let hash = sha(&bytes);
                if let Some(previous) = result.files.get(&key) {
                    result.collisions.push(
                        json!({"file":rel,"winnerLayer":index,"shadowedLayer":previous.layer}),
                    );
                }
                result.files.insert(
                    key,
                    Input {
                        layer: index,
                        relative: rel,
                        raw,
                        hash,
                    },
                );
            }
        }
        for layer in result.layers.iter().skip(1) {
            if let Some(deps) = layer.metadata.first("dependencies") {
                for entry in deps.entries() {
                    let name = entry.value.atom().ok_or("Dependency name must be scalar")?;
                    if !names.contains(name) {
                        return Err(format!("{}: missing source dependency {name}", layer.name));
                    }
                }
            }
        }
        Ok(result)
    }

    pub fn expand(&mut self, raw: &str, stack: &[String]) -> Result<String> {
        self.expand_inner(raw, stack, &mut 0)
    }
    fn expand_inner(
        &mut self,
        raw: &str,
        stack: &[String],
        calls_used: &mut usize,
    ) -> Result<String> {
        const MAX_EXPANDED: usize = 64 * 1024 * 1024;
        if raw.len() > MAX_EXPANDED {
            return Err("Expanded script exceeds 64 MiB budget".into());
        }
        if !stack.is_empty()
            && (raw.contains("[[")
                || raw
                    .split('$')
                    .enumerate()
                    .any(|(i, s)| i % 2 == 1 && s.contains('|')))
        {
            return Err(
                "Inline conditional/default parameters are not supported; refusing to guess".into(),
            );
        }
        let parsed = script::parse(raw)?;
        let mut calls = Vec::new();
        fn find(value: &Value, out: &mut Vec<crate::script::Entry>) {
            for e in value.entries() {
                if e.key.as_deref() == Some("inline_script") {
                    out.push(e.clone());
                } else {
                    find(&e.value, out);
                }
            }
        }
        find(&parsed, &mut calls);
        let mut result = raw.to_owned();
        for call in calls.into_iter().rev() {
            *calls_used += 1;
            if *calls_used > 4096 {
                return Err("Inline expansion call budget exceeded".into());
            }
            let (name, args) = match &call.value {
                Value::Atom(name) => (name.to_owned(), vec![]),
                block => {
                    if block.all("script").count() != 1 {
                        return Err("Inline script requires one unambiguous script name".into());
                    }
                    let name = block
                        .scalar("script")
                        .ok_or("Inline script has no literal name")?
                        .to_owned();
                    let mut args = Vec::new();
                    let mut seen = BTreeSet::new();
                    for e in block.entries() {
                        let key = e.key.as_deref().ok_or("Bare inline-script argument")?;
                        if key == "script" {
                            continue;
                        }
                        if !seen.insert(key) {
                            return Err("Duplicate inline-script parameter".into());
                        }
                        let value = e
                            .value
                            .atom()
                            .ok_or("Inline block argument is unsupported")?;
                        if value.is_empty()
                            || value.chars().any(|c| {
                                c.is_whitespace() || c.is_control() || "{}=#$\"\\".contains(c)
                            })
                        {
                            return Err("Unsupported inline argument form; refusing to guess quoting or nested substitutions".into());
                        }
                        args.push((key.to_owned(), value.to_owned()));
                    }
                    (name, args)
                }
            };
            let key = format!("common/inline_scripts/{}.txt", relative(&name)?).to_lowercase();
            if stack.contains(&key) || stack.len() >= 32 {
                return Err(format!("Inline cycle/depth limit: {key}"));
            }
            let mut body = self
                .files
                .get(&key)
                .ok_or_else(|| format!("Missing inline script {key}"))?
                .raw
                .clone();
            let mut replaced = String::new();
            let mut rest = body.as_str();
            while let Some(start) = rest.find('$') {
                replaced.push_str(&rest[..start]);
                let tail = &rest[start + 1..];
                if let Some(end) = tail.find('$') {
                    if let Some((_, value)) = args.iter().find(|(k, _)| k == &tail[..end]) {
                        replaced.push_str(value);
                    } else {
                        replaced.push_str(&rest[start..start + end + 2]);
                    }
                    rest = &tail[end + 1..];
                } else {
                    replaced.push_str(&rest[start..]);
                    rest = "";
                    break;
                }
                if replaced.len() > MAX_EXPANDED {
                    return Err("Inline parameter expansion exceeds byte budget".into());
                }
            }
            replaced.push_str(rest);
            if replaced.len() > MAX_EXPANDED {
                return Err("Inline parameter expansion exceeds byte budget".into());
            }
            body = replaced;
            self.inline_reads.insert(key.clone());
            let mut stack = stack.to_vec();
            stack.push(key);
            let expanded = self.expand_inner(&body, &stack, calls_used)?;
            if result
                .len()
                .saturating_sub(call.span.len())
                .saturating_add(expanded.len())
                .saturating_add(2)
                > MAX_EXPANDED
            {
                return Err("Expanded script exceeds 64 MiB budget".into());
            }
            // A final inline comment must not swallow caller fields or braces.
            result.replace_range(call.span, &format!("\n{expanded}\n"));
        }
        Ok(result)
    }

    pub fn fingerprint(&self) -> String {
        let rows: Vec<_> = self
            .files
            .iter()
            .map(|(k, v)| json!([k, v.layer, v.hash]))
            .collect();
        sha(json!({"files":rows,"layers":self.layers.iter().map(|l|json!([path_json(&l.root),l.name,l.descriptor_hash,l.archive_hash])).collect::<Vec<_>>()}).to_string().as_bytes())
    }
    pub fn has_resource(&self, path: &str) -> bool {
        let Ok(rel) = relative(path) else {
            return false;
        };
        let key = rel.to_lowercase();
        for layer in self.layers.iter().rev() {
            if layer.archive_entries.as_ref().map_or_else(
                || layer.root.join(&rel).is_file(),
                |entries| entries.contains(&key),
            ) {
                return true;
            }
            if layer
                .metadata
                .all("replace_path")
                .filter_map(Value::atom)
                .filter_map(|s| relative(s).ok())
                .map(|s| s.to_lowercase())
                .any(|p| key == p || key.starts_with(&(p + "/")))
            {
                return false;
            }
        }
        false
    }
    pub fn verify(&self) -> Result<()> {
        let roots: Vec<_> = self.layers.iter().skip(1).map(|l| l.root.clone()).collect();
        let current = Self::load(
            &self.layers[0].root,
            &roots,
            self.languages.clone(),
            self.include_ui,
        )?;
        if current.fingerprint() != self.fingerprint() {
            Err("Sources changed during collection; no successful result".into())
        } else {
            Ok(())
        }
    }
    pub fn localisation(&self, language: &str) -> (BTreeMap<String, String>, Vec<Json>) {
        let mut rows: Vec<_> = self
            .files
            .iter()
            .filter(|(k, _)| k.starts_with("localisation") && k.ends_with(".yml"))
            .collect();
        rows.sort_by_key(|(key, v)| (key.contains("/replace/"), v.layer, (*key).clone()));
        let mut values = BTreeMap::new();
        let mut warnings = Vec::new();
        for (_, file) in rows {
            let expected = format!("l_{language}:");
            if !file.raw.lines().any(|s| s.trim() == expected) {
                continue;
            }
            for (n, line) in file.raw.lines().enumerate() {
                let s = line.trim_start();
                if s.starts_with('#') || s.trim() == expected {
                    continue;
                }
                if let Some((key, rest)) = s.split_once(':') {
                    let rest = rest
                        .trim_start_matches(|c: char| c.is_ascii_digit())
                        .trim_start();
                    if rest.starts_with('"') {
                        // Localisation dollar references and engine escape sequences stay literal.
                        let bytes = rest.as_bytes();
                        let mut end = 1;
                        while end < bytes.len() {
                            if bytes[end] == b'\\' {
                                end += 2;
                            } else if bytes[end] == b'"' {
                                break;
                            } else {
                                end += 1;
                            }
                        }
                        if end < bytes.len() {
                            values.insert(key.into(), rest[1..end].into());
                        } else {
                            warnings.push(json!({"file":file.relative,"line":n+1,"issue":"unclosed localisation string"}));
                        }
                    }
                }
            }
        }
        (values, warnings)
    }
}
