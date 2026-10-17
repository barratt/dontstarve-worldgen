//! WebAssembly entry points (Emscripten build, see web/build.ps1).
//!
//! The page mounts the user's own game files into the Emscripten
//! filesystem, or uses the game data a standalone build embeds
//! (`dsw_embedded`), then calls `dsw_generate` with a JSON request:
//!
//! ```json
//! { "game": "ds" | "dst", "root": "/game", "seed": 1001,
//!   "options": { "dlc": "rog", "level": "cave", "current_level": 2,
//!                "choices": "{...}", "preset": "DST_CAVE",
//!                "world_size": "huge", "platform": "linux" },
//!   "where": "prefabs.pigking >= 1", "savedata": false }
//! ```
//!
//! and gets back `{ "summary": {...}, "matched": true }`, plus
//! `"savedata"` and `"tile_names"` (tile id -> ground name) when
//! `"savedata": true`,
//! or `{ "error": "..." }`. The returned string is freed with `dsw_free`.

use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};
use std::path::Path;

use ds_worldgen::api::{Generator, WorldOptions};
use ds_worldgen::filter::Filter;
use serde_json::{json, Value};

thread_local! {
    /// The generator for the last world type requested, reused across seeds.
    static CACHE: RefCell<Option<(String, Generator)>> = const { RefCell::new(None) };
}

fn options(v: &Value) -> WorldOptions {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(String::from);
    WorldOptions {
        dlc: s("dlc"),
        level: s("level"),
        current_level: v.get("current_level").and_then(Value::as_u64).map(|n| n as u32),
        choices: s("choices"),
        preset: s("preset"),
        gen_parameters: s("gen_parameters"),
        world_size: s("world_size"),
        platform: s("platform"),
    }
}

fn handle(request: &str) -> Result<Value, String> {
    let req: Value = serde_json::from_str(request).map_err(|e| format!("bad request: {e}"))?;
    let seed = req.get("seed").and_then(Value::as_u64).filter(|s| *s <= u32::MAX as u64).ok_or("seed must be an integer 0..4294967295")? as u32;
    let root = req.get("root").and_then(Value::as_str).unwrap_or("/game").to_string();
    let game = req.get("game").and_then(Value::as_str).unwrap_or("ds").to_string();
    let opts_json = req.get("options").cloned().unwrap_or(json!({}));
    let key = format!("{game}|{root}|{opts_json}");
    let filter = req.get("where").and_then(Value::as_str).filter(|w| !w.trim().is_empty()).map(Filter::parse).transpose()?;

    if let Some(kind) = root.strip_prefix("/embedded/") {
        prepare_embedded(kind)?;
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.as_ref().map(|(k, _)| k != &key).unwrap_or(true) {
            let opts = options(&opts_json);
            let gen = match game.as_str() {
                "dst" => Generator::dst(Path::new(&root), &opts)?,
                "ds" => Generator::ds(Path::new(&root), &opts)?,
                other => return Err(format!("unknown game {other} (ds, dst)")),
            };
            *cache = Some((key, gen));
        }
        let gen = &cache.as_ref().unwrap().1;
        let world = gen.generate(seed)?;
        let summary = gen.summary(&world)?;
        let matched = filter.as_ref().map_or(true, |f| f.matches(&summary));
        let mut out = json!({ "seed": seed, "matched": matched, "summary": summary });
        if req.get("savedata").and_then(Value::as_bool).unwrap_or(false) {
            out["savedata"] = Value::String(String::from_utf8_lossy(&world.savedata).into_owned());
            out["tile_names"] = json!(gen.tile_names(&world));
            out["positions"] = json!(ds_worldgen::summary::positions(&ds_worldgen::summary::parse_savedata(&world.savedata)?));
        }
        Ok(out)
    })
}

/// # Safety
/// `request` must be a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn dsw_generate(request: *const c_char) -> *mut c_char {
    let request = CStr::from_ptr(request).to_string_lossy();
    let out = match std::panic::catch_unwind(|| handle(&request)) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => json!({ "error": e }),
        Err(_) => json!({ "error": "internal error (panic)" }),
    };
    CString::new(out.to_string()).unwrap_or_default().into_raw()
}

/// # Safety
/// `s` must come from `dsw_generate`.
#[no_mangle]
pub unsafe extern "C" fn dsw_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

// The game data bundle of a standalone build (empty otherwise), see build.rs.
include!(concat!(env!("OUT_DIR"), "/embedded.rs"));

