//! Game data bundles for standalone builds.
//!
//! `ds-worldgen pack` collects, from the user's own installs, exactly what
//! world generation reads: the Lua scripts, plus the names (not contents)
//! of assets whose existence the scripts check. For the web page it adds
//! the games' minimap icons and display names. It stores them in one
//! compact file of independently compressed sections:
//!
//! ```text
//! "DSWB" u32 version, u32 section count, then per section:
//!   u16 name length, name, u32 entry count,
//!   entries: u16 path length, path, u8 kind (0 file, 1 name only), u32 size,
//!   u32 compressed length, one deflate stream of the contents in entry order.
//! ```
//!
//! Sections: `ds` and `dst` (scripts), `icons-ds`, `icons-dst`,
//! `meta-ds`, `meta-dst` (display names). A standalone web build embeds
//! the bundle (build.rs, `DSW_EMBED`); each web worker inflates only the
//! game it generates and lays it out in its in-memory filesystem with
//! [`unpack`], like an install, so the generator runs unchanged. Bundles are built locally
//! from the user's copy of the games and are not meant to be redistributed.

use std::collections::BTreeMap;
use std::path::Path;

const MAGIC: &[u8; 4] = b"DSWB";
const VERSION: u32 = 2;

/// The section an entry path belongs to.
fn section_of(path: &str) -> String {
    let mut parts = path.splitn(3, '/');
    match (parts.next(), parts.next()) {
        (Some(kind @ ("icons" | "meta")), Some(game)) => format!("{kind}-{game}"),
        (Some(first), _) => first.to_string(),
        _ => String::new(),
    }
}

#[derive(Default)]
pub struct Bundle {
    /// Path (e.g. "ds/data/scripts/main.lua") -> contents, or None for an
    /// entry whose existence is all that matters.
    pub entries: BTreeMap<String, Option<Vec<u8>>>,
}

fn walk_lua(dir: &Path, rel: &str, out: &mut BTreeMap<String, Option<Vec<u8>>>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_dir() {
            walk_lua(&path, &format!("{rel}/{name}"), out)?;
        } else if name.ends_with(".lua") {
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            out.insert(format!("{rel}/{name}"), Some(bytes));
        }
    }
    Ok(())
}

fn walk_names(dir: &Path, rel: &str, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_dir() {
            walk_names(&path, &format!("{rel}/{name}"), out);
        } else {
            out.insert(format!("{rel}/{name}"), None);
        }
    }
}

fn bad() -> String {
    "not a dsworldgen data bundle (rebuild it with ds-worldgen pack)".to_string()
}

/// One section of an encoded bundle, its index read but not inflated.
pub struct Section<'a> {
    pub name: String,
    /// (path, name only, size)
    pub index: Vec<(String, bool, usize)>,
    data: &'a [u8],
}

/// Iterates the sections of an encoded bundle without inflating them.
pub struct Sections<'a> {
    bytes: &'a [u8],
    at: usize,
    left: u32,
}

impl<'a> Sections<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Sections<'a>, String> {
        if bytes.len() < 12 || &bytes[..4] != MAGIC {
            return Err(bad());
        }
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != VERSION {
            return Err(format!("data bundle version {version} is not supported (rebuild it with ds-worldgen pack)"));
        }
        Ok(Sections { bytes, at: 12, left: u32::from_le_bytes(bytes[8..12].try_into().unwrap()) })
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.bytes.get(self.at..self.at + n).ok_or_else(bad)?;
        self.at += n;
        Ok(s)
    }

    fn read_section(&mut self) -> Result<Section<'a>, String> {
        let u16_of = |b: &[u8]| u16::from_le_bytes(b.try_into().unwrap()) as usize;
        let u32_of = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap()) as usize;
        let len = u16_of(self.take(2)?);
        let name = String::from_utf8_lossy(self.take(len)?).into_owned();
        let count = u32_of(self.take(4)?);
        let mut index = Vec::with_capacity(count);
        for _ in 0..count {
            let len = u16_of(self.take(2)?);
            let path = String::from_utf8_lossy(self.take(len)?).into_owned();
            let name_only = self.take(1)?[0] == 1;
            let size = u32_of(self.take(4)?);
            index.push((path, name_only, size));
        }
        let packed = u32_of(self.take(4)?);
        let data = self.take(packed)?;
        Ok(Section { name, index, data })
    }
}

