//! The Rust port of the game's native `WorldSimActual`.
//!
//! Each method reproduces the game's machine code exactly, including float
//! precision and random-number draws. `call` returns `None` for methods
//! that are not ported.

pub mod boostmap;
pub mod ca;
pub mod encode;
pub mod islands;
pub mod layout;
pub mod maze;
pub mod roads;
pub mod points;
pub mod tiles;
pub mod voronoi;

use std::collections::BTreeMap;

use crate::value::Value;
use crate::worldsim::WorldSim;
use boostmap::BoostMap;
use tiles::TileGrid;

#[derive(Default)]
pub struct Native {
    pub map: BoostMap,
    pub tiles: Option<TileGrid>,
    /// Grid4 (WorldSimActual+0x0C): task name per stamped cell.
    pub names: Vec<Option<usize>>,
    /// The interned task-name set (WorldSimActual+0x1C), name -> id.
    pub task_names: BTreeMap<String, usize>,
    /// The same names by id.
    pub task_name_list: Vec<String>,
    /// Island -> region (WorldSimActual+0x10), AddIslandRegionMapping; not
    /// reset by ResetAll (it lives outside the BoostMap).
    pub island_regions: BTreeMap<String, String>,
    pub road_params: roads::RoadParams,
}

/// Lua 5.1 `lua_tointeger` in the game: x87 fistp, round half to even.
pub fn lua_int(v: f64) -> i32 {
    let r = v.round_ties_even();
    if !(r >= -2147483648.0 && r <= 2147483647.0) { i32::MIN } else { r as i32 }
}

/// Numeric argument as the game's wrappers read it: lua_tonumber /
/// lua_tointeger, which give 0 for nil and non-numeric values (numeric
/// strings convert). Never an error, as in the game.
fn num(args: &[Value], i: usize) -> Result<f64, String> {
    Ok(match args.get(i) {
        Some(Value::Num(n)) => *n,
        Some(Value::Str(s)) => std::str::from_utf8(s).ok().and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.0),
        _ => 0.0,
    })
}

fn opt_num(args: &[Value], i: usize) -> Option<f64> {
    args.get(i).and_then(Value::as_num)
}

fn text(args: &[Value], i: usize) -> Result<String, String> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(String::from_utf8_lossy(s).into_owned()),
        Some(Value::Num(n)) => Ok(format!("{n}")),
        _ => Err(format!("argument {} must be a string", i + 1)),
    }
}

fn nums(xs: impl IntoIterator<Item = f64>) -> Value {
    Value::Arr(xs.into_iter().map(Value::Num).collect())
}

impl Native {
    pub fn new() -> Native {
        Native::default()
    }