thread_local! {
    /// Games whose embedded scripts this worker has laid out under /embedded.
    static PREPARED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// What the module has built in, from the bundle's index (nothing inflated):
/// `{ "ds": { "root": "/embedded/ds", "dlcs": [...] } | null,
///    "dst": { "root": "/embedded/dst" } | null }`, or `{ "error": ... }`.
fn embedded_info() -> Value {
    if EMBEDDED.is_empty() {
        return json!({ "ds": null, "dst": null });
    }
    let sections = match ds_worldgen::pack::Sections::new(EMBEDDED).and_then(|s| s.collect::<Result<Vec<_>, _>>()) {
        Ok(s) => s,
        Err(e) => return json!({ "error": e }),
    };
    let ds = sections.iter().find(|s| s.name == "ds").map(|s| {
        let dlcs: Vec<&str> = [("DLC0001", "rog"), ("DLC0002", "sw"), ("DLC0003", "hamlet")]
            .into_iter()
            .filter(|(dir, _)| s.index.iter().any(|(p, _, _)| p.starts_with(&format!("ds/data/{dir}/"))))
            .map(|(_, dlc)| dlc)
            .collect();
        json!({ "root": "/embedded/ds", "dlcs": dlcs })
    });
    let dst = sections.iter().any(|s| s.name == "dst").then(|| json!({ "root": "/embedded/dst" }));
    json!({ "ds": ds, "dst": dst })
}

/// Inflates one game's scripts from the embedded bundle into /embedded/<game>
/// (once per worker).
fn prepare_embedded(kind: &str) -> Result<(), String> {
    if PREPARED.with(|p| p.borrow().iter().any(|k| k == kind)) {
        return Ok(());
    }
    let bundle = ds_worldgen::pack::Bundle::decode(EMBEDDED, Some(&[kind]))?;
    ds_worldgen::pack::unpack(&bundle, Path::new("/embedded"))?;
    PREPARED.with(|p| p.borrow_mut().push(kind.to_string()));
    Ok(())
}

/// Describes the game data built into this module; the result is freed
/// with `dsw_free`.
#[no_mangle]
pub extern "C" fn dsw_embedded() -> *mut c_char {
    CString::new(embedded_info().to_string()).unwrap_or_default().into_raw()
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn assets(game: &str) -> Result<Value, String> {
    let bundle = ds_worldgen::pack::Bundle::decode(EMBEDDED, Some(&[&format!("icons-{game}"), &format!("meta-{game}"), "brand"]))?;
    let icons: serde_json::Map<String, Value> = bundle
        .icons(game)
        .into_iter()
        .map(|(prefab, png)| (prefab, Value::String(format!("data:image/png;base64,{}", base64(png)))))
        .collect();
    let names: Value = bundle
        .entries
        .get(&format!("meta/{game}/names.json"))
        .and_then(|d| d.as_ref())
        .and_then(|d| serde_json::from_slice(d).ok())
        .unwrap_or_else(|| json!({}));
    // Title logos, the button texture and a background, from the installs.
    let brand: serde_json::Map<String, Value> = bundle
        .entries
        .iter()
        .filter_map(|(k, v)| {
            let name = k.strip_prefix("brand/")?.strip_suffix(".png")?;
            Some((name.to_string(), Value::String(format!("data:image/png;base64,{}", base64(v.as_ref()?)))))
        })
        .collect();
    Ok(json!({ "icons": icons, "names": names, "notable": ds_worldgen::summary::NOTABLE, "brand": brand }))
}

/// A game's minimap icons (prefab -> PNG data URI), display names (prefab ->
/// name), notable prefabs and branding images, for the page; freed with
/// `dsw_free`.
///
/// # Safety
/// `game` must be a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn dsw_assets(game: *const c_char) -> *mut c_char {
    let game = CStr::from_ptr(game).to_string_lossy();
    let out = assets(&game).unwrap_or_else(|e| json!({ "error": e }));
    CString::new(out.to_string()).unwrap_or_default().into_raw()
}

/// Validates a `--where` expression; returns NULL when it parses, else the error.
///
/// # Safety
/// `expr` must be a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn dsw_check_filter(expr: *const c_char) -> *mut c_char {
    match Filter::parse(&CStr::from_ptr(expr).to_string_lossy()) {
        Ok(_) => std::ptr::null_mut(),
        Err(e) => CString::new(e).unwrap_or_default().into_raw(),
    }
}

fn main() {}