impl<'a> Iterator for Sections<'a> {
    type Item = Result<Section<'a>, String>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        Some(self.read_section())
    }
}

/// `STRINGS.NAMES` of a strings.lua: prefab -> display name.
fn display_names(src: &str, out: &mut BTreeMap<String, String>) {
    // The line holding the NAMES key itself (not CHARACTER_NAMES and the like).
    let mut offset = 0;
    let mut start = None;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("NAMES =") || t.starts_with("NAMES=") {
            start = Some(offset);
            break;
        }
        offset += line.len();
    }
    let Some(start) = start else { return };
    let mut depth = 0i32;
    for line in src[start..].lines() {
        let opens = line.matches('{').count() as i32;
        let closes = line.matches('}').count() as i32;
        if depth == 1 {
            let t = line.trim();
            if let Some((key, rest)) = t.split_once('=') {
                let key = key.trim();
                let value = rest.trim().trim_end_matches(',').trim();
                if !key.is_empty()
                    && key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                    && value.len() >= 2
                    && value.starts_with('"')
                    && value.ends_with('"')
                {
                    out.insert(key.to_ascii_lowercase(), value[1..value.len() - 1].to_string());
                }
            }
        }
        depth += opens - closes;
        if depth <= 0 && opens + closes > 0 {
            break;
        }
    }
}

impl Bundle {
    /// Display names from the strings.lua files in the bundle whose path
    /// matches, stored as `meta/<game>/names.json`.
    fn add_names(&mut self, game: &str, matches: impl Fn(&str) -> bool) {
        let mut names = BTreeMap::new();
        let sources: Vec<String> = self
            .entries
            .iter()
            .filter(|(k, v)| matches(k) && v.is_some())
            .map(|(_, v)| String::from_utf8_lossy(v.as_ref().unwrap()).into_owned())
            .collect();
        for src in sources {
            display_names(&src, &mut names);
        }
        let json = serde_json::to_vec(&names).unwrap_or_default();
        self.entries.insert(format!("meta/{game}/names.json"), Some(json));
    }

    /// Don't Starve: every Lua script of the base game and each installed DLC.
    pub fn add_ds(&mut self, game: &Path) -> Result<(), String> {
        let data = game.join("data");
        if !data.join("scripts/worldgen_main.lua").is_file() {
            return Err(format!("{}: not a Don't Starve install (no data/scripts/worldgen_main.lua)", game.display()));
        }
        walk_lua(&data.join("scripts"), "ds/data/scripts", &mut self.entries)?;
        walk_lua(&data.join("scriptlibs"), "ds/data/scriptlibs", &mut self.entries)?;
        for dlc in ["DLC0001", "DLC0002", "DLC0003"] {
            let dir = data.join(dlc).join("scripts");
            if dir.is_dir() {
                walk_lua(&dir, &format!("ds/data/{dlc}/scripts"), &mut self.entries)?;
            }
        }
        self.add_names("ds", |k| k.starts_with("ds/data/") && k.ends_with("/scripts/strings.lua"));
        self.add_ds_icons(game)
    }