    /// `None`: not implemented (yet).
    pub fn call(&mut self, method: &str, args: &[Value]) -> Option<Result<Vec<Value>, String>> {
        let r = match method {
            "LuaPrint" => Ok(vec![]),
            "AddChild" => self.add_child(args),
            "AddLink" | "AddExternalLink" => (|| {
                self.map.add_link(&text(args, 0)?, &text(args, 1)?);
                Ok(vec![])
            })(),
            "ClearNodeLinks" => text(args, 0).map(|id| {
                self.map.clear_node_links(&id);
                vec![]
            }),
            "RemoveLink" => (|| {
                self.map.remove_link(&text(args, 0)?, &text(args, 1)?)?;
                Ok(vec![])
            })(),
            "SetNodeType" => (|| {
                let id = text(args, 0)?;
                let t = lua_int(num(args, 1)?);
                if let Some(n) = self.map.node(&id) {
                    self.map.nodes[n].kind = t;
                }
                Ok(vec![])
            })(),
            "SetSiteFlags" => (|| {
                let id = text(args, 0)?;
                let f = lua_int(num(args, 1)?);
                if let Some(n) = self.map.node(&id) {
                    self.map.nodes[n].site_flags = f;
                }
                Ok(vec![])
            })(),
            "SetWorldSize" => (|| {
                let (w, h) = (num(args, 0)?.trunc() as i32 as u32, num(args, 1)?.trunc() as i32 as u32);
                self.map.set_world_size_core(w, h);
                Ok(vec![])
            })(),
            "GenerateVoronoiMap" => (|| {
                let border = opt_num(args, 2).map(lua_int).unwrap_or(20);
                Ok(vec![Value::Bool(self.map.generate_voronoi_map(border)?)])
            })(),
            "GetSite" => self.with_node(args, |m, n| {
                let s = &m.sites[m.nodes[n].site?];
                Some(vec![Value::Num(s.x as f64), Value::Num(s.y as f64)])
            }),
            "GetSiteCentroid" => self.with_node(args, |m, n| {
                let c = m.nodes[n].centroid;
                Some(vec![Value::Num(c.0 as f64), Value::Num(c.1 as f64)])
            }),
            "GetSitePolygon" => self.with_node(args, |m, n| {
                let p = &m.nodes[n].poly;
                Some(vec![nums(p.iter().map(|q| q.0 as f64)), nums(p.iter().map(|q| q.1 as f64))])
            }),
            "GetSiteArea" => (|| {
                let id = text(args, 0)?;
                let pts = self.map.site_points(&id).ok_or_else(|| format!("GetSiteArea: unknown site {id}"))?;
                Ok(vec![Value::Num(pts.len() as f64)])
            })(),
            "GetChildrenForSite" => (|| {
                let id = text(args, 0)?;
                let Some(parent) = self.map.node(&id) else { return Ok(vec![]) };
                let children: Vec<Value> = self
                    .map
                    .node_by_name
                    .iter()
                    .filter(|(_, &c)| self.map.nodes[c].parent == Some(parent) && self.map.nodes[c].site.is_some())
                    .map(|(name, _)| Value::str(name))
                    .collect();
                Ok(if children.is_empty() { vec![] } else { vec![Value::Arr(children)] })
            })(),
            "SetImpassibleTileType" => num(args, 0).map(|t| {
                self.map.default_tile = (t as f32).trunc() as i32 as u8;
                vec![]
            }),
            "ConvertToTileMap" => self.convert_to_tile_map(args),
            // ResetAll (0x004a4720): delete and re-create the BoostMap only;
            // the tile grid stays until the next ConvertToTileMap.
            "ResetAll" => {
                self.map = BoostMap::default();
                Ok(vec![])
            }
            "GetWorldSize" => Ok(vec![Value::Num(self.map.width.trunc()), Value::Num(self.map.height.trunc())]),
            "GetTile" => self.tiles_op(|t| Ok(vec![Value::Num(points::get_tile(t, lua_int(num(args, 0)?), lua_int(num(args, 1)?)) as f64)])),
            "SetTile" => self.tiles_mut(|t| {
                let f = opt_num(args, 3).map(lua_int).unwrap_or(0);
                points::set_tile(t, lua_int(num(args, 0)?), lua_int(num(args, 1)?), lua_int(num(args, 2)?), f);
                Ok(vec![])
            }),
            "ReserveTile" => self.tiles_mut(|t| Ok(vec![Value::Bool(points::reserve_tile(t, lua_int(num(args, 0)?), lua_int(num(args, 1)?)))])),
            "IsTileReserved" => self.tiles_op(|t| Ok(vec![Value::Bool(points::is_tile_reserved(t, lua_int(num(args, 0)?), lua_int(num(args, 1)?)))])),
            "GetVisualTileAtPosition" => self.tiles_op(|t| Ok(vec![Value::Num(points::visual_tile(t, num(args, 0)?, num(args, 1)?) as f64)])),
            "IsWater" => num(args, 0).map(|t| vec![Value::Bool(tiles::is_water(lua_int(t) as u8))]),
            "IsShore" => num(args, 0).map(|t| vec![Value::Bool(tiles::is_shore(lua_int(t) as u8))]),
            "IsLand" => num(args, 0).map(|t| {
                let t = lua_int(t);
                vec![Value::Bool(t > 1 && t != 0x32 && !tiles::is_water(t as u8))]
            }),
            "SeparateIslands" => (|| {
                let t = self.tiles.as_mut().ok_or("SeparateIslands before ConvertToTileMap")?;
                islands::separate_islands(&mut self.map, t);
                Ok(vec![])
            })(),
            "ForceConnectivity" => (|| {
                let join = matches!(args.first(), Some(Value::Bool(true)));
                let tile = opt_num(args, 2).map(|t| t.trunc() as i32 as u8).unwrap_or(4);
                let t = self.tiles.as_mut().ok_or("ForceConnectivity before ConvertToTileMap")?;
                islands::force_connectivity(&mut self.map, t, join, tile)?;
                Ok(vec![])
            })(),
            "GetWormholesExtra" => (|| {
                let t = self.tiles.as_mut().ok_or("GetWormholesExtra before ConvertToTileMap")?;
                Ok(match islands::get_wormholes_extra(&self.map, t) {
                    None => vec![Value::Nil, Value::Nil, Value::Nil, Value::Nil],
                    Some(arrs) => arrs
                        .into_iter()
                        .map(|mut a| {
                            // The game fills createtable(N, 0) with settable, so
                            // nils leave holes. A non-nil prefix followed only by
                            // nils is the same Lua table (same #) as the prefix
                            // alone.
                            let k = a.iter().take_while(|v| v.is_some()).count();
                            if a[k..].iter().all(|v| v.is_none()) {
                                a.truncate(k);
                            }
                            Value::Arr(a.into_iter().map(|v| v.map(Value::Num).unwrap_or(Value::Nil)).collect())
                        })
                        .collect(),
                })
            })(),
            "SetRoadParameters" => (|| {
                self.road_params = roads::RoadParams {
                    nsub: lua_int(num(args, 0)?),
                    min_w: num(args, 1)? as f32,
                    max_w: num(args, 2)? as f32,
                    min_e: num(args, 3)? as f32,
                    max_e: num(args, 4)? as f32,
                    jitter: num(args, 5)? as f32,
                };
                Ok(vec![])
            })(),
            "DrawRoads" => (|| {
                let join = matches!(args.first(), Some(Value::Bool(true)));
                let t = self.tiles.as_mut().ok_or("DrawRoads before ConvertToTileMap")?;
                roads::draw_roads(&mut self.map, t, &self.road_params, join)?;
                Ok(vec![])
            })(),
            "GetRoad" => (|| {
                let mut idx = lua_int(num(args, 0)?) as i64;
                let join = matches!(args.get(1), Some(Value::Bool(true)));
                let rs = roads::get_roads(&mut self.map, join)?;
                if idx > rs.len() as i64 {
                    idx = 0;
                }
                let r = rs.get(idx as usize).cloned().unwrap_or_default();
                Ok(vec![
                    Value::Num(rs.len() as f64),
                    Value::Num(1.0),
                    nums(r.iter().map(|p| p.0 as f64)),
                    nums(r.iter().map(|p| p.1 as f64)),
                ])
            })(),
            "GetEncodedMap" => (|| {
                let Some(t) = self.tiles.as_mut() else { return Ok(vec![Value::str("")]) };
                let enc = encode::get_encoded_map(&self.map, t);
                Ok(vec![Value::Str(enc.tiles), Value::Str(enc.nav), Value::Arr(enc.adj.into_iter().map(nums).collect())])
            })(),
            // Engine global perlin(x, y, z) (0x4A3D40): the tile stamper's
            // Improved Noise (0x55D910), already remapped to [0, 1].
            "$perlin" => (|| Ok(vec![Value::Num(tiles::perlin01(num(args, 0)?, num(args, 1)?, num(args, 2)?))]))(),
            // PointInSite(name, x, z) (0x4A5AC0): winding test on the cell polygon.
            "PointInSite" => (|| {
                let id = text(args, 0)?;
                let (x, z) = (num(args, 1)? as f32, num(args, 2)? as f32);
                let inside = self.map.node(&id).is_some_and(|n| boostmap::point_in_poly((x, z), &self.map.nodes[n].poly));
                Ok(vec![Value::Bool(inside)])
            })(),
            // GenerateBlendedMap(ks, sigma, levels, default) (0x4A6760): blurred
            // per-tile lookup; writes no tiles.
            "GenerateBlendedMap" => (|| {
                let g = self.tiles.as_ref().ok_or("GenerateBlendedMap before ConvertToTileMap")?;
                let (w, h) = (g.w, g.h);
                let ks = ((lua_int(num(args, 0)?) | 1).clamp(1, 255)) as usize;
                let sigma = num(args, 1)? as f32;
                let Some(Value::Arr(levels)) = args.get(2) else { return Ok(vec![]) };
                let default = opt_num(args, 3).map(|d| d as f32).unwrap_or(0.0);
                let mut lut = [default; 128];
                for e in levels {
                    if let Value::Arr(pair) = e {
                        if pair.len() >= 2 {
                            let tile = lua_int(pair[0].as_num().unwrap_or(0.0));
                            let v = pair[1].as_num().unwrap_or(0.0) as f32;
                            if (tile as u32) <= 0x7F {
                                lut[tile as usize] = v;
                            }
                        }
                    }
                }
                let mut a = Vec::with_capacity(w * h);
                for c in &g.cells {
                    if c[0] >= 0x80 {
                        return Err("GenerateBlendedMap: tile id >= 0x80 reads uninitialised memory in the game".into());
                    }
                    a.push(lut[c[0] as usize]);
                }
                let k = tiles::gaussian_kernel(ks, sigma);
                let b = tiles::blur(&a, w, h, &k, ks, 1);
                let a = tiles::blur(&b, w, h, &k, 1, ks);
                Ok(vec![Value::ZeroBased(a.into_iter().map(|v| v as f64).collect()), Value::Num(w as f64), Value::Num(h as f64)])
            })(),
            "AddIslandRegionMapping" => (|| {
                // std::map insert keeps the first value for a duplicate key.
                self.island_regions.entry(text(args, 0)?).or_insert(text(args, 1)?);
                Ok(vec![])
            })(),
            "DeleteSpecifiedTiles" => (|| {
                self.delete_specified_tiles();
                Ok(vec![])
            })(),
            "GetPointsForSite" => (|| {
                let id = text(args, 0)?;
                let t = self.tiles.as_ref().ok_or("GetPointsForSite before ConvertToTileMap")?;
                let (xs, ys, ts) = points::points_for_site(&self.map, t, &id).ok_or_else(|| format!("unknown site {id}"))?;
                Ok(vec![nums(xs), nums(ys), nums(ts)])
            })(),
            "ReserveSpace" => (|| {
                let id = text(args, 0)?;
                let size = num(args, 1)? as f32;
                let (start, fill, layout) = (lua_int(num(args, 2)?), lua_int(num(args, 3)?), lua_int(num(args, 4)?));
                let mask: Vec<i32> = match args.last() {
                    Some(Value::Arr(a)) => a.iter().map(|v| v.as_num().map(lua_int).unwrap_or(0)).collect(),
                    _ => vec![],
                };
                // arg 6 is a float `extra` only when it is a number (0x004A9370).
                let extra = opt_num(args, 5).map(|e| e as f32).unwrap_or(0.0);
                let t = self.tiles.as_mut().ok_or("ReserveSpace before ConvertToTileMap")?;
                Ok(match points::reserve_space(&self.map, t, &id, size, extra, start, fill, layout, &mask) {
                    Some((x, y)) => vec![Value::Num(x), Value::Num(y)],
                    None => vec![Value::Nil, Value::Nil],
                })
            })(),
            "DetectDisconnect" => (|| {
                let t = self.tiles.as_mut().ok_or("DetectDisconnect before ConvertToTileMap")?;
                Ok(vec![Value::Num(islands::detect_disconnect(&self.map, t)? as f64)])
            })(),
            // RunMaze(type, val, nodes) (0x4AB4A0) / GetPointsForMetaMaze(nodes)
            // (0x4AB830): xs, ys, types, or nothing when no point came out.
            "RunMaze" | "GetPointsForMetaMaze" => (|| {
                let (kind, val, tbl) = if method == "RunMaze" {
                    (lua_int(num(args, 0)?), lua_int(num(args, 1)?), args.get(2))
                } else {
                    (0, 0, args.first())
                };
                let names: Vec<&Value> = match tbl {
                    Some(Value::Arr(a)) => a.iter().collect(),
                    Some(Value::Map(e)) => e.iter().map(|(_, v)| v).collect(),
                    _ => vec![],
                };
                let mut nodes = Vec::new();
                for v in names {
                    let name = match v {
                        Value::Str(s) => String::from_utf8_lossy(s).into_owned(),
                        Value::Num(n) => format!("{n}"),
                        _ => continue,
                    };
                    if let Some(n) = self.map.node(&name) {
                        nodes.push(n);
                    }
                }
                let t = self.tiles.as_mut().ok_or("RunMaze before ConvertToTileMap")?;
                let out = maze::run(&self.map, t, &nodes, kind, val)?;
                if out.is_empty() {
                    return Ok(vec![]);
                }
                let (mut xs, mut ys, mut ts) = (Vec::new(), Vec::new(), Vec::new());
                for (k, pts) in &out {
                    for p in pts {
                        xs.push(p.0.trunc() as i32 as f64);
                        ys.push(p.1.trunc() as i32 as f64);
                        ts.push(*k as f64);
                    }
                }
                Ok(vec![nums(xs), nums(ys), nums(ts)])
            })(),
            // DrawCellLine(a, b, mode, tile) (0x4A5C60): Line between the two
            // centroids (mode 0) or sites (otherwise).
            "DrawCellLine" => (|| {
                let (a, b) = (text(args, 0)?, text(args, 1)?);
                let (mode, tile) = (lua_int(num(args, 2)?), lua_int(num(args, 3)?));
                let (Some(na), Some(nb)) = (self.map.node(&a), self.map.node(&b)) else { return Ok(vec![]) };
                let pos = |n: usize| -> Result<(f64, f64), String> {
                    let node = &self.map.nodes[n];
                    if mode == 0 {
                        Ok((node.centroid.0 as f64, node.centroid.1 as f64))
                    } else {
                        let s = node.site.ok_or("DrawCellLine: node without a site (null Site* in the game)")?;
                        Ok((self.map.sites[s].x as f64, self.map.sites[s].y as f64))
                    }
                };
                let (pa, pb) = (pos(na)?, pos(nb)?);
                let t = self.tiles.as_mut().ok_or("DrawCellLine before ConvertToTileMap")?;
                islands::line(t, pa.0, pa.1, pb.0, pb.1, tile as u8, false, false);
                Ok(vec![])
            })(),
            // RunCA(id, iterations, seed_mode, n) (0x4A47C0); n := 1 unless mode 0.
            "RunCA" => (|| {
                let id = text(args, 0)?;
                let (it, mode, mut n) = (lua_int(num(args, 1)?), lua_int(num(args, 2)?), lua_int(num(args, 3)?));
                if mode != 0 {
                    n = 1;
                }
                let node = self.map.node(&id).ok_or_else(|| format!("RunCA: unknown node {id}"))?;
                let t = self.tiles.as_mut().ok_or("RunCA before ConvertToTileMap")?;
                ca::run_ca(&self.map, t, node, it, mode, n)?;
                Ok(vec![])
            })(),
            _ => return None,
        };
        Some(r)
    }

