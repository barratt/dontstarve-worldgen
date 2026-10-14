//! The game's script filesystem as seen by `kleiloadlua`.
//!
//! DS resolves paths against `<game>/data` and overlays the active DLC
//! directories file by file (`data/DLC0001/scripts/map/forest_map.lua`
//! shadows `data/scripts/map/forest_map.lua` for Reign of Giants). DST reads
//! `scripts/...` from `data/databundles/scripts.zip`. Paths under
//! `../mods/` resolve to the mods directory.
//!
//! Install files do not change while the generator runs, so each file, each
//! existence check and each zip index is read once per thread and reused by
//! every later world. That keeps seed searches fast, above all in the web
//! build, where every read goes through the browser's File API.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::zipfile::Zip;

/// DST: scripts.zip and the asset names of every data/databundles/*.zip.
struct DstBundles {
    scripts: RefCell<Zip>,
    /// Entry names of every bundle (asset existence).
    names: HashSet<String>,
    /// Decompressed scripts already read.
    files: RefCell<HashMap<String, Rc<[u8]>>>,
}

thread_local! {
    static FILES: RefCell<HashMap<PathBuf, Option<Rc<[u8]>>>> = RefCell::new(HashMap::new());
    static IS_FILE: RefCell<HashMap<PathBuf, bool>> = RefCell::new(HashMap::new());
    static DST: RefCell<HashMap<PathBuf, Rc<DstBundles>>> = RefCell::new(HashMap::new());
    /// When recording: every install file world generation found or read,
    /// as "data/..." (relative to the install) or "scripts.zip/..." (DST).
    static RECORD: RefCell<Option<std::collections::BTreeSet<String>>> = const { RefCell::new(None) };
}

/// Starts recording which install files world generation uses (this thread).
pub fn record_start() {
    RECORD.with(|r| *r.borrow_mut() = Some(Default::default()));
}

/// Stops recording and returns the files used since `record_start`.
pub fn record_take() -> std::collections::BTreeSet<String> {
    RECORD.with(|r| r.borrow_mut().take()).unwrap_or_default()
}

fn record(key: String) {
    RECORD.with(|r| {
        if let Some(set) = r.borrow_mut().as_mut() {
            set.insert(key);
        }
    });
}

/// Reads an install file (cached).
pub fn read_file(path: &Path) -> Result<Rc<[u8]>, String> {
    let cached = FILES.with(|f| f.borrow().get(path).cloned());
    let bytes = match cached {
        Some(b) => b,
        None => {
            let b = std::fs::read(path).ok().map(Rc::from);
            FILES.with(|f| f.borrow_mut().insert(path.to_path_buf(), b.clone()));
            b
        }
    };
    bytes.ok_or_else(|| format!("{}: cannot read file", path.display()))
}

fn is_file(path: &Path) -> bool {
    if let Some(v) = IS_FILE.with(|c| c.borrow().get(path).copied()) {
        return v;
    }
    let v = path.is_file();
    IS_FILE.with(|c| c.borrow_mut().insert(path.to_path_buf(), v));
    v
}

fn dst_bundles(data_root: &Path) -> Result<Rc<DstBundles>, String> {
    if let Some(b) = DST.with(|d| d.borrow().get(data_root).cloned()) {
        return Ok(b);
    }
    let bundles = data_root.join("databundles");
    let scripts = Zip::open(&bundles.join("scripts.zip"))?;
    let mut names = HashSet::new();
    if let Ok(dir) = std::fs::read_dir(&bundles) {
        for entry in dir.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("zip") {
                continue;
            }
            if let Ok(z) = Zip::open(&p) {
                names.extend(z.names().map(String::from));
            }
        }
    }
    let b = Rc::new(DstBundles { scripts: RefCell::new(scripts), names, files: RefCell::new(HashMap::new()) });
    DST.with(|d| d.borrow_mut().insert(data_root.to_path_buf(), b.clone()));
    Ok(b)
}