    /// Don't Starve Together: the Lua scripts in scripts.zip, plus the names
    /// of every bundled asset and of data/levels (existence checks).
    pub fn add_dst(&mut self, game: &Path) -> Result<(), String> {
        let data = game.join("data");
        let bundles = data.join("databundles");
        let mut scripts = crate::zipfile::Zip::open(&bundles.join("scripts.zip"))?;
        let names: Vec<String> = scripts.names().map(String::from).collect();
        for name in names {
            if name.ends_with(".lua") {
                let bytes = scripts.read(&name).ok_or_else(|| format!("scripts.zip: cannot read {name}"))?;
                self.entries.insert(format!("dst/scripts.zip/{name}"), Some(bytes));
            }
        }
        for entry in std::fs::read_dir(&bundles).map_err(|e| format!("{}: {e}", bundles.display()))?.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("zip") {
                if let Ok(z) = crate::zipfile::Zip::open(&p) {
                    for n in z.names() {
                        self.entries.entry(format!("dst/assets/{n}")).or_insert(None);
                    }
                }
            }
        }
        walk_names(&data.join("levels"), "dst/data/levels", &mut self.entries);
        self.add_names("dst", |k| k == "dst/scripts.zip/scripts/strings.lua");
        self.add_dst_icons(game)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut sections: BTreeMap<String, Vec<(&String, &Option<Vec<u8>>)>> = BTreeMap::new();
        for (path, data) in &self.entries {
            sections.entry(section_of(path)).or_default().push((path, data));
        }
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
        for (name, entries) in sections {
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
            let mut payload = Vec::new();
            for (path, data) in entries {
                out.extend_from_slice(&(path.len() as u16).to_le_bytes());
                out.extend_from_slice(path.as_bytes());
                out.push(data.is_none() as u8);
                out.extend_from_slice(&(data.as_ref().map_or(0, Vec::len) as u32).to_le_bytes());
                if let Some(d) = data {
                    payload.extend_from_slice(d);
                }
            }
            let packed = miniz_oxide::deflate::compress_to_vec(&payload, 9);
            out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            out.extend_from_slice(&packed);
        }
        out
    }

    /// Decodes the named sections (all of them when `wanted` is None).
    pub fn decode(bytes: &[u8], wanted: Option<&[&str]>) -> Result<Bundle, String> {
        let mut entries = BTreeMap::new();
        for section in Sections::new(bytes)? {
            let section = section?;
            if wanted.is_some_and(|w| !w.contains(&section.name.as_str())) {
                continue;
            }
            let payload = miniz_oxide::inflate::decompress_to_vec(section.data).map_err(|_| bad())?;
            let mut at = 0;
            for (path, name_only, size) in section.index {
                if name_only {
                    entries.insert(path, None);
                } else {
                    entries.insert(path, Some(payload.get(at..at + size).ok_or_else(bad)?.to_vec()));
                    at += size;
                }
            }
        }
        Ok(Bundle { entries })
    }

    pub fn has_ds(&self) -> bool {
        self.entries.contains_key("ds/data/scripts/worldgen_main.lua")
    }

    pub fn has_dst(&self) -> bool {
        self.entries.keys().any(|k| k.starts_with("dst/scripts.zip/"))
    }

    /// DS DLC overlays present in the bundle.
    pub fn ds_dlcs(&self) -> Vec<&'static str> {
        [("DLC0001", "rog"), ("DLC0002", "sw"), ("DLC0003", "hamlet")]
            .into_iter()
            .filter(|(dir, _)| self.entries.keys().any(|k| k.starts_with(&format!("ds/data/{dir}/"))))
            .map(|(_, dlc)| dlc)
            .collect()
    }
}

/// Lays the bundle out under `root` like the installs: `root/ds` is a
/// dont_starve directory and `root/dst` a Don't Starve Together directory
/// (data/databundles/scripts.zip, the asset names in names.zip, and
/// data/levels as empty files).
pub fn unpack(bundle: &Bundle, root: &Path) -> Result<(), String> {
    let write = |path: &Path, bytes: &[u8]| -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    };
    let mut scripts = Vec::new();
    let mut names = Vec::new();
    for (path, data) in &bundle.entries {
        if let Some(rest) = path.strip_prefix("dst/scripts.zip/") {
            scripts.push((rest.to_string(), data.clone().unwrap_or_default()));
        } else if let Some(rest) = path.strip_prefix("dst/assets/") {
            names.push((rest.to_string(), Vec::new()));
        } else if path.starts_with("icons/") || path.starts_with("meta/") || path.starts_with("brand/") {
            continue;
        } else {
            write(&root.join(path), data.as_deref().unwrap_or(&[]))?;
        }
    }
    if !scripts.is_empty() {
        let bundles = root.join("dst/data/databundles");
        write(&bundles.join("scripts.zip"), &crate::zipfile::stored_zip(&scripts))?;
        write(&bundles.join("names.zip"), &crate::zipfile::stored_zip(&names))?;
    }
    Ok(())
}

