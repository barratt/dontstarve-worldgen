//! Don't Starve Together's worldgen Lua state, outside the game.
//!
//! What the engine provides (inventoried from the dedicated server's worldgen
//! state): PCG32 + Klei math.random, truncating float->int,
//! scripts from data/databundles/scripts.zip, the save-string methods on
//! WorldSim, and the engine's join of worldgen_main's returned table of
//! DataDumper strings (0x140119730) into one savedata string.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::lua::ffi;
use crate::lua::{Ctx, Lua};
use crate::pcg;
use crate::value::Value;
use crate::vfs::Vfs;
use crate::worldsim::WorldSim;

pub struct DstConfig {
    /// `<Don't Starve Together>/data`
    pub data_root: PathBuf,
    pub mods_dir: Option<PathBuf>,
    pub seed: f64,
    pub gen_parameters: String,
    pub gen_moddata: String,
    pub capture_log: bool,
}

pub struct DstOutput {
    pub savedata: Vec<u8>,
    pub log: Vec<String>,
    /// Tile id -> WORLD_TILES name (DST assigns ids at load time).
    pub tile_names: std::collections::BTreeMap<u16, String>,
}

pub const APP_VERSION: &str = "756039";

/// The 63 WorldSimActual methods of the x64 dedicated server
/// (the save-string methods are handled in Rust).
pub const DST_METHODS: &[&str] = &[
    "SetResolveNoiseFunction", "SetValidateGroundTileFunction", "SetPointsBarrenOrReservedTile", "ResetAll",
    "AddExternalLink", "AddLink", "AddChild", "AddEdgePoints", "SetSiteFlags", "RemoveLink", "ClearNodeLinks",
    "SetNodeType", "ForceDisconectedCells", "SetWorldSize", "GetWorldSize", "GenerateVoronoiMap",
    "WorldGen_AddNewPositions", "WorldGen_InitializeNodePoints", "WorldGen_VoronoiPass", "WorldGen_Commit",
    "SetImpassibleTileType", "ConvertToTileMap", "SeparateIslands", "ForceConnectivity", "DrawRoads",
    "SetRoadParameters", "CreateNodeIdTileMap", "SetTileNodeId", "GenerateBlendedMap", "RunCA", "RunMaze",
    "GetPointsForMetaMaze", "GetEncodedMap", "ReplaceSingleNonLandTiles", "GetPointsForSite",
    "GetRandomPointsForSite", "GetSiteCentroid", "GetSitePolygon", "PointInSite", "GetSiteArea", "GetSite",
    "GetChildrenForSite", "GetVisualTileAtPosition", "GetRoad", "GetWormholes", "GetWormholesExtra",
    "ReserveSpace", "ReserveTile", "IsTileReserved", "SetTile", "GetTile", "DrawCellLine", "DrawGroundLine",
    "LuaPrint", "ClearSafeFromDisconnect", "MakeSafeFromDisconnect", "DetectDisconnect",
    "GenerateSessionIdentifier",
];

const ENGINE_FUNCTIONS: &[&str] = &["perlin", "getminimumradius"];


fn utf8_chars(s: &[u8]) -> Vec<char> {
    String::from_utf8_lossy(s).chars().collect()
}

pub fn run(config: &DstConfig, worldsim: Rc<RefCell<dyn WorldSim>>) -> Result<DstOutput, String> {
    unsafe { ffi::ds_set_number2int_round(0) };
    let result = run_inner(config, worldsim);
    unsafe { ffi::ds_set_number2int_round(1) };
    result
}

