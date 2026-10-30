//! Read-only virtual ZIP source. Never extracts files, executes source scripts,
//! or shells out to an archive utility. Only selected text entries are inflated.
use crate::{script::Result, sources};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufReader, Read},
    path::Path,
};
const MAX_ARCHIVE: u64 = 8 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 250000;
const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_SELECTED: u64 = 512 * 1024 * 1024;
pub struct Archive {
    pub hash: String,
    pub entries: BTreeSet<String>,
    pub texts: BTreeMap<String, (String, Vec<u8>)>,
}
pub(crate) fn hash(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_ARCHIVE {
        return Err("ZIP exceeds8GiB source budget".into());
    }
    let mut sha = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut read = 0u64;
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        read += n as u64;
        if read > MAX_ARCHIVE {
            return Err("ZIP grew beyond8GiB source budget".into());
        }
        sha.update(&buffer[..n]);
    }
    Ok(format!("{:x}", sha.finalize()))
}
impl Archive {
    pub fn open(path: &Path, want: impl Fn(&str) -> bool) -> Result<Self> {
        let hash_before = hash(path)?;
        let file = File::open(path).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(BufReader::new(file))
            .map_err(|e| format!("Invalid source ZIP: {e}"))?;
        if zip.len() > MAX_ENTRIES {
            return Err("ZIP entry count exceeds250000".into());
        }
        if zip.has_duplicate_names() {
            return Err("Duplicate ZIP source entry names".into());
        }
        let mut result = Self {
            hash: hash_before.clone(),
            entries: BTreeSet::new(),
            texts: BTreeMap::new(),
        };
        let mut selected = 0u64;
        let mut indexes = Vec::new();
        for i in 0..zip.len() {
            let entry = zip
                .by_index_raw(i)
                .map_err(|e| format!("ZIP entry {i}: {e}"))?;
            let name = entry.name().map_err(|e| e.to_string())?.into_owned();
            if name.len() > 4096 {
                return Err("ZIP source entry path exceeds4096bytes".into());
            }
            let rel = sources::relative(&name)?;
            if entry.is_symlink() {
                return Err(format!("ZIP source symlink unsupported: {rel}"));
            }
            if entry.encrypted() {
                return Err(format!("Encrypted ZIP source unsupported: {rel}"));
            }
            if !matches!(
                entry.compression(),
                zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
            ) {
                return Err(format!("Unsupported ZIP source compression: {rel}"));
            }
            if entry.is_dir() {
                continue;
            }
            let key = rel.to_lowercase();
            if !result.entries.insert(key.clone()) {
                return Err(format!("Case-colliding ZIP source entry: {rel}"));
            }
            if key == "descriptor.mod" || want(&key) {
                if entry.size() > MAX_FILE {
                    return Err(format!("ZIP source text exceeds64MiB: {rel}"));
                }
                selected = selected
                    .checked_add(entry.size())
                    .ok_or("ZIP size overflow")?;
                if selected > MAX_SELECTED {
                    return Err("ZIP selected text exceeds512MiB".into());
                }
                indexes.push((i, key, rel, entry.size()));
            }
        }
        if !result.entries.contains("descriptor.mod") {
            return Err("ZIP source requires descriptor.mod at archive root".into());
        }
        for (i, key, rel, size) in indexes {
            let entry = zip.by_index(i).map_err(|e| format!("ZIP {rel}: {e}"))?;
            let mut bytes = Vec::new();
            entry
                .take(MAX_FILE + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| format!("ZIP {rel}: {e}"))?;
            if bytes.len() as u64 != size || bytes.len() as u64 > MAX_FILE {
                return Err(format!("ZIP source entry size mismatch: {rel}"));
            }
            result.texts.insert(key, (rel, bytes));
        }
        if hash(path)? != hash_before {
            return Err("ZIP changed while reading; no successful result".into());
        }
        Ok(result)
    }
    pub fn descriptor(&self) -> Result<String> {
        let (_, raw) = self
            .texts
            .get("descriptor.mod")
            .ok_or("ZIP descriptor missing")?;
        Ok(crate::script::normalise(
            std::str::from_utf8(raw).map_err(|_| "ZIP descriptor is not UTF-8")?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn make(rows: &[(&str, &[u8])]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tte-zip-probe-{}-{}.zip",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, data) in rows {
            zip.start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
        path
    }
    #[test]
    fn inflates_only_selected_text_and_keeps_an_asset_index() {
        let path = make(&[
            ("descriptor.mod", b"name=Owned"),
            ("common/technology/a.txt", b"a={area=physics}"),
            ("gfx/owned.dds", b"untrusted-not-decoded"),
        ]);
        let zip = Archive::open(&path, |s| s.starts_with("common/")).unwrap();
        assert_eq!(zip.texts.len(), 2);
        assert!(zip.entries.contains("gfx/owned.dds"));
        assert!(!zip.texts.contains_key("gfx/owned.dds"));
        assert_eq!(zip.descriptor().unwrap(), "name=Owned");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn rejects_traversal_case_collisions_and_wrapper_roots() {
        for rows in [
            vec![
                ("descriptor.mod", b"name=Owned".as_slice()),
                ("../escape.txt", b"x".as_slice()),
            ],
            vec![
                ("descriptor.mod", b"name=Owned".as_slice()),
                ("A.txt", b"a".as_slice()),
                ("a.txt", b"b".as_slice()),
            ],
            vec![("wrapper/descriptor.mod", b"name=Owned".as_slice())],
        ] {
            let path = make(&rows);
            assert!(Archive::open(&path, |_| true).is_err());
            std::fs::remove_file(path).unwrap();
        }
    }
}