/// String literals passed to `call(` in Lua source, e.g. every
/// `Prefab("name"` or `SetIcon("x.png"`.
fn string_args(src: &str, call: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find(call) {
        rest = &rest[i + call.len()..];
        let t = rest.trim_start();
        if let Some(q) = t.chars().next().filter(|c| *c == '"' || *c == '\'') {
            if let Some(end) = t[1..].find(q) {
                out.push(t[1..1 + end].to_string());
            }
        }
    }
    out
}

impl Bundle {
    /// Minimap icons for the map view: each prefab whose script sets a
    /// minimap icon (or that has an icon of its own name) gets that icon,
    /// cut out of the game's atlases, as `icons/<game>/<prefab>.png`.
    /// `atlases` are (atlas XML, KTEX texture) pairs in override order;
    /// `scripts` are the prefab scripts as (file stem, contents).
    fn add_icons(&mut self, game: &str, atlases: &[(String, Vec<u8>)], scripts: &[(String, String)]) {
        let mut regions = BTreeMap::new();
        for (xml, tex) in atlases {
            let Ok(img) = crate::ktex::decode(tex) else { continue };
            let img = std::rc::Rc::new(img);
            for (name, uv) in crate::ktex::atlas_elements(xml) {
                regions.insert(name, (img.clone(), uv));
            }
        }
        let mut icon_of: BTreeMap<String, String> = BTreeMap::new();
        for (stem, src) in scripts {
            let icons: Vec<String> = string_args(src, "SetIcon(").into_iter().filter(|i| regions.contains_key(i)).collect();
            // Prefabs named literally, and the file's own name (prefabs made
            // by factory functions, e.g. MakePortal("multiplayer_portal", ...)).
            let mut prefabs: Vec<String> = string_args(src, "Prefab(").iter().map(|p| p.rsplit('/').next().unwrap_or(p).to_string()).collect();
            prefabs.push(stem.clone());
            for prefab in prefabs {
                let own = format!("{prefab}.png");
                let icon = if icons.contains(&own) {
                    Some(own)
                } else if let Some(first) = icons.first() {
                    Some(first.clone())
                } else {
                    regions.contains_key(&own).then_some(own)
                };
                if let Some(icon) = icon {
                    icon_of.entry(prefab).or_insert(icon);
                }
            }
        }
        // Icons named after a prefab, and spawners shown with what they spawn.
        for name in regions.keys() {
            if let Some(base) = name.strip_suffix(".png") {
                icon_of.entry(base.to_string()).or_insert_with(|| name.clone());
            }
        }
        let spawners: Vec<(String, String)> = icon_of
            .keys()
            .chain(scripts.iter().map(|(stem, _)| stem))
            .filter_map(|p| {
                let base = p.strip_suffix("_spawner")?;
                let icon = format!("{base}.png");
                regions.contains_key(&icon).then(|| (p.clone(), icon))
            })
            .collect();
        for (p, icon) in spawners {
            icon_of.entry(p).or_insert(icon);
        }
        let mut pngs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for (prefab, icon) in icon_of {
            let (img, uv) = &regions[&icon];
            let png = pngs.entry(icon.clone()).or_insert_with(|| crate::ktex::png(&crate::ktex::crop(img, *uv))).clone();
            self.entries.insert(format!("icons/{game}/{prefab}.png"), Some(png));
        }
    }