    fn tiles_op(&self, f: impl FnOnce(&TileGrid) -> Result<Vec<Value>, String>) -> Result<Vec<Value>, String> {
        f(self.tiles.as_ref().ok_or("tile map used before ConvertToTileMap")?)
    }

    fn tiles_mut(&mut self, f: impl FnOnce(&mut TileGrid) -> Result<Vec<Value>, String>) -> Result<Vec<Value>, String> {
        f(self.tiles.as_mut().ok_or("tile map used before ConvertToTileMap")?)
    }

    /// DeleteSpecifiedTiles (0x4A6330 → 0x637D10). Decisions read only the
    /// name grid and the island map; index arithmetic follows the game,
    /// including the row wrap at x = W-1 (indices past the grid count as
    /// unnamed and are never written).
    fn delete_specified_tiles(&mut self) {
        let Some(g) = self.tiles.as_mut() else { return };
        let (w, h) = (g.w as i64, g.h as i64);
        let n = (w * h) as usize;
        let region = |i: i64| -> Option<&String> {
            if i < 0 || i as usize >= n {
                return None;
            }
            let task = self.names[i as usize]?;
            self.island_regions.get(&self.task_name_list[task])
        };
        let mut writes = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let Some(r1) = region(w * y + x) else { continue };
                let (ymin, xmin) = ((y - 1).max(0), (x - 1).max(0));
                let (ymax, xmax) = (h.min(y + 1), w.min(x + 1));
                let mut deleted = false;
                for yy in ymin..=ymax {
                    for xx in xmin..=xmax {
                        let ni = w * yy + xx;
                        if let Some(r2) = region(ni) {
                            if r1 != r2 {
                                deleted = true;
                                writes.push(ni as usize);
                            }
                        }
                    }
                }
                if deleted {
                    writes.push((w * y + x) as usize);
                }
            }
        }
        for i in writes {
            g.cells[i][0] = 1;
        }
    }

    /// ConvertToTileMap (wrapper 0x4A73D0, core 0x63E9D0).
    fn convert_to_tile_map(&mut self, args: &[Value]) -> Result<Vec<Value>, String> {
        let size = num(args, 0)?;
        let cap = opt_num(args, 1);
        let (w, h) = self.map.convert_rescale(size, cap)?;
        let (w, h) = (w as usize, h as usize);
        let mut grid = TileGrid::new(w, h);
        self.names = vec![None; w * h];
        self.task_names.clear();
        self.task_name_list.clear();
        grid.fill(self.map.default_tile);
        for s in 0..self.map.sites.len() {
            let Some(n) = self.map.sites[s].node else { continue };
            let node = &self.map.nodes[n];
            if node.kind == 3 || node.kind == 1 || node.parent.is_none() {
                continue;
            }
            let name = self.map.sites[s].name.clone();
            let pts = self.map.site_points(&name).unwrap_or_default();
            let task = name.split(':').next().unwrap_or("").to_string();
            let next = self.task_names.len();
            let key = *self.task_names.entry(task.clone()).or_insert(next);
            if key == next {
                self.task_name_list.push(task);
            }
            let tile = if node.value == -1 { self.map.default_tile as i32 } else { node.value };
            grid.stamp(&pts, tile, key, &mut self.names);
        }
        self.tiles = Some(grid);
        Ok(vec![])
    }

    fn with_node(&self, args: &[Value], f: impl Fn(&BoostMap, usize) -> Option<Vec<Value>>) -> Result<Vec<Value>, String> {
        let id = text(args, 0)?;
        Ok(self.map.node(&id).and_then(|n| f(&self.map, n)).unwrap_or_default())
    }

    fn add_child(&mut self, args: &[Value]) -> Result<Vec<Value>, String> {
        let parent = text(args, 0)?;
        let id = text(args, 1)?;
        // lua_tointeger: non-numbers (Shipwrecked islands pass nil) give 0.
        let value = opt_num(args, 2).map(lua_int).unwrap_or(0);
        let type_str = args.get(7).and_then(Value::as_bytes);
        let internal = opt_num(args, 8).map(lua_int);
        self.map.add_child(&parent, &id, value, type_str, internal);
        Ok(vec![])
    }
}

impl WorldSim for Native {
    fn call(&mut self, method: &str, args: &[Value]) -> Result<Vec<Value>, String> {
        match Native::call(self, method, args) {
            Some(r) => r,
            None => Err(format!("WorldSim:{method} is not implemented natively yet")),
        }
    }
}