fn run_inner(config: &DstConfig, worldsim: Rc<RefCell<dyn WorldSim>>) -> Result<DstOutput, String> {
    let mut lua = Lua::new();
    let vfs = Rc::new(Vfs::with_scripts_zip(&config.data_root, config.mods_dir.clone())?);
    let log = Rc::new(RefCell::new(Vec::<String>::new()));

    for (k, v) in [
        ("GEN_PARAMETERS", config.gen_parameters.as_str()),
        ("GEN_MODDATA", config.gen_moddata.as_str()),
        ("PLATFORM", "WIN32"),
        ("CONFIGURATION", "PRODUCTION"),
        ("BRANCH", "release"),
        ("MODS_ROOT", "../mods/"),
        ("APP_REGION", "NONE"),
        ("APP_VERSION", APP_VERSION),
        ("APP_BUILD_DATE", "441"),
        ("APP_BUILD_TIME", "01:32:27"),
    ] {
        lua.set_global(k, &Value::str(v));
    }
    lua.set_global("SEED", &Value::Num(config.seed));
    lua.set_global("IS_STEAM_DECK", &Value::Bool(false));

    {
        let log = log.clone();
        let keep = config.capture_log;
        lua.set_global_fn("print", Box::new(move |ctx: &mut Ctx| {
            if keep {
                let parts: Vec<String> = (1..=ctx.top()).map(|i| ctx.arg(i).map(|v| v.to_display()).unwrap_or_default()).collect();
                log.borrow_mut().push(parts.join("\t"));
            }
            Ok(0)
        }));
    }
    {
        let vfs = vfs.clone();
        lua.set_global_fn("kleiloadlua", Box::new(move |ctx: &mut Ctx| {
            let path = String::from_utf8_lossy(&ctx.arg_str(1).ok_or("kleiloadlua: path expected")?).into_owned();
            let Some((source, chunkname)) = vfs.read(&path) else {
                ctx.push(&Value::Nil);
                return Ok(1);
            };
            let source = crate::lua_patch::apply(&source);
            if let Err(msg) = ctx.load_chunk(&source, &chunkname) {
                ctx.push(&Value::str(&msg));
            }
            Ok(1)
        }));
    }
    {
        let vfs = vfs.clone();
        lua.set_global_fn("kleifileexists", Box::new(move |ctx: &mut Ctx| {
            let path = String::from_utf8_lossy(&ctx.arg_str(1).ok_or("kleifileexists: path expected")?).into_owned();
            ctx.push(&Value::Bool(vfs.exists(&path)));
            Ok(1)
        }));
    }
    for name in ["getrealtime", "walltime"] {
        lua.set_global_fn(name, Box::new(|ctx: &mut Ctx| {
            ctx.push(&Value::Num(0.0));
            Ok(1)
        }));
    }
    for name in ["kleiregistermods", "requireeventfile"] {
        lua.set_global_fn(name, Box::new(|_ctx: &mut Ctx| Ok(0)));
    }
    // stringidsorter(a, b) (0x140118e00): unsigned-byte strcmp(a, b) < 0.
    lua.set_global_fn("stringidsorter", Box::new(|ctx: &mut Ctx| {
        let a = ctx.arg_str(1).unwrap_or_default();
        let b = ctx.arg_str(2).unwrap_or_default();
        ctx.push(&Value::Bool(a < b));
        Ok(1)
    }));
    for name in ["toarray", "toarrayornil"] {
        let ornil = name == "toarrayornil";
        lua.set_global_fn(name, Box::new(move |ctx: &mut Ctx| {
            let args = ctx.args()?;
            if ornil && args.is_empty() {
                ctx.push(&Value::Nil);
            } else {
                ctx.push(&Value::Arr(args));
            }
            Ok(1)
        }));
    }
    // utf8 helpers (globals and string.*).
    let utf8_len: fn(&mut Ctx) -> Result<i32, String> = |ctx| {
        let s = ctx.arg_str(1).unwrap_or_default();
        ctx.push(&Value::Num(utf8_chars(&s).len() as f64));
        Ok(1)
    };
    let utf8_upper: fn(&mut Ctx) -> Result<i32, String> = |ctx| {
        let s = ctx.arg_str(1).unwrap_or_default();
        ctx.push(&Value::str(&String::from_utf8_lossy(&s).to_uppercase()));
        Ok(1)
    };
    let utf8_lower: fn(&mut Ctx) -> Result<i32, String> = |ctx| {
        let s = ctx.arg_str(1).unwrap_or_default();
        ctx.push(&Value::str(&String::from_utf8_lossy(&s).to_lowercase()));
        Ok(1)
    };
    let utf8_sub: fn(&mut Ctx) -> Result<i32, String> = |ctx| {
        let chars = utf8_chars(&ctx.arg_str(1).unwrap_or_default());
        let n = chars.len() as i64;
        let norm = |i: i64| if i < 0 { (n + i + 1).max(0) } else { i };
        let i = norm(ctx.arg_num(2).unwrap_or(1.0) as i64).max(1);
        let j = norm(ctx.arg_num(3).unwrap_or(-1.0) as i64).min(n);
        let s: String = if i > j { String::new() } else { chars[(i - 1) as usize..j as usize].iter().collect() };
        ctx.push(&Value::str(&s));
        Ok(1)
    };
    let utf8_char: fn(&mut Ctx) -> Result<i32, String> = |ctx| {
        let s: String = (1..=ctx.top()).filter_map(|i| ctx.arg_num(i)).filter_map(|c| char::from_u32(c as u32)).collect();
        ctx.push(&Value::str(&s));
        Ok(1)
    };
    for (g, f) in [("utf8strlen", utf8_len), ("utf8strtoupper", utf8_upper), ("utf8strtolower", utf8_lower), ("utf8substr", utf8_sub), ("utf8char", utf8_char)] {
        lua.set_global_fn(g, Box::new(f));
    }
    for (g, f) in [("utf8len", utf8_len), ("utf8upper", utf8_upper), ("utf8lower", utf8_lower), ("utf8sub", utf8_sub), ("utf8char", utf8_char)] {
        lua.set_table_fn("string", g, Box::new(f));
    }
    lua.set_global_fn("string_search_subwords", Box::new(|ctx: &mut Ctx| {
        ctx.push(&Value::Bool(false));
        Ok(1)
    }));
    // bit (32-bit).
    let bits: [(&str, fn(u32, u32) -> u32); 5] = [
        ("band", |a, b| a & b),
        ("bor", |a, b| a | b),
        ("bxor", |a, b| a ^ b),
        ("lshift", |a, b| a.wrapping_shl(b & 31)),
        ("rshift", |a, b| a.wrapping_shr(b & 31)),
    ];
    for (name, f) in bits {
        lua.set_table_fn("bit", name, Box::new(move |ctx: &mut Ctx| {
            let a = ctx.arg_num(1).unwrap_or(0.0) as i64 as u32;
            let mut acc = a;
            for i in 2..=ctx.top().max(2) {
                acc = f(acc, ctx.arg_num(i).unwrap_or(0.0) as i64 as u32);
            }
            ctx.push(&Value::Num(acc as i32 as f64));
            Ok(1)
        }));
    }
    lua.set_table_fn("bit", "bnot", Box::new(|ctx: &mut Ctx| {
        let a = ctx.arg_num(1).unwrap_or(0.0) as i64 as u32;
        ctx.push(&Value::Num((!a) as i32 as f64));
        Ok(1)
    }));
    // math.random / randomseed: Klei's versions over PCG32 (0x140497780).
    lua.set_table_fn("math", "random", Box::new(|ctx: &mut Ctx| {
        let r = pcg::lua_draw();
        let trunc = |v: f64| v as i64 as i32;
        let out = match ctx.top() {
            0 => r,
            1 => {
                let m = trunc(ctx.arg_num(1).unwrap_or(0.0));
                if m < 1 {
                    return Err("bad argument #1 to 'random' (interval is empty)".into());
                }
                (m as f64 * r).floor() + 1.0
            }
            _ => {
                let l = trunc(ctx.arg_num(1).unwrap_or(0.0));
                let u = trunc(ctx.arg_num(2).unwrap_or(0.0));
                if l > u {
                    return Err("bad argument #2 to 'random' (interval is empty)".into());
                }
                (u.wrapping_sub(l).wrapping_add(1) as f64 * r).floor() + l as f64
            }
        };
        ctx.push(&Value::Num(out));
        Ok(1)
    }));
    lua.set_table_fn("math", "randomseed", Box::new(|ctx: &mut Ctx| {
        let s = ctx.arg_num(1).unwrap_or(0.0) as i64 as u32;
        pcg::srand(s);
        Ok(0)
    }));
    lua.set_table_fn("io", "open", Box::new(|ctx: &mut Ctx| {
        ctx.push(&Value::Nil);
        ctx.push(&Value::str("io.open is disabled in ds-worldgen"));
        Ok(2)
    }));

    // Engine objects whose __index Lua extends (tilegroups.lua), and stubs.
    lua.exec(
        br#"
TileGroupManager = setmetatable({}, { __index = {} })
ManifestManager = { LoadModManifest = function() end, UnloadModManifest = function() end, AddFileToModManifest = function() end }
package.path = "scripts\\?.lua;scriptlibs\\?.lua"
"#,
        "=ds-worldgen",
    )?;

    // WorldSim: save-string methods locally, everything else native.
    let save = Rc::new(RefCell::new(Vec::<u8>::new()));
    for &method in DST_METHODS.iter().chain(crate::lua_patch::HELPERS) {
        crate::env::bind_worldsim(&mut lua, Some("WorldSim"), method, method, worldsim.clone(), 2);
    }
    {
        let s = save.clone();
        lua.set_table_fn("WorldSim", "InitSaveString", Box::new(move |_ctx: &mut Ctx| {
            s.borrow_mut().clear();
            Ok(0)
        }));
        let s = save.clone();
        lua.set_table_fn("WorldSim", "AppendSaveString", Box::new(move |ctx: &mut Ctx| {
            let piece = ctx.arg_str(2).ok_or("AppendSaveString: string expected")?;
            s.borrow_mut().extend_from_slice(&piece);
            Ok(0)
        }));
        let s = save.clone();
        lua.set_table_fn("WorldSim", "RemoveLastCommaSaveString", Box::new(move |_ctx: &mut Ctx| {
            let mut b = s.borrow_mut();
            if b.last() == Some(&b',') {
                b.pop();
            }
            Ok(0)
        }));
        let s = save.clone();
        lua.set_table_fn("WorldSim", "GetSaveString", Box::new(move |ctx: &mut Ctx| {
            let v = Value::Str(s.borrow().clone());
            ctx.push(&v);
            Ok(1)
        }));
        lua.set_table_fn("WorldSim", "FinalizeSaveString", Box::new(|_ctx: &mut Ctx| Ok(0)));
    }
    for &name in ENGINE_FUNCTIONS {
        crate::env::bind_worldsim(&mut lua, None, name, crate::env::engine_method(name), worldsim.clone(), 1);
    }

    unsafe { crate::lua::ffi::ds_register_helpers(lua.raw()) };
    lua.exec(crate::lua_patch::PRELUDE.as_bytes(), "=dsw_prelude")?;
    let (st, _) = vfs.read("scripts/stacktrace.lua").ok_or("scripts/stacktrace.lua not found in scripts.zip")?;
    lua.exec(&st, "scripts/stacktrace.lua")?;
    let (main, _) = vfs.read("scripts/worldgen_main.lua").ok_or("scripts/worldgen_main.lua not found in scripts.zip")?;
    let results = lua.exec(&main, "@scripts/worldgen_main.lua")?;
    let savedata = match results.into_iter().next() {
        Some(data @ Value::Map(_)) => engine_join(&data)?,
        Some(Value::Str(s)) => s,
        Some(Value::Nil) | None => return Err("worldgen_main returned nil (generation failed after retries)".into()),
        Some(other) => return Err(format!("worldgen_main returned {other:?}")),
    };
    worldsim.borrow_mut().finish()?;
    let mut tile_names = std::collections::BTreeMap::new();
    let names = lua.exec(b"local t = {} for k, v in pairs(rawget(_G, 'WORLD_TILES') or {}) do t[#t + 1] = { v, k } end return t", "=tile_names")?;
    if let Some(Value::Arr(pairs)) = names.into_iter().next() {
        for p in pairs {
            if let Value::Arr(kv) = p {
                if let (Some(Value::Num(id)), Some(Value::Str(name))) = (kv.first(), kv.get(1)) {
                    tile_names.insert(*id as u16, String::from_utf8_lossy(name).into_owned());
                }
            }
        }
    }
    let log = log.borrow().clone();
    Ok(DstOutput { savedata, log, tile_names })
}