    /// (file stem, contents) of the prefab scripts whose bundle path matches.
    fn prefab_scripts(&self, matches: impl Fn(&str) -> bool) -> Vec<(String, String)> {
        self.entries
            .iter()
            .filter(|(k, _)| matches(k))
            .filter_map(|(k, v)| {
                let stem = k.rsplit('/').next()?.strip_suffix(".lua")?.to_string();
                Some((stem, String::from_utf8_lossy(v.as_ref()?).into_owned()))
            })
            .collect()
    }

    /// Icons of every Don't Starve prefab (base game and DLC atlases).
    pub fn add_ds_icons(&mut self, game: &Path) -> Result<(), String> {
        let data = game.join("data");
        let mut atlases = Vec::new();
        for dir in ["", "DLC0001", "DLC0002", "DLC0003"] {
            let mm = data.join(dir).join("minimap");
            if let (Ok(xml), Ok(tex)) = (std::fs::read_to_string(mm.join("minimap_data.xml")), std::fs::read(mm.join("minimap_atlas.tex"))) {
                atlases.push((xml, tex));
            }
        }
        let scripts = self.prefab_scripts(|k| k.starts_with("ds/data/") && k.contains("/scripts/prefabs/"));
        self.add_icons("ds", &atlases, &scripts);
        Ok(())
    }

    /// Icons of every Don't Starve Together prefab.
    pub fn add_dst_icons(&mut self, game: &Path) -> Result<(), String> {
        let mm = game.join("data").join("minimap");
        let mut atlases = Vec::new();
        for (xml, tex) in [("minimap_data.xml", "minimap_atlas.tex"), ("minimap_data1.xml", "minimap_atlas1.tex"), ("minimap_data2.xml", "minimap_atlas2.tex")] {
            if let (Ok(xml), Ok(tex)) = (std::fs::read_to_string(mm.join(xml)), std::fs::read(mm.join(tex))) {
                atlases.push((xml, tex));
            }
        }
        let scripts = self.prefab_scripts(|k| k.starts_with("dst/scripts.zip/scripts/prefabs/"));
        self.add_icons("dst", &atlases, &scripts);
        Ok(())
    }

