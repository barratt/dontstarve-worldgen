//! A readable, filterable summary of one generated world.
//!
//! Built only from the savedata the generator returns (the same data the
//! game saves), so it works the same for generated worlds and the game's own
//! output. Field names are stable: `--where` filters and downstream
//! tools address them by path (`prefabs.pigking`, `nearest.pigking.distance`).

use std::collections::BTreeMap;

use serde_json::{json, Map, Value as Json};

use crate::lua::Lua;
use crate::value::Value;

/// Prefabs reported individually with positions and distance from spawn:
/// bosses, unique structures and things players search seeds for.
pub const NOTABLE: &[&str] = &[
    "pigking", "walrus_camp", "resurrectionstone", "wormhole", "cave_entrance", "chester_eyebone",
    "adventure_portal", "beefalo", "lightninggoat", "spiderden", "tallbirdnest", "mermhouse",
    "pighouse", "rabbithole", "pond", "pond_mos", "houndmound", "beehive", "wasphive", "knight",
    "bishop", "rook", "marbletree", "statueharp", "statue_marble", "basalt", "insanityrock",
    "sanityrock", "gravestone", "mandrake", "catcoonden", "buzzardspawner", "monkeybarrel",
    "slurtlehole", "warg", "moose_nesting_ground", "mooseegg", "dragonfly_spawner", "bearger",
    "deerclops", "lureplant", "ancient_altar", "teleportato_base", "teleportato_box",
    "teleportato_crank", "teleportato_potato", "teleportato_ring", "maxwelllight", "sculpture_knight",
    "critterlab", "tumbleweedspawner",
    // caves and ruins (DS and DST)
    "cave_exit", "cave_stairs", "minotaur", "ancient_altar_broken", "pandoraschest", "rabbithouse",
    "batcave", "tentacle_garden", "treasurechest", "skeleton", "slurper", "bishop_nightmare",
    "rook_nightmare", "knight_nightmare", "ruins_statue_mage", "ruins_statue_head", "pond_cave",
    "toadstool_cap", "atrium_gate", "archive_switch", "archive_lockbox_dispencer", "sacred_chest",
    "spiderhole", "dropperweb", "wormlight_plant", "monkeybarrel_spawner",
    // DST
    "multiplayer_portal", "moonbase", "beequeenhive", "dragonfly_spawner", "antlion_spawner", "crabking_spawner",
    "oasislake", "hermithouse_construction1", "monkeyqueen", "lunarrift_portal", "moon_altar_rock_glass",
    "moon_altar_rock_idol", "moon_altar_rock_seed", "klaus_sack", "pigking", "walrus_camp", "stagehand",
    "statueglommer", "sculpture_rookbody", "sculpture_bishopbody", "sculpture_knightbody", "archive_orchestrina_main",
    "terrariumchest", "junk_pile_big", "wobysmall", "lunar_grazer",
];

/// Evaluates the savedata chunk (`return { ... }`) in a fresh Lua state.
pub fn parse_savedata(savedata: &[u8]) -> Result<Value, String> {
    let mut lua = Lua::new();
    let out = lua.exec(savedata, "=savedata")?;
    out.into_iter().next().ok_or_else(|| "savedata chunk returned nothing".to_string())
}

fn field<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Map(entries) => entries.iter().find(|(k, _)| k.as_bytes() == Some(key.as_bytes())).map(|(_, v)| v),
        _ => None,
    }
}

fn items(v: &Value) -> Vec<&Value> {
    match v {
        Value::Arr(a) => a.iter().collect(),
        Value::Map(entries) => {
            // Sparse arrays: order by numeric key.
            let mut numbered: Vec<(f64, &Value)> = entries.iter().filter_map(|(k, v)| k.as_num().map(|n| (n, v))).collect();
            numbered.sort_by(|a, b| a.0.total_cmp(&b.0));
            numbered.into_iter().map(|(_, v)| v).collect()
        }
        _ => vec![],
    }
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_num)
}

fn text(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_bytes).map(|b| String::from_utf8_lossy(b).into_owned())
}

