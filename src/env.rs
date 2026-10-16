//! Reproduces the engine's worldgen Lua state and runs the game's own
//! `worldgen_main.lua` in it.
//!
//! What the engine provides (inventoried from the game's worldgen state): the
//! `SEED`, `GEN_PARAMETERS`, `GEN_MODDATA` and `APP_*` globals, `WorldSim`,
//! the save-string buffer used by dumper.lua, `kleiloadlua`,
//! `kleifileexists`, `getrealtime`, `walltime`, `getminimumradius` and
//! `perlin`, and `package.path`. The engine runs `scripts/stacktrace.lua`
//! and then `scripts/worldgen_main.lua`, whose return value is the
//! serialised savedata.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::lua::{Ctx, Lua};
use crate::value::Value;
use crate::vfs::Vfs;
use crate::worldsim::{WorldSim, ENGINE_FUNCTIONS, METHODS};

pub struct Config {
    /// `<game>/data`
    pub data_root: PathBuf,
    /// DLC directories overlaid on `data/`, highest priority first.
    pub overlays: Vec<String>,
    pub mods_dir: Option<PathBuf>,
    pub seed: f64,
    pub gen_parameters: String,
    pub gen_moddata: String,
    pub app_version: String,
    /// Keep the scripts' print output.
    pub capture_log: bool,
}

pub struct Output {
    pub savedata: Vec<u8>,
    pub log: Vec<String>,
    /// Tile id -> GROUND name, read from the worldgen state (works whether
    /// the scripts are source or compiled).
    pub ground_names: std::collections::BTreeMap<u8, String>,
}

/// Engine strings observed in the worldgen state (build 578406).
pub const APP_VERSION: &str = "578406";
const APP_BUILD_DATE: &str = "840";
const APP_BUILD_TIME: &str = "17:53:09";