    /// The icons of one game as prefab -> PNG bytes.
    pub fn icons(&self, game: &str) -> BTreeMap<String, &[u8]> {
        let prefix = format!("icons/{game}/");
        self.entries
            .iter()
            .filter_map(|(k, v)| Some((k.strip_prefix(&prefix)?.strip_suffix(".png")?.to_string(), v.as_deref()?)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_table() {
        let src = "STRINGS = {\n    CHARACTER_NAMES =\n    {\n        WILSON = \"wilson\",\n    },\n    NAMES =\n    {\n        DEFAULT = \"INVENTORY ITEM\",\n\n        PIGKING = \"Pig King\",\n        NESTED = { A = \"x\" },\n        WALRUS_CAMP = \"Walrus Camp\",\n    },\n}\n";
        let mut out = BTreeMap::new();
        display_names(src, &mut out);
        assert_eq!(out.get("pigking").map(String::as_str), Some("Pig King"));
        assert_eq!(out.get("walrus_camp").map(String::as_str), Some("Walrus Camp"));
        assert!(!out.contains_key("wilson") && !out.contains_key("a"));
    }
}

impl Bundle {
    /// One element of a KTEX atlas.
    fn atlas_image(xml: &Path, element: &str) -> Option<crate::ktex::Image> {
        let text = std::fs::read_to_string(xml).ok()?;
        let tex_name = text.split("filename=\"").nth(1)?.split('"').next()?.to_string();
        let tex = std::fs::read(xml.with_file_name(tex_name)).ok()?;
        let uv = *crate::ktex::atlas_elements(&text).get(element)?;
        let img = crate::ktex::decode(&tex).ok()?;
        Some(crate::ktex::crop(&img, uv))
    }

    /// Branding for the page from the user's installs (title logos and the
    /// hand-drawn button), stored as `brand/<name>.png`.
    pub fn add_brand(&mut self, ds: Option<&Path>, dst: Option<&Path>) {
        let mut put = |name: &str, img: Option<crate::ktex::Image>| {
            if let Some(img) = img {
                self.entries.insert(format!("brand/{name}.png"), Some(crate::ktex::png(&img)));
            }
        };
        let trimmed = |img: Option<crate::ktex::Image>| img.map(|i| crate::ktex::trim(&i));
        if let Some(game) = ds {
            let images = game.join("data").join("images");
            put("title-ds", trimmed(Self::atlas_image(&images.join("ps4.xml"), "ps4_mainmenu_title.tex")));
            put("title-rog", trimmed(Self::atlas_image(&images.join("ps4_dlc0001.xml"), "ps4_mainmenu_title.tex")));
            for element in ["button", "button_over"] {
                put(element, Self::atlas_image(&images.join("ui.xml"), &format!("{element}.tex")));
            }
            put("background", Self::atlas_image(&images.join("bg_loading_loading_night.xml"), "loading_night.tex").map(|i| crate::ktex::half(&i)));
        }
        if let Some(game) = dst {
            put("title-dst", trimmed(Self::atlas_image(&game.join("data").join("images").join("frontscreen.xml"), "title.tex")));
        }
    }
}

/// Every world type a bundle can generate: (game, options) pairs.
pub fn world_types(ds_dlcs: &[&str], dst: bool) -> Vec<(&'static str, crate::api::WorldOptions)> {
    use crate::api::WorldOptions;
    let mut out = Vec::new();
    for &dlc in ds_dlcs {
        let levels: &[(&str, &[u32])] = match dlc {
            "sw" => &[("shipwrecked", &[]), ("volcano", &[]), ("survival", &[])],
            "hamlet" => &[("porkland", &[]), ("survival", &[]), ("shipwrecked", &[]), ("volcano", &[])],
            _ => &[("survival", &[]), ("cave", &[1, 2]), ("adventure", &[1, 2, 3, 4, 5])],
        };
        for (level, sublevels) in levels {
            let subs: Vec<Option<u32>> = if sublevels.is_empty() { vec![None] } else { sublevels.iter().map(|&n| Some(n)).collect() };
            for current_level in subs {
                out.push(("ds", WorldOptions { dlc: Some(dlc.into()), level: Some((*level).into()), current_level, ..Default::default() }));
            }
        }
    }
    if dst {
        for preset in ["SURVIVAL_TOGETHER", "DST_CAVE"] {
            out.push(("dst", WorldOptions { preset: Some(preset.into()), ..Default::default() }));
        }
    }
    out
}

/// The install files world generation uses, found by generating `seeds`
/// worlds of every world type: keys "ds/data/..." and "dst/scripts.zip/...".
/// `progress` gets one line per world type.
pub fn trace_worldgen(ds: Option<&Path>, dst: Option<&Path>, seeds: u32, mut progress: impl FnMut(String)) -> Result<std::collections::BTreeSet<String>, String> {
    let ds_dlcs: Vec<&str> = match ds {
        Some(game) => {
            let data = game.join("data");
            let mut d = vec!["base"];
            for (dir, dlc) in [("DLC0001", "rog"), ("DLC0002", "sw"), ("DLC0003", "hamlet")] {
                if data.join(dir).join("scripts").is_dir() {
                    d.push(dlc);
                }
            }
            d
        }
        None => vec![],
    };
    let mut all = std::collections::BTreeSet::new();
    for (game, opts) in world_types(&ds_dlcs, dst.is_some()) {
        let (dir, prefix) = if game == "ds" { (ds.unwrap(), "ds/") } else { (dst.unwrap(), "dst/") };
        crate::vfs::record_start();
        let gen = if game == "ds" { crate::api::Generator::ds(dir, &opts)? } else { crate::api::Generator::dst(dir, &opts)? };
        let mut after_first = 0;
        let mut failed = 0;
        for seed in 1..=seeds {
            if gen.generate(seed).is_err() {
                failed += 1;
            }
            if seed == 1 {
                let so_far = crate::vfs::record_take();
                after_first = so_far.len();
                crate::vfs::record_start();
                all.extend(so_far.into_iter().map(|k| format!("{prefix}{k}")));
            }
        }
        let rest = crate::vfs::record_take();
        let before = all.len();
        all.extend(rest.into_iter().map(|k| format!("{prefix}{k}")));
        progress(format!(
            "{game} {:?} {:?} {:?}: {after_first} files after seed 1, {} more from seeds 2..{seeds}{}",
            opts.dlc.as_deref().or(opts.preset.as_deref()).unwrap_or(""),
            opts.level.as_deref().unwrap_or(""),
            opts.current_level,
            all.len() - before,
            if failed > 0 { format!(" ({failed} seeds failed)") } else { String::new() }
        ));
    }
    Ok(all)
}

type LuaWriter = unsafe extern "C" fn(*mut crate::lua::ffi::lua_State, *const std::ffi::c_void, usize, *mut std::ffi::c_void) -> std::ffi::c_int;

extern "C" {
    fn ds_dump_wasm32(l: *mut crate::lua::ffi::lua_State, writer: LuaWriter, data: *mut std::ffi::c_void, strip: std::ffi::c_int) -> std::ffi::c_int;
}

unsafe extern "C" fn append(_l: *mut crate::lua::ffi::lua_State, p: *const std::ffi::c_void, size: usize, ud: *mut std::ffi::c_void) -> std::ffi::c_int {
    let out = &mut *(ud as *mut Vec<u8>);
    out.extend_from_slice(std::slice::from_raw_parts(p as *const u8, size));
    0
}

/// Compiles Lua source to a precompiled chunk for the wasm32 build, with
/// debug information stripped.
pub fn compile_wasm32(source: &[u8], name: &str) -> Result<Vec<u8>, String> {
    use crate::lua::ffi;
    let lua = crate::lua::Lua::new();
    let l = lua.raw();
    let cname = std::ffi::CString::new(format!("={name}")).unwrap();
    let mut out = Vec::new();
    unsafe {
        if ffi::luaL_loadbuffer(l, source.as_ptr() as *const _, source.len(), cname.as_ptr()) != 0 {
            let mut len = 0;
            let msg = ffi::lua_tolstring(l, -1, &mut len);
            let msg = String::from_utf8_lossy(std::slice::from_raw_parts(msg as *const u8, len)).into_owned();
            return Err(format!("{name}: {msg}"));
        }
        if ds_dump_wasm32(l, append, &mut out as *mut Vec<u8> as *mut _, 1) != 0 {
            return Err(format!("{name}: could not dump the compiled chunk"));
        }
    }
    Ok(out)
}

impl Bundle {
    /// Reduces the scripts to the files world generation actually uses
    /// (traced by generating every world type), compiled to stripped
    /// bytecode for the wasm32 build. Loading does not depend on the seed,
    /// so one seed per world type finds them all. Icons, names, branding and
    /// asset names are kept as they are.
    pub fn worldgen_only(&mut self, ds: Option<&Path>, dst: Option<&Path>, mut progress: impl FnMut(String)) -> Result<(), String> {
        let used = trace_worldgen(ds, dst, 1, &mut progress)?;
        let is_script = |k: &str| k.starts_with("ds/") || k.starts_with("dst/scripts.zip/");
        self.entries.retain(|k, _| !is_script(k) || used.contains(k));
        for (key, data) in self.entries.iter_mut() {
            if !is_script(key) || !key.ends_with(".lua") {
                continue;
            }
            if let Some(source) = data {
                *source = compile_wasm32(source, key)?;
            }
        }
        Ok(())
    }
}
