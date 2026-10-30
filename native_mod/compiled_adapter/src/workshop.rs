//! Explicit opt-in personalization of THIS Mod's fixed runtime ZIP only.
//! No launcher registration, source-Mod edits, DLLs or process control.
use crate::{
    script::{self, Result},
    sources,
};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MARKER: &str = "technology-adapter/WORKSHOP-PACKAGE.json";
const STATE: &str = "tte-adapter-state.json";
pub struct Target {
    pub root: PathBuf,
    archive: PathBuf,
    hash: String,
    base_hash: String,
    marker_hash: String,
    descriptor: Vec<u8>,
    picture: Option<Vec<u8>>,
}

fn text_member(zip: &mut ZipArchive<fs::File>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let mut file = zip
        .by_name(name)
        .map_err(|e| format!("Workshop ZIP {name}: {e}"))?;
    if file.size() > limit {
        return Err(format!("Workshop ZIP member too large: {name}"));
    }
    let mut raw = Vec::new();
    file.by_ref()
        .take(limit + 1)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() as u64 > limit {
        return Err("ZIP member expanded beyond its budget".into());
    }
    Ok(raw)
}

#[cfg(windows)]
fn process_names() -> Result<Vec<String>> {
    #[repr(C)]
    struct Entry {
        size: u32,
        usage: u32,
        pid: u32,
        heap: usize,
        module: u32,
        threads: u32,
        parent: u32,
        priority: i32,
        flags: u32,
        exe: [u16; 260],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
        fn Process32FirstW(snapshot: isize, entry: *mut Entry) -> i32;
        fn Process32NextW(snapshot: isize, entry: *mut Entry) -> i32;
        fn CloseHandle(handle: isize) -> i32;
        fn GetLastError() -> u32;
    }
    struct Snapshot(isize);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    // Names only, without opening another process or reading its memory.
    let handle = unsafe { CreateToolhelp32Snapshot(2, 0) };
    if handle == -1 {
        return Err(format!(
            "Cannot check game processes: {}",
            std::io::Error::last_os_error()
        ));
    }
    let snapshot = Snapshot(handle);
    let mut entry: Entry = unsafe { std::mem::zeroed() };
    entry.size = std::mem::size_of::<Entry>() as u32;
    let mut names = Vec::new();
    let mut found = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    loop {
        if found == 0 {
            if unsafe { GetLastError() } != 18 {
                return Err("Cannot enumerate process names; nothing replaced".into());
            }
            break;
        }
        let n = entry
            .exe
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.exe.len());
        names.push(String::from_utf16_lossy(&entry.exe[..n]));
        found = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    Ok(names)
}
#[cfg(target_os = "linux")]
fn process_names() -> Result<Vec<String>> {
    let mut names = Vec::new();
    for row in
        fs::read_dir("/proc").map_err(|e| format!("Cannot inspect /proc process names: {e}"))?
    {
        let row = row.map_err(|e| e.to_string())?;
        if !row
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        match fs::read_to_string(row.path().join("comm")) {
            Ok(name) => names.push(name.trim().to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("Cannot inspect process name: {e}")),
        }
    }
    Ok(names)
}
#[cfg(all(not(windows), not(target_os = "linux")))]
fn process_names() -> Result<Vec<String>> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-A", "-o", "comm="])
        .output()
        .map_err(|e| format!("Cannot check game/launcher processes: {e}"))?;
    if !output.status.success() {
        return Err("Cannot check game/launcher processes; nothing replaced".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            Path::new(line.trim())
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .collect())
}
fn ensure_closed() -> Result<()> {
    let running = process_names()?.into_iter().any(|name| {
        let name = name.to_ascii_lowercase();
        matches!(
            name.as_str(),
            "stellaris"
                | "stellaris.exe"
                | "paradox launcher"
                | "paradox launcher.exe"
                | "paradoxlauncher"
                | "paradox launche"
                | "paradox-launche"
                | "dowser.exe"
        )
    });
    if running {
        Err("Close Stellaris and its launcher before adapting this Mod. / 请先退出群星并关闭启动器，再运行适配工具。".into())
    } else {
        Ok(())
    }
}