/// Lua values to JSON (tables with string keys become objects).
pub fn to_json(v: &Value) -> Json {
    match v {
        Value::Nil => Json::Null,
        Value::Bool(b) => json!(b),
        Value::Num(n) => json!(n),
        Value::Str(s) => json!(String::from_utf8_lossy(s)),
        Value::Arr(a) => Json::Array(a.iter().map(to_json).collect()),
        Value::ZeroBased(a) => json!(a),
        Value::Function(_) => json!("<function>"),
        Value::Map(entries) => {
            let mut m = Map::new();
            let mut sorted: Vec<_> = entries.iter().collect();
            sorted.sort_by_key(|(k, _)| match k {
                Value::Str(s) => String::from_utf8_lossy(s).into_owned(),
                Value::Num(n) => format!("{n:020.6}"),
                other => format!("{other:?}"),
            });
            for (k, v) in sorted {
                let key = match k {
                    Value::Str(s) => String::from_utf8_lossy(s).into_owned(),
                    Value::Num(n) => format!("{n}"),
                    other => format!("{other:?}"),
                };
                m.insert(key, to_json(v));
            }
            Json::Object(m)
        }
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

pub struct Context<'a> {
    pub dlc: &'a str,
    pub generator: Json,
    /// Tile id -> GROUND name from the DLC's constants.lua (None: ids only).
    pub ground_names: Option<&'a BTreeMap<u8, String>>,
    /// DST: u16 tile ids and their WORLD_TILES names.
    pub dst_tile_names: Option<&'a BTreeMap<u16, String>>,
}

/// GROUND = { NAME = id, ... } from a constants.lua source.
pub fn parse_ground_table(constants_lua: &str) -> BTreeMap<u8, String> {
    let mut out = BTreeMap::new();
    let Some(start) = constants_lua.find("
GROUND =").or_else(|| constants_lua.find("
GROUND=")) else { return out };
    let body = &constants_lua[start..];
    let Some(open) = body.find('{') else { return out };
    let Some(close) = body[open..].find('}') else { return out };
    for line in body[open + 1..open + close].lines() {
        let line = line.split("--").next().unwrap_or("").trim().trim_end_matches(',');
        if let Some((k, v)) = line.split_once('=') {
            if let Ok(id) = v.trim().parse::<u8>() {
                out.entry(id).or_insert_with(|| k.trim().to_string());
            }
        }
    }
    out
}

/// Counts tile ids in the encoded map: base64("VRSN\0" + u32 + cells),
/// DS cells {u8 id, u8 flags}, DST cells u16 ids (little endian).
fn tile_counts(tiles_b64: &[u8], dst: bool) -> Option<BTreeMap<u16, usize>> {
    let raw = base64_decode(tiles_b64)?;
    if raw.len() < 9 || &raw[..4] != b"VRSN" {
        return None;
    }
    let mut counts = BTreeMap::new();
    for cell in raw[9..].chunks_exact(2) {
        let id = if dst { u16::from_le_bytes([cell[0], cell[1]]) } else { cell[0] as u16 };
        *counts.entry(id).or_insert(0) += 1;
    }
    Some(counts)
}

fn base64_decode(s: &[u8]) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for chunk in s.chunks(4) {
        let mut n = 0u32;
        let mut pad = 0;
        for (i, &c) in chunk.iter().enumerate() {
            if c == b'=' {
                pad += 1;
                n <<= 6;
            } else {
                n = (n << 6) | val(c)?;
            }
            let _ = i;
        }
        if chunk.len() < 4 {
            return None;
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

pub fn summarise(save: &Value, ctx: &Context) -> Result<Json, String> {
    let ents = field(save, "ents").ok_or("savedata has no ents")?;
    let map = field(save, "map").ok_or("savedata has no map")?;
    let meta = field(save, "meta");
    let player = field(save, "playerinfo");
    // DS: playerinfo; DST: the multiplayer_portal (players spawn there).
    let portal = field(ents, "multiplayer_portal").and_then(|l| items(l).into_iter().next());
    // DST caves have neither; players arrive by a cave_exit.
    let exit = field(ents, "cave_exit").and_then(|l| items(l).into_iter().next());
    let spawn = match player.or(portal).or(exit) {
        Some(p) => (num(field(p, "x")).unwrap_or(0.0), num(field(p, "z")).unwrap_or(0.0)),
        None => (0.0, 0.0),
    };

    // Prefab counts and the notable prefabs' positions.
    let mut prefabs = BTreeMap::new();
    let mut notable = Map::new();
    let mut nearest = Map::new();
    if let Value::Map(entries) = ents {
        for (k, list) in entries {
            let Some(name) = k.as_bytes().map(|b| String::from_utf8_lossy(b).into_owned()) else { continue };
            let instances = items(list);
            prefabs.insert(name.clone(), instances.len());
            if !NOTABLE.contains(&name.as_str()) {
                continue;
            }
            let mut positions: Vec<Json> = Vec::new();
            let mut best: Option<(f64, f64, f64)> = None;
            for inst in instances {
                let (Some(x), Some(z)) = (num(field(inst, "x")), num(field(inst, "z"))) else { continue };
                let d = ((x - spawn.0).powi(2) + (z - spawn.1).powi(2)).sqrt();
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, x, z));
                }
                positions.push(json!({ "x": x, "z": z, "distance": round2(d) }));
            }
            if let Some((d, x, z)) = best {
                nearest.insert(name.clone(), json!({ "distance": round2(d), "x": x, "z": z }));
            }
            notable.insert(name, Json::Array(positions));
        }
    }

    // Story layout: tasks (biomes) and their rooms, from the topology.
    let topology = field(map, "topology");
    let mut tasks: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut node_count = 0usize;
    if let Some(ids) = topology.and_then(|t| field(t, "ids")) {
        for id in items(ids) {
            let Some(id) = text(Some(id)) else { continue };
            node_count += 1;
            let mut parts = id.split(':');
            let task = parts.next().unwrap_or("").to_string();
            let room = parts.nth(1).unwrap_or("").to_string();
            if task.is_empty() || task == "START" {
                continue;
            }
            tasks.entry(task).or_default().push(room);
        }
    }
    let task_list: Vec<Json> = tasks
        .iter()
        .map(|(name, rooms)| json!({ "name": name, "rooms": rooms.len(), "room_types": rooms }))
        .collect();

    let roads = field(map, "roads").map(|r| items(r).len()).unwrap_or(0);
    // Ground composition from the encoded tile map.
    let mut tiles = Map::new();
    let mut land = 0usize;
    let dst = ctx.dst_tile_names.is_some();
    if let Some(counts) = field(map, "tiles").and_then(Value::as_bytes).and_then(|b| tile_counts(b, dst)) {
        for (id, n) in counts {
            let name = if let Some(names) = ctx.dst_tile_names {
                names.get(&id).cloned()
            } else {
                ctx.ground_names.and_then(|g| g.get(&(id as u8)).cloned())
            }
            .unwrap_or_else(|| format!("tile_{id}"));
            let is_land = if dst {
                !matches!(name.as_str(), "IMPASSABLE" | "INVALID") && !name.starts_with("OCEAN_") && !name.starts_with("FAKE_")
            } else {
                id > 1 && id < 0x80 && !matches!(id, 0x30 | 0x34 | 0x37..=0x3F | 0x40)
            };
            if is_land {
                land += n;
            }
            tiles.insert(name, json!(n));
        }
    }
    Ok(json!({
        "seed": num(meta.and_then(|m| field(m, "seed"))),
        "dlc": ctx.dlc,
        "level_id": text(meta.and_then(|m| field(m, "level_id"))),
        "level_type": text(topology.and_then(|t| field(t, "level_type"))),
        "map": {
            "name": text(field(map, "name")),
            "prefab": text(field(map, "prefab")),
            "width": num(field(map, "width")),
            "height": num(field(map, "height")),
            "nodes": node_count,
            "roads": roads,
            "land_tiles": land,
        },
        "tiles": tiles,
        "spawn": { "x": spawn.0, "z": spawn.1 },
        "tasks": task_list,
        "task_names": tasks.keys().collect::<Vec<_>>(),
        "prefabs": prefabs,
        "nearest": nearest,
        "notable": notable,
        "build_version": text(meta.and_then(|m| field(m, "build_version"))),
        "generator": ctx.generator,
    }))
}

/// Every entity's position (x, z), by prefab: what the map view draws.
pub fn positions(save: &Value) -> BTreeMap<String, Vec<[f64; 2]>> {
    let mut out = BTreeMap::new();
    let Some(Value::Map(entries)) = field(save, "ents") else { return out };
    let round = |v: f64| (v * 100.0).round() / 100.0;
    for (k, list) in entries {
        let Some(name) = k.as_bytes().map(|b| String::from_utf8_lossy(b).into_owned()) else { continue };
        let points: Vec<[f64; 2]> = items(list)
            .into_iter()
            .filter_map(|inst| Some([round(num(field(inst, "x"))?), round(num(field(inst, "z"))?)]))
            .collect();
        if !points.is_empty() {
            out.insert(name, points);
        }
    }
    out
}