pub struct Vfs {
    data_root: PathBuf,
    overlays: Vec<String>,
    mods_dir: Option<PathBuf>,
    dst: Option<Rc<DstBundles>>,
}

pub struct Resolved {
    pub path: PathBuf,
    pub chunkname: String,
}

impl Vfs {
    pub fn new(data_root: &Path, overlays: Vec<String>, mods_dir: Option<PathBuf>) -> Vfs {
        Vfs { data_root: data_root.to_path_buf(), overlays, mods_dir, dst: None }
    }

    /// DST layout: scripts come from `<data>/databundles/scripts.zip`.
    pub fn with_scripts_zip(data_root: &Path, mods_dir: Option<PathBuf>) -> Result<Vfs, String> {
        let dst = dst_bundles(data_root)?;
        Ok(Vfs { data_root: data_root.to_path_buf(), overlays: vec![], mods_dir, dst: Some(dst) })
    }

    fn normalize(path: &str) -> String {
        let mut p = path.replace('\\', "/");
        while let Some(rest) = p.strip_prefix("./") {
            p = rest.to_string();
        }
        p
    }

    /// File on disk (DS layout, or mods).
    pub fn resolve(&self, path: &str) -> Option<Resolved> {
        let p = Self::normalize(path);
        if let Some(rest) = p.strip_prefix("../mods/") {
            let full = self.mods_dir.as_ref()?.join(rest);
            return is_file(&full).then(|| Resolved { chunkname: format!("@{p}"), path: full });
        }
        if p.starts_with("../") || p.contains(':') || p.starts_with('/') {
            return None;
        }
        let (rel, chunk_prefix) = match p.strip_prefix("data/") {
            Some(rest) => {
                // Chunk names as the game prints them: <game>/data/scripts/...
                let game = self.data_root.parent().unwrap_or(&self.data_root);
                (rest.to_string(), format!("{}/", game.display().to_string().replace('\\', "/")))
            }
            None => (p.clone(), String::new()),
        };
        let found = |full: &Path| {
            if let Ok(rest) = full.strip_prefix(self.data_root.parent().unwrap_or(&self.data_root)) {
                record(rest.to_string_lossy().replace('\\', "/"));
            }
        };
        for overlay in &self.overlays {
            let full = self.data_root.join(overlay).join(&rel);
            if is_file(&full) {
                found(&full);
                return Some(Resolved { path: full, chunkname: format!("@{chunk_prefix}{p}") });
            }
        }
        let full = self.data_root.join(&rel);
        if !is_file(&full) {
            return None;
        }
        found(&full);
        Some(Resolved { path: full, chunkname: format!("@{chunk_prefix}{p}") })
    }

    /// Contents and chunk name of a script, from the zip (DST) or disk.
    pub fn read(&self, path: &str) -> Option<(Vec<u8>, String)> {
        if let Some(dst) = &self.dst {
            let p = Self::normalize(path);
            if dst.scripts.borrow().contains(&p) {
                record(format!("scripts.zip/{p}"));
                if let Some(bytes) = dst.files.borrow().get(&p) {
                    return Some((bytes.to_vec(), format!("@{p}")));
                }
                let buf = dst.scripts.borrow_mut().read(&p)?;
                dst.files.borrow_mut().insert(p.clone(), Rc::from(buf.as_slice()));
                return Some((buf, format!("@{p}")));
            }
            if !p.starts_with("../mods/") && !p.starts_with("scriptlibs/") {
                return None;
            }
        }
        let r = self.resolve(path)?;
        let bytes = read_file(&r.path).ok()?;
        Some((bytes.to_vec(), r.chunkname))
    }

    pub fn exists(&self, path: &str) -> bool {
        let p = Self::normalize(path);
        if let Some(dst) = &self.dst {
            if dst.scripts.borrow().contains(&p) {
                record(format!("scripts.zip/{p}"));
                return true;
            }
            if dst.names.contains(&p) {
                return true;
            }
        }
        self.resolve(path).is_some()
    }
}