pub fn run(config: &Config, worldsim: Rc<RefCell<dyn WorldSim>>) -> Result<Output, String> {
    let mut lua = Lua::new();
    let vfs = Rc::new(Vfs::new(&config.data_root, config.overlays.clone(), config.mods_dir.clone()));
    let log = Rc::new(RefCell::new(Vec::<String>::new()));

    lua.set_global("SEED", &Value::Num(config.seed));
    lua.set_global("GEN_PARAMETERS", &Value::str(&config.gen_parameters));
    lua.set_global("GEN_MODDATA", &Value::str(&config.gen_moddata));
    lua.set_global("PLATFORM", &Value::str("WIN32"));
    lua.set_global("APP_VERSION", &Value::str(&config.app_version));
    lua.set_global("APP_BUILD_DATE", &Value::str(APP_BUILD_DATE));
    lua.set_global("APP_BUILD_TIME", &Value::str(APP_BUILD_TIME));
    lua.set_global("APP_REGION", &Value::str("NONE"));
    lua.set_global("CONFIGURATION", &Value::str("PRODUCTION"));

    // print: the engine routes it to its log; keep it out of stdout.
    {
        let log = log.clone();
        let keep = config.capture_log;
        lua.set_global_fn("print", Box::new(move |ctx: &mut Ctx| {
            if keep {
                let parts: Vec<String> = (1..=ctx.top())
                    .map(|i| ctx.arg_str(i).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_else(|| {
                        ctx.arg(i).map(|v| v.to_display()).unwrap_or_default()
                    }))
                    .collect();
                log.borrow_mut().push(parts.join("\t"));
            }
            Ok(0)
        }));
    }

    // kleiloadlua(path): function, nil if missing, or an error string.
    {
        let vfs = vfs.clone();
        lua.set_global_fn("kleiloadlua", Box::new(move |ctx: &mut Ctx| {
            let path = ctx.arg_str(1).ok_or("kleiloadlua: path expected")?;
            let path = String::from_utf8_lossy(&path).into_owned();
            let Some(resolved) = vfs.resolve(&path) else {
                ctx.push(&Value::Nil);
                return Ok(1);
            };
            let source = crate::vfs::read_file(&resolved.path)?;
            let source = crate::lua_patch::apply(&source);
            if let Err(msg) = ctx.load_chunk(&source, &resolved.chunkname) {
                ctx.push(&Value::str(&msg));
            }
            Ok(1)
        }));
    }
    {
        let vfs = vfs.clone();
        lua.set_global_fn("kleifileexists", Box::new(move |ctx: &mut Ctx| {
            let path = ctx.arg_str(1).ok_or("kleifileexists: path expected")?;
            let exists = vfs.resolve(&String::from_utf8_lossy(&path)).is_some();
            ctx.push(&Value::Bool(exists));
            Ok(1)
        }));
    }
    // Wall-clock timers only feed profiling prints; fixed for determinism.
    for name in ["getrealtime", "walltime"] {
        lua.set_global_fn(name, Box::new(|ctx: &mut Ctx| {
            ctx.push(&Value::Num(0.0));
            Ok(1)
        }));
    }

    // Save-string buffer (dumper.lua, USE_SAVEBUFFER).
    let save = Rc::new(RefCell::new(Vec::<u8>::new()));
    {
        let s = save.clone();
        lua.set_global_fn("InitSaveString", Box::new(move |_ctx: &mut Ctx| {
            s.borrow_mut().clear();
            Ok(0)
        }));
        let s = save.clone();
        lua.set_global_fn("AppendSaveString", Box::new(move |ctx: &mut Ctx| {
            let piece = ctx.arg_str(1).ok_or("AppendSaveString: string expected")?;
            s.borrow_mut().extend_from_slice(&piece);
            Ok(0)
        }));
        let s = save.clone();
        lua.set_global_fn("RemoveLastCommaSaveString", Box::new(move |_ctx: &mut Ctx| {
            let mut b = s.borrow_mut();
            if b.last() == Some(&b',') {
                b.pop();
            }
            Ok(0)
        }));
        let s = save.clone();
        lua.set_global_fn("GetSaveString", Box::new(move |ctx: &mut Ctx| {
            let v = Value::Str(s.borrow().clone());
            ctx.push(&v);
            Ok(1)
        }));
        lua.set_global_fn("FinalizeSaveString", Box::new(|_ctx: &mut Ctx| Ok(0)));
    }

    // io.open is not available to worldgen here: nothing may write files.
    lua.set_table_fn("io", "open", Box::new(|ctx: &mut Ctx| {
        ctx.push(&Value::Nil);
        ctx.push(&Value::str("io.open is disabled in ds-worldgen"));
        Ok(2)
    }));

    // WorldSim: a table of methods; the first argument (self) is dropped.
    for &method in METHODS.iter().chain(crate::lua_patch::HELPERS) {
        bind_worldsim(&mut lua, Some("WorldSim"), method, method, worldsim.clone(), 2);
    }
    for &name in ENGINE_FUNCTIONS {
        bind_worldsim(&mut lua, None, name, engine_method(name), worldsim.clone(), 1);
    }

    lua.exec(br#"package.path = "data\\scripts\\?.lua;data\\scriptlibs\\?.lua""#, "=ds-worldgen")?;

    unsafe { crate::lua::ffi::ds_register_helpers(lua.raw()) };
    lua.exec(crate::lua_patch::PRELUDE.as_bytes(), "=dsw_prelude")?;
    let stacktrace = vfs.resolve("scripts/stacktrace.lua").ok_or("scripts/stacktrace.lua not found")?;
    lua.exec(&read(&stacktrace.path)?, "@scripts/stacktrace.lua")?;
    let main = vfs.resolve("scripts/worldgen_main.lua").ok_or("scripts/worldgen_main.lua not found")?;
    let results = lua.exec(&read(&main.path)?, "@scripts/worldgen_main.lua")?;

    worldsim.borrow_mut().finish()?;
    let savedata = match results.into_iter().next() {
        Some(Value::Str(s)) => s,
        Some(Value::Nil) | None => return Err("worldgen_main returned nil (generation failed after retries)".into()),
        Some(other) => return Err(format!("worldgen_main returned {other:?}")),
    };
    let mut ground_names = std::collections::BTreeMap::new();
    let pairs = lua.exec(b"local t = {} for k, v in pairs(rawget(_G, 'GROUND') or {}) do t[#t + 1] = { v, k } end return t", "=ground_names")?;
    if let Some(Value::Arr(pairs)) = pairs.into_iter().next() {
        for p in pairs {
            if let Value::Arr(kv) = p {
                if let (Some(Value::Num(id)), Some(Value::Str(name))) = (kv.first(), kv.get(1)) {
                    ground_names.insert(*id as u8, String::from_utf8_lossy(name).into_owned());
                }
            }
        }
    }
    let log = log.borrow().clone();
    Ok(Output { savedata, log, ground_names })
}

fn read(path: &std::path::Path) -> Result<Vec<u8>, String> {
    crate::vfs::read_file(path).map(|b| b.to_vec())
}

/// DLC overlay directories for a dlc choice (engine `SetDLCEnabled`).
pub fn overlays_for(dlc: &str) -> Result<Vec<String>, String> {
    Ok(match dlc {
        "base" => vec![],
        "rog" => vec!["DLC0001".into()],
        "sw" => vec!["DLC0002".into()],
        "hamlet" => vec!["DLC0003".into()],
        other => return Err(format!("unknown dlc {other} (base, rog, sw, hamlet)")),
    })
}

/// Builds `GEN_PARAMETERS` the way the frontend does for a new world: the
/// table mirrors screens/worldgenscreen.lua, serialised by the game's own json.lua so
/// key order — which `pairs(world_gen_choices)` in forest_map.lua depends
/// on — is identical.
///
/// `extra_choices` is a Lua table constructor merged into the default
/// choices after the defaults (e.g. `{ preset = "SURVIVAL_DEFAULT_PLUS" }`).
/// `current_level` is the cave depth (1 caves, 2 ruins) or adventure level
/// (1..5); it defaults to 1 for both.
pub fn gen_parameters(data_root: &std::path::Path, level_type: &str, dlc: &str, extra_choices: Option<&str>, current_level: Option<u32>) -> Result<String, String> {
    let vfs = Vfs::new(data_root, overlays_for(dlc)?, None);
    let json = vfs.resolve("scripts/json.lua").ok_or("scripts/json.lua not found")?;
    let mut lua = Lua::new();
    lua.exec(&read(&json.path)?, "@scripts/json.lua")?;
    let (rog, sw, hamlet) = match dlc {
        "base" => (false, false, false),
        "rog" => (true, false, false),
        "sw" => (false, true, false),
        "hamlet" => (false, false, true),
        other => return Err(format!("unknown dlc {other}")),
    };
    let lua_str = |s: &str| format!("{s:?}");
    let script = format!(
        r#"
local p = {{}}
p.level_type = {level_type}
p.world_gen_choices = {{ monsters = "default", animals = "default", resources = "default", unprepared = "default" }}
for k, v in pairs({extra}) do p.world_gen_choices[k] = v end
if p.level_type == "adventure" then p.current_level = {level}; p.adventure_progress = 1 end
if p.level_type == "cave" then p.current_level = {level} end
p.profiledata = {{ unlocked_characters = {{}} }}
p.DLCEnabled = {{ {rog}, {sw}, {hamlet} }}
return json.encode(p)
"#,
        level_type = lua_str(level_type),
        extra = extra_choices.unwrap_or("{}"),
        level = current_level.unwrap_or(1),
    );
    match lua.exec(script.as_bytes(), "=gen_parameters")?.into_iter().next() {
        Some(Value::Str(s)) => String::from_utf8(s).map_err(|e| e.to_string()),
        other => Err(format!("gen_parameters: unexpected {other:?}")),
    }
}

/// Registers a WorldSim method (or engine function) on `table` (or as a
/// global). Per-tile methods get a fast path (WorldSim::fast) for calls
/// whose arguments are all numbers. `first` is the stack index of the
/// first real argument.
pub fn bind_worldsim(lua: &mut Lua, table: Option<&str>, name: &str, method: &'static str, ws: Rc<RefCell<dyn WorldSim>>, first: i32) {
    let slow_ws = ws.clone();
    let slow: crate::lua::Callback = Box::new(move |ctx: &mut Ctx| {
        let args = ctx.args_from(first)?;
        let results = slow_ws.borrow_mut().call(method, &args)?;
        Ok(ctx.push_all(&results))
    });
    let Some(m) = crate::worldsim::Fast::of(method) else {
        match table {
            Some(t) => lua.set_table_fn(t, name, slow),
            None => lua.set_global_fn(name, slow),
        }
        return;
    };
    let data = Box::new(FastData { ws, method: m });
    match table {
        Some(t) => lua.set_table_fast_fn(t, name, fast_call, data, first, slow),
        None => lua.set_global_fast_fn(name, fast_call, data, first, slow),
    }
}

struct FastData {
    ws: Rc<RefCell<dyn WorldSim>>,
    method: crate::worldsim::Fast,
}

unsafe extern "C" fn fast_call(ud: *mut std::ffi::c_void, args: *const f64, n: std::ffi::c_int, out: *mut f64) -> std::ffi::c_int {
    let d = &*(ud as *const FastData);
    // A WorldSim call already in progress (one that called back into Lua)
    // keeps the general path and its error.
    let Ok(mut ws) = d.ws.try_borrow_mut() else { return 0 };
    match ws.fast(d.method, std::slice::from_raw_parts(args, n as usize)) {
        Some(crate::worldsim::FastOut::Num(v)) => {
            *out = v;
            1
        }
        Some(crate::worldsim::FastOut::Bool(b)) => {
            *out = b as u8 as f64;
            2
        }
        Some(crate::worldsim::FastOut::Nothing) => 3,
        Some(crate::worldsim::FastOut::Num2(x, y)) => {
            *out = x;
            *out.add(1) = y;
            4
        }
        None => 0,
    }
}

/// The WorldSim method name of an engine function ("$" + name).
pub fn engine_method(name: &str) -> &'static str {
    match name {
        "perlin" => "$perlin",
        "getminimumradius" => "$getminimumradius",
        other => Box::leak(format!("${other}").into_boxed_str()),
    }
}
