//! Never open the user's SQLite database with SQLite itself. WAL readers can
//! otherwise create/change shared-memory sidecars even with READ_ONLY flags.
use crate::{script::Result, sources::sha};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub type InputHashes = Vec<(PathBuf, Option<String>)>;
static NEXT: AtomicUsize = AtomicUsize::new(0);

pub fn bytes(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut raw = Vec::new();
    file.take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() > 128 * 1024 * 1024 {
        return Err("Launcher input exceeds 128 MiB snapshot budget".into());
    }
    Ok(raw)
}

pub fn track(path: &Path, hashes: &mut InputHashes) -> Result<()> {
    let hash = if path.exists() {
        Some(crate::archive::hash(path)?)
    } else {
        None
    };
    hashes.push((path.to_owned(), hash));
    Ok(())
}

pub fn verify(hashes: &InputHashes) -> Result<()> {
    for (path, expected) in hashes {
        let now = if path.exists() {
            Some(crate::archive::hash(path)?)
        } else {
            None
        };
        if &now != expected {
            return Err(
                "Launcher selection changed during collection; rerun after closing the launcher"
                    .into(),
            );
        }
    }
    Ok(())
}

pub struct Snapshot {
    root: PathBuf,
    parent: PathBuf,
    pub database: PathBuf,
    pub hashes: InputHashes,
}
impl Snapshot {
    pub fn new(original: &Path) -> Result<Self> {
        let parent = fs::canonicalize(std::env::temp_dir()).map_err(|e| e.to_string())?;
        let clock = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let root = parent.join(format!(
            "tte-core-db-{}-{clock}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        #[allow(unused_mut)]
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&root).map_err(|e| e.to_string())?;
        let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
        let database = root.join("launcher.sqlite");
        let mut result = Self {
            root,
            parent,
            database,
            hashes: Vec::new(),
        };
        let mut journal = original.as_os_str().to_os_string();
        journal.push("-journal");
        let journal = PathBuf::from(journal);
        track(&journal, &mut result.hashes)?;
        if journal.exists() && !bytes(&journal)?.is_empty() {
            return Err(
                "Launcher database has an active rollback journal; close launcher and retry".into(),
            );
        }
        for suffix in ["", "-wal"] {
            let mut path = original.as_os_str().to_os_string();
            path.push(suffix);
            let path = PathBuf::from(path);
            if path.exists() {
                let raw = bytes(&path)?;
                result.hashes.push((path, Some(sha(&raw))));
                let target = result.root.join(format!("launcher.sqlite{suffix}"));
                let mut file = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(target)
                    .map_err(|e| e.to_string())?;
                file.write_all(&raw).map_err(|e| e.to_string())?;
            } else {
                result.hashes.push((path, None));
            }
        }
        verify(&result.hashes)?;
        Ok(result)
    }
}
impl Drop for Snapshot {
    fn drop(&mut self) {
        if self.root.parent() == Some(self.parent.as_path())
            && self
                .root
                .file_name()
                .is_some_and(|s| s.to_string_lossy().starts_with("tte-core-db-"))
            && fs::canonicalize(&self.root).ok().as_ref() == Some(&self.root)
        {
            // Only this process's verified, private temporary copy; never inputs.
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
