//! Publish a complete report without ever overwriting an existing destination.
//! Windows uses MoveFileExW without replacement, Unix uses
//! a same-directory hard link. Unsupported filesystems fail, never clobber.
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
static NEXT_PARTIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct Partial(PathBuf);
impl Drop for Partial {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(windows)]
fn publish_path(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let mut source: Vec<u16> = source.as_os_str().encode_wide().collect();
    let mut destination: Vec<u16> = destination.as_os_str().encode_wide().collect();
    if source.contains(&0) || destination.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in report path",
        ));
    }
    source.push(0);
    destination.push(0);
    // WRITE_THROUGH only; absence of REPLACE_EXISTING still refuses clobber.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 8) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn publish_path(source: &Path, destination: &Path) -> io::Result<()> {
    fs::hard_link(source, destination)
}

pub(crate) fn publish_with(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let clock = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let pending_path = parent.join(format!(
        ".tte-report-{}-{clock}-{}.partial",
        std::process::id(),
        NEXT_PARTIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    // Do not arm cleanup before create_new succeeds: on a collision that
    // would remove another writer's partial file. Windows clocks can be coarse.
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending_path)?;
    let pending = Partial(pending_path);
    let result = write(&mut file).and_then(|()| file.sync_all());
    drop(file);
    result?;
    publish_path(&pending.0,path).map_err(|e|io::Error::new(e.kind(),format!("Cannot atomically publish the report without overwriting: {e}. Use a fresh filename on a filesystem supporting atomic rename/link publication.")))?;
    Ok(())
}
pub fn publish(path: &Path, raw: &[u8]) -> io::Result<()> {
    publish_with(path, |file| file.write_all(raw))
}
/// Check the actual destination filesystem before generating a large candidate.
/// Only fresh owned probe files are created and removed; no fallback overwrites.
pub fn check_parent(parent: &Path) -> io::Result<()> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let probe = parent.join(format!(
        ".tte-publication-check-{}-{stamp}-{}",
        std::process::id(),
        NEXT_PARTIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    publish(&probe, b"")?;
    fs::remove_file(probe)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_probe_leaves_no_files_or_modified_neighbors() {
        let dir = folder();
        fs::write(dir.join("keep"), b"unchanged").unwrap();
        check_parent(&dir).unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        assert_eq!(fs::read(dir.join("keep")).unwrap(), b"unchanged");
        cleanup(&dir);
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    fn folder() -> PathBuf {
        let clock = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tte-report-test-{}-{clock}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }
    fn cleanup(path: &Path) {
        let resolved = fs::canonicalize(path).unwrap();
        assert_eq!(
            resolved.parent(),
            Some(fs::canonicalize(std::env::temp_dir()).unwrap().as_path())
        );
        assert!(resolved
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("tte-report-test-"));
        fs::remove_dir_all(resolved).unwrap();
    }
    #[test]
    fn failed_write_never_leaves_final_or_partial_result() {
        let dir = folder();
        let final_path = dir.join("result.json");
        let done = publish_with(&final_path, |file| {
            file.write_all(b"truncated")?;
            Err(io::Error::other("authored injected write failure"))
        });
        assert!(done.is_err());
        assert!(!final_path.exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        cleanup(&dir);
    }
    #[test]
    fn concurrent_destination_is_never_replaced() {
        let dir = folder();
        let path = dir.join("result.json");
        publish(&path, b"first complete").unwrap();
        assert!(publish(&path, b"second").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"first complete");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        cleanup(&dir);
    }
    #[test]
    fn parallel_complete_reports_have_independent_staging_files() {
        let dir = folder();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
        let workers = (0..16)
            .map(|i| {
                let b = barrier.clone();
                let p = dir.join(format!("{i}.json"));
                std::thread::spawn(move || {
                    b.wait();
                    publish(&p, &vec![i as u8; 4096])
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        for i in 0..16 {
            assert_eq!(
                fs::read(dir.join(format!("{i}.json"))).unwrap(),
                vec![i as u8; 4096]
            );
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 16);
        cleanup(&dir);
    }
}