struct Pending(PathBuf);
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(windows)]
fn replace(source: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let mut from: Vec<u16> = source.as_os_str().encode_wide().collect();
    let mut to: Vec<u16> = target.as_os_str().encode_wide().collect();
    if from.contains(&0) || to.contains(&0) {
        return Err("NUL in Workshop path".into());
    }
    from.push(0);
    to.push(0);
    // Both paths share a verified parent. Replace only the fixed owned ZIP.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 {
        return Err(format!(
            "Cannot replace owned runtime ZIP: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
#[cfg(not(windows))]
fn replace(source: &Path, target: &Path) -> Result<()> {
    fs::rename(source, target).map_err(|e| format!("Cannot replace owned runtime ZIP: {e}"))
}

impl Target {
    pub fn from_exe() -> Result<Self> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // Binary must be directly in technology-adapter/<platform>/.
        let root = exe
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .ok_or("Run the adapter included in the Workshop Mod directory")?;
        Self::open(root)
    }
    fn open(root: &Path) -> Result<Self> {
        ensure_closed()?;
        let root = sources::canonical(root)?;
        let marker_path = root.join(MARKER);
        if fs::metadata(&marker_path)
            .map_err(|_| "Workshop package marker missing")?
            .len()
            > 1024 * 1024
        {
            return Err("Workshop marker exceeds size budget".into());
        }
        let raw = fs::read(&marker_path).map_err(|_| {
            "Run INSTALL-TECH-ADAPTER from the Workshop Mod directory; package marker missing"
        })?;
        if raw.len() > 1024 * 1024
            || fs::symlink_metadata(&marker_path)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("Unsafe Workshop marker".into());
        }
        let marker: Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
        if marker["kind"] != "tte-workshop-adapter"
            || marker["itemId"] != sources::OWN_ID
            || marker["archive"] != "tte_runtime.zip"
        {
            return Err("This directory is not the supported planner Workshop package".into());
        }
        let base_hash = marker["archiveSha256"]
            .as_str()
            .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
            .ok_or("Missing base archive hash")?
            .to_owned();
        let archive = root.join("tte_runtime.zip");
        if fs::symlink_metadata(&archive)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
            || sources::canonical(&archive)?.parent() != Some(root.as_path())
        {
            return Err("Owned runtime ZIP must be a direct regular file".into());
        }
        let hash = crate::archive::hash(&archive)?;
        let mut zip = ZipArchive::new(fs::File::open(&archive).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let descriptor = text_member(&mut zip, "descriptor.mod", 1024 * 1024)?;
        let meta = script::parse(
            std::str::from_utf8(&descriptor)
                .map_err(|e| e.to_string())?
                .trim_start_matches('\u{feff}'),
        )?;
        if meta.all("remote_file_id").count() != 1
            || meta.scalar("remote_file_id") != Some(sources::OWN_ID)
            || meta.first("path").is_some()
            || meta.first("archive").is_some()
        {
            return Err("Runtime ZIP does not belong to this Workshop planner".into());
        }
        if hash != base_hash {
            let state: Value = serde_json::from_slice(&text_member(&mut zip, STATE, 1024 * 1024)
                .map_err(|_|"Runtime changed or Steam update is incomplete; allow Steam to finish before rerunning")?)
                .map_err(|e| e.to_string())?;
            if state["kind"] != "tte-personalized-workshop"
                || state["baseArchiveSha256"] != base_hash
                || state["itemId"] != sources::OWN_ID
            {
                return Err("Runtime differs from this Workshop package; allow Steam update to finish first".into());
            }
        }
        let picture = if zip
            .file_names()
            .any(|n| n.is_ok_and(|n| n == "thumbnail.png"))
        {
            Some(text_member(&mut zip, "thumbnail.png", 4 * 1024 * 1024)?)
        } else {
            None
        };
        Ok(Self {
            root,
            archive,
            hash,
            base_hash,
            marker_hash: sources::sha(&raw),
            descriptor,
            picture,
        })
    }
    fn verify(&self) -> Result<()> {
        if crate::archive::hash(&self.archive)? != self.hash
            || sources::sha(&fs::read(self.root.join(MARKER)).map_err(|e| e.to_string())?)
                != self.marker_hash
        {
            return Err(
                "Workshop files changed during generation; rerun after Steam finishes updating"
                    .into(),
            );
        }
        if fs::symlink_metadata(&self.archive)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("Runtime path became a link; nothing replaced".into());
        }
        Ok(())
    }
    pub fn install(
        &self,
        runtime: &Path,
        verify_inputs: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let lock_path = self.root.join(".tte-adapter.lock");
        if lock_path.exists()
            && fs::symlink_metadata(&lock_path)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("Adapter lock path is a link; nothing replaced".into());
        }
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|e| e.to_string())?;
        lock.try_lock().map_err(|e| {
            format!("Another adapter is updating this Mod, or locking is unavailable: {e}")
        })?;
        // The zero-byte lock file is intentionally retained. The OS releases
        // its lock on exit/crash; deleting its path would create an inode race.
        ensure_closed()?;
        self.verify()?;
        let runtime = sources::canonical(runtime)?;
        if runtime.starts_with(&self.root) {
            return Err("Working candidate must be outside the Workshop folder".into());
        }
        let proof: Value = serde_json::from_slice(
            &fs::read(runtime.join("tte-build.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if proof["identity"] != "tte-native-generated"
            || proof["buildStatus"] != "GENERATION COMPLETE; NATIVE QA PENDING"
            || runtime.join("tte-generating.json").exists()
        {
            return Err("Only a fully generated native planner can be installed".into());
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let pending_path = self.root.join(format!(
            ".tte-personalize-{}-{stamp}.tmp",
            std::process::id()
        ));
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(&pending_path)
            .map_err(|e| e.to_string())?;
        let pending = Pending(pending_path);
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(6));
        let inventory = proof["inventory"]
            .as_object()
            .ok_or("Missing generated inventory")?;
        for (name, row) in inventory {
            let name = sources::relative(name)?;
            let path = runtime.join(&name);
            if !sources::canonical(&path)?.starts_with(&runtime) {
                return Err("Generated file escaped runtime root".into());
            }
            if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024 {
                return Err("Generated file size budget exceeded".into());
            }
            let bytes = fs::read(&path).map_err(|e| e.to_string())?;
            if row["sha256"] != sources::sha(&bytes) || row["bytes"] != bytes.len() {
                return Err(format!("Generated candidate changed: {name}"));
            }
            if matches!(
                name.as_str(),
                "descriptor.mod" | "README.txt" | "tte-adapter-state.json"
            ) || name == "thumbnail.png" && self.picture.is_some()
            {
                continue;
            }
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            zip.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        for (name, bytes) in [
            ("descriptor.mod", Some(&self.descriptor)),
            ("thumbnail.png", self.picture.as_ref()),
        ] {
            if let Some(bytes) = bytes {
                zip.start_file(name, options).map_err(|e| e.to_string())?;
                zip.write_all(bytes).map_err(|e| e.to_string())?;
            }
        }
        let fingerprint = sources::sha(
            &serde_json::to_vec(&proof["sourceFieldProof"]).map_err(|e| e.to_string())?,
        );
        let state=serde_json::to_vec(&json!({"kind":"tte-personalized-workshop","itemId":sources::OWN_ID,"baseArchiveSha256":self.base_hash,"packageMarkerSha256":self.marker_hash,"sourceFingerprint":fingerprint,"technologies":proof["technologies"],"noNewLauncherRegistration":true})).map_err(|e|e.to_string())?;
        zip.start_file(STATE, options).map_err(|e| e.to_string())?;
        zip.write_all(&state).map_err(|e| e.to_string())?;
        zip.finish()
            .map_err(|e| e.to_string())?
            .sync_all()
            .map_err(|e| e.to_string())?;
        // A content-addressed backup is outside Steam. Existing verified backups
        // are reused, never overwritten. An interrupted backup stays unusable.
        let backups = runtime
            .parent()
            .ok_or("Working parent missing")?
            .join("Workshop-backups");
        fs::create_dir_all(&backups).map_err(|e| e.to_string())?;
        let mut backup = backups.join(format!("{}.zip", self.hash));
        if backup.exists() && crate::archive::hash(&backup)? != self.hash {
            backup = backups.join(format!("{}-{stamp}.zip", self.hash));
        }
        if !backup.exists() {
            crate::atomic_report::publish_with(&backup, |target| {
                let mut source = fs::File::open(&self.archive)?;
                std::io::copy(&mut source, target).map(|_| ())
            })
            .map_err(|e| e.to_string())?;
        }
        if crate::archive::hash(&backup)? != self.hash {
            return Err(format!(
                "Backup did not verify; original runtime kept. Inspect {}",
                sources::path_json(&backup)
            ));
        }
        #[cfg(unix)]
        fs::File::open(&backups)
            .and_then(|f| f.sync_all())
            .map_err(|e| format!("Cannot sync backup directory: {e}"))?;
        ensure_closed()?;
        self.verify()?;
        verify_inputs()?;
        replace(&pending.0, &self.archive)?;
        #[cfg(unix)]
        if let Err(e) = fs::File::open(&self.root).and_then(|f| f.sync_all()) {
            eprintln!("Runtime updated, but directory durability could not be confirmed: {e}");
        }
        println!(
            "Updated the same Workshop Mod / 已更新当前工坊 Mod：{}\nBackup / 备份：{}",
            sources::path_json(&self.archive),
            sources::path_json(&backup)
        );
        Ok(())
    }
}