/// The engine's join (0x140119730) of worldgen_main's `data` table, in
/// next() order as the engine's lua_next walk sees it.
fn engine_join(data: &Value) -> Result<Vec<u8>, String> {
    let Value::Map(entries) = data else { return Err("worldgen_main did not return a table".into()) };
    let text = |v: &Value| v.as_bytes().map(|b| b.to_vec()).ok_or_else(|| format!("savedata part is not a string: {v:?}"));
    let mut out = b"local savedata = {}
local tablefunctions = {}
".to_vec();
    for (k, v) in entries {
        let k = String::from_utf8_lossy(k.as_bytes().ok_or("non-string savedata key")?).into_owned();
        if k == "ents" {
            out.extend_from_slice(b"savedata[\"ents\"] = {}
");
            let inner: Vec<(Value, Value)> = match v {
                Value::Map(e) => e.clone(),
                Value::Arr(a) if a.is_empty() => vec![],
                other => return Err(format!("ents is {other:?}")),
            };
            for (p, s) in &inner {
                let p = String::from_utf8_lossy(p.as_bytes().ok_or("non-string prefab key")?).into_owned();
                out.extend_from_slice(format!("tablefunctions[\"ents_{p}_fn\"] = function()
").as_bytes());
                out.extend_from_slice(&text(s)?);
                out.extend_from_slice(format!("
end
savedata[\"ents\"][\"{p}\"] = tablefunctions[\"ents_{p}_fn\"]()
").as_bytes());
            }
        } else {
            out.extend_from_slice(format!("tablefunctions[\"{k}_fn\"] = function()
").as_bytes());
            out.extend_from_slice(&text(v)?);
            out.extend_from_slice(format!("
end
savedata[\"{k}\"] = tablefunctions[\"{k}_fn\"]()
").as_bytes());
        }
    }
    out.extend_from_slice(b"return savedata\n");
    Ok(out)
}
