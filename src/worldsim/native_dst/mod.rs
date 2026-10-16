//! The Rust port of Don't Starve Together's native `WorldSimActual`
//! (x64 dedicated server), written as a delta against the DS port in
//! ../native/.
//!
//! Native randomness uses the shared PCG32 stream (crate::pcg): pcg::rand()
//! (pcg32_rand: reject raw < 4, % 32767) and pcg::boundedrand(n).
//! `call` returns `None` for methods not ported yet.

pub mod blend;
pub mod caves;
pub mod grid;
pub mod islands;
pub mod map;
pub mod maze;
pub mod roads;

use crate::pcg;
use crate::value::Value;
use crate::worldsim::WorldSim;
use grid::{tgm, Grid, Tiles, HI_RESERVED};
use map::DstMap;

pub struct NativeDst {
    pub map: DstMap,
    pub tiles: Option<Tiles>,
    /// WS+0x18 safe-from-disconnect grid (bit 0x1000).
    pub safe: Option<Grid>,
    /// WS+0x28 node-id grid.
    pub nodeid: Option<Grid>,
    /// WS+0x3C / +0x40 registry refs (Lua functions).
    pub resolve_noise: Option<Value>,
    pub validate_ground: Option<Value>,
    /// WS+0x44 points-barren-or-reserved tile.
    pub barren: u16,
    pub road_params: roads::RoadParams,
}

impl Default for NativeDst {
    fn default() -> Self {
        NativeDst {
            map: DstMap::new(),
            tiles: None,
            safe: None,
            nodeid: None,
            resolve_noise: None,
            validate_ground: None,
            barren: 0,
            road_params: Default::default(),
        }
    }
}

/// `cvttsd2si` (64-bit) keeping the low 32 bits: what `lua_tointeger` does.
pub fn toint(v: f64) -> i32 {
    if v.is_nan() || !(v > -9.223372036854775808e18 && v < 9.223372036854775807e18) {
        0
    } else {
        v as i64 as i32
    }
}

/// lua_tonumber: numbers and numeric strings, else 0.
fn num(args: &[Value], i: usize) -> f64 {
    match args.get(i) {
        Some(Value::Num(n)) => *n,
        Some(Value::Str(s)) => std::str::from_utf8(s).ok().and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.0),
        _ => 0.0,
    }
}

/// lua_tointeger.
fn int(args: &[Value], i: usize) -> i32 {
    toint(num(args, i))
}

/// lua_toboolean.
fn boolean(args: &[Value], i: usize) -> bool {
    !matches!(args.get(i), None | Some(Value::Nil) | Some(Value::Bool(false)))
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

fn table_values(v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::Arr(a)) => a.clone(),
        Some(Value::Map(m)) => m.iter().map(|(_, v)| v.clone()).collect(),
        _ => vec![],
    }
}

fn is_table(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Arr(_)) | Some(Value::Map(_)) | Some(Value::ZeroBased(_)))
}

/// A Lua callback returning one integer (luaL_checkinteger, kept as u16).
fn call_u16(f: &Option<Value>, args: &[Value]) -> Result<u16, String> {
    let f = f.as_ref().ok_or("native callback used before it was set")?;
    let r = crate::lua::call_function(f, args)?;
    match r.first() {
        Some(Value::Num(n)) => Ok(toint(*n) as u16),
        other => Err(format!("callback returned {other:?}")),
    }
}

impl NativeDst {
    pub fn new() -> NativeDst {
        NativeDst::default()
    }

    fn tiles(&self) -> Result<&Tiles, String> {
        self.tiles.as_ref().ok_or_else(|| "tile map used before ConvertToTileMap".to_string())
    }

    fn tiles_mut(&mut self) -> Result<&mut Tiles, String> {
        self.tiles.as_mut().ok_or_else(|| "tile map used before ConvertToTileMap".to_string())
    }

    /// `None`: not implemented (yet).
    pub fn call(&mut self, method: &str, args: &[Value]) -> Option<Result<Vec<Value>, String>> {
        let r = match method {
            "LuaPrint" => Ok(vec![]),
            "SetResolveNoiseFunction" => {
                self.resolve_noise = args.first().cloned();
                Ok(vec![])
            }
            "SetValidateGroundTileFunction" => {
                self.validate_ground = args.first().cloned();
                Ok(vec![])
            }
            "SetPointsBarrenOrReservedTile" => {
                self.barren = int(args, 0) as u16;
                Ok(vec![])
            }
            // ResetAll (0x14012a460): a new map object (257 x 257).
            "ResetAll" => {
                self.map = DstMap::new();
                Ok(vec![])
            }
            "SetImpassibleTileType" => Ok(vec![]),
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
            "SetSiteFlags" | "SetNodeType" | "ForceDisconectedCells" => (|| {
                let id = text(args, 0)?;
                if let Some(n) = self.map.node(&id) {
                    let node = &mut self.map.nodes[n];
                    match method {
                        "SetSiteFlags" => node.site_flags = int(args, 1),
                        "SetNodeType" => node.kind = int(args, 1),
                        _ => node.blocked = true,
                    }
                }
                Ok(vec![])
            })(),
            "SetWorldSize" => {
                let (w, h) = (int(args, 0), int(args, 1));
                self.map.set_world_size_core(w as u32, h as u32);
                Ok(vec![])
            }
            "GetWorldSize" => Ok(vec![Value::Num(toint(self.map.width) as f64), Value::Num(toint(self.map.height) as f64)]),
            "WorldGen_InitializeNodePoints" => {
                self.map.initialize_node_points();
                Ok(vec![])
            }
            "WorldGen_AddNewPositions" => self.map.add_new_positions().map(|_| vec![]),
            "WorldGen_VoronoiPass" => self.map.voronoi_pass(int(args, 0) as i64).map(|_| vec![]),
            "WorldGen_Commit" => self.map.commit().map(|ok| vec![Value::Bool(ok)]),
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
            "PointInSite" => (|| {
                let id = text(args, 0)?;
                let p = (num(args, 1) as f32, num(args, 2) as f32);
                let inside = self.map.node(&id).is_some_and(|n| map::point_in_poly(p, &self.map.nodes[n].poly));
                Ok(vec![Value::Bool(inside)])
            })(),
            "ConvertToTileMap" => self.convert_to_tile_map(args),
            "$perlin" => Ok(vec![Value::Num(crate::worldsim::native::tiles::perlin01(num(args, 0), num(args, 1), num(args, 2)))]),
            "GetTile" => self.tiles().map(|t| {
                let (x, y) = (int(args, 0), int(args, 1));
                let v = if t.t.in_range(x, y) { t.t.cells[y as usize * t.w() + x as usize] } else { tgm::INVALID_TILE };
                vec![Value::Num(v as f64)]
            }),
            "SetTile" => self.tiles_mut().map(|t| {
                let (x, y, tile, f) = (int(args, 0), int(args, 1), int(args, 2), int(args, 3));
                if t.t.in_range(x, y) {
                    let i = y as usize * t.w() + x as usize;
                    t.t.cells[i] = tile as u16;
                    let hi = (t.d.hi(i) & 0xBF) | (((f & 1) as u8) << 6);
                    t.d.set_hi(i, hi);
                }
                vec![]
            }),
            "ReserveTile" => self.tiles_mut().map(|t| {
                let (x, y) = (int(args, 0), int(args, 1));
                let mut ok = false;
                if t.d.in_range(x, y) {
                    let i = y as usize * t.d.w + x as usize;
                    if t.d.hi(i) & HI_RESERVED == 0 {
                        let hi = t.d.hi(i) | HI_RESERVED;
                        t.d.set_hi(i, hi);
                        ok = true;
                    }
                }
                vec![Value::Bool(ok)]
            }),
            "IsTileReserved" => self.tiles().map(|t| {
                let (x, y) = (int(args, 0), int(args, 1));
                let r = !t.d.in_range(x, y) || t.d.hi(y as usize * t.d.w + x as usize) & HI_RESERVED != 0;
                vec![Value::Bool(r)]
            }),
            "GetVisualTileAtPosition" => {
                self.tiles().and_then(|t| grid::visual_tile(t, num(args, 0), num(args, 1))).map(|v| vec![Value::Num(v as f64)])
            }
            "SetTileNodeId" => {
                let (x, y, id) = (int(args, 0), int(args, 1), int(args, 2));
                if let Some(g) = self.nodeid.as_mut() {
                    if g.in_range(x, y) {
                        let w = g.w;
                        g.cells[y as usize * w + x as usize] = id as u16;
                    }
                }
                Ok(vec![])
            }
            "CreateNodeIdTileMap" => self.create_node_id_tile_map(args),
            "ReplaceSingleNonLandTiles" => self.tiles_mut().map(|t| {
                grid::replace_single_non_land(&mut t.t);
                vec![]
            }),
            "GetEncodedMap" => self.get_encoded_map(),
            "GenerateBlendedMap" => self.generate_blended_map(args),
            // ocean_gen.lua GetRandomWaterPoints' get_points after its
            // math.random calls, with its checkTile(x, y, populating_tile).
            // (adj_width, adj_height, start_x, start_y, edge_dist, need, inc, tile)
            "DswOceanGetPoints" => self.tiles().map(|t| {
                let tile = num(args, 7);
                let ok = |x: f64, y: f64| {
                    let (ix, iy) = (toint(x), toint(y));
                    let reserved = !t.d.in_range(ix, iy) || t.d.hi(iy as usize * t.d.w + ix as usize) & HI_RESERVED != 0;
                    !reserved && crate::worldsim::helpers::Grid::get(t, x, y) == tile
                };
                let r = crate::worldsim::helpers::get_points(&ok, num(args, 0), num(args, 1), num(args, 2), num(args, 3), num(args, 4), num(args, 5), num(args, 6));
                vec![Value::Arr(r.into_iter().map(Value::Num).collect())]
            }),
            "GetPointsForSite" => (|| {
                let id = text(args, 0)?;
                let t = self.tiles.as_ref().ok_or("GetPointsForSite before ConvertToTileMap")?;
                let n = self.map.node(&id).ok_or_else(|| format!("GetPointsForSite: unknown site {id}"))?;
                let mut pts = self.map.node_points(n).unwrap_or_default();
                grid::random_shuffle(&mut pts);
                let (xs, ys, ts) = grid::push_points(&self.map, t, &pts, self.barren);
                Ok(vec![nums(xs), nums(ys), nums(ts)])
            })(),
            "GetRandomPointsForSite" => (|| {
                let id = text(args, 0)?;
                let count = int(args, 1) as i64;
                let n = self.map.node(&id).ok_or_else(|| format!("GetRandomPointsForSite: unknown site {id}"))?;
                let mut pts = grid::random_points(&self.map, self.tiles.as_ref(), n, count);
                grid::random_shuffle(&mut pts);
                let t = self.tiles.as_ref().ok_or("GetRandomPointsForSite before ConvertToTileMap")?;
                let (xs, ys, ts) = grid::push_points(&self.map, t, &pts, self.barren);
                Ok(vec![nums(xs), nums(ys), nums(ts)])
            })(),
            "ReserveSpace" => (|| {
                let id = text(args, 0)?;
                let size = num(args, 1) as f32;
                let (start, fill, layout) = (int(args, 2), int(args, 3), int(args, 4));
                let mask: Vec<u16> = match args.last() {
                    Some(v @ (Value::Arr(_) | Value::Map(_))) => {
                        table_values(Some(v)).iter().map(|v| toint(v.as_num().unwrap_or(0.0)) as u16).collect()
                    }
                    _ => vec![],
                };
                let t = self.tiles.as_mut().ok_or("ReserveSpace before ConvertToTileMap")?;
                Ok(match grid::reserve_space(&self.map, t, &id, size, start, fill, layout, &mask) {
                    Some((x, y)) => vec![Value::Num(x as f64), Value::Num(y as f64)],
                    None => vec![Value::Nil, Value::Nil],
                })
            })(),
            "SeparateIslands" => (|| {
                let t = self.tiles.as_mut().ok_or("SeparateIslands before ConvertToTileMap")?;
                let vg = &self.validate_ground;
                islands::separate_islands(&mut self.map, t, &mut |v| call_u16(vg, &[Value::Num(v as f64)]))?;
                Ok(vec![])
            })(),
            "ForceConnectivity" => (|| {
                let join = boolean(args, 0);
                let is_cave = boolean(args, 1);
                let tile = int(args, 2) as u16;
                islands::generate_landmasses(&mut self.map);
                islands::merge_landmasses(&mut self.map, join);
                let t = self.tiles.as_mut().ok_or("ForceConnectivity before ConvertToTileMap")?;
                let vg = &self.validate_ground;
                islands::force_connectivity_core(&mut self.map, t, &mut |v| call_u16(vg, &[Value::Num(v as f64)]), tile, is_cave)?;
                Ok(vec![])
            })(),
            "SetRoadParameters" => {
                self.road_params = roads::RoadParams {
                    nsub: int(args, 0),
                    min_w: num(args, 1) as f32,
                    max_w: num(args, 2) as f32,
                    min_e: num(args, 3) as f32,
                    max_e: num(args, 4) as f32,
                    jitter: num(args, 5) as f32,
                };
                Ok(vec![])
            }
            "DrawRoads" => (|| {
                let join = boolean(args, 0);
                let tile = int(args, 1) as u16;
                let t = self.tiles.as_mut().ok_or("DrawRoads before ConvertToTileMap")?;
                roads::draw_roads(&mut self.map, t, &self.road_params, join, tile)?;
                Ok(vec![])
            })(),
            "GetRoad" => (|| {
                let idx = int(args, 0);
                let join = boolean(args, 1);
                let rs = roads::get_roads(&mut self.map, join)?;
                let mut i = idx as u32 as usize;
                if i > rs.len() {
                    i = 0;
                }
                let r = rs.get(i).cloned().unwrap_or_default();
                Ok(vec![
                    Value::Num(rs.len() as f64),
                    Value::Num(1.0),
                    nums(r.iter().map(|p| p.0 as f64)),
                    nums(r.iter().map(|p| p.1 as f64)),
                ])
            })(),
            "MakeSafeFromDisconnect" => (|| {
                let t = self.tiles.as_ref().ok_or("MakeSafeFromDisconnect before ConvertToTileMap")?;
                islands::make_safe(&self.map, t, &mut self.safe, int(args, 0), int(args, 1));
                Ok(vec![])
            })(),
            "ClearSafeFromDisconnect" => {
                self.safe = None;
                Ok(vec![])
            }
            "DetectDisconnect" => (|| {
                let t = self.tiles.as_mut().ok_or("DetectDisconnect before ConvertToTileMap")?;
                let n = islands::detect_disconnect(&self.map, t, self.safe.as_ref())?;
                Ok(vec![Value::Num(n as f64)])
            })(),
            "GetWormholesExtra" => (|| {
                let t = self.tiles.as_mut().ok_or("GetWormholesExtra before ConvertToTileMap")?;
                Ok(match islands::get_wormholes_extra(&self.map, t) {
                    None => vec![Value::Nil, Value::Nil, Value::Nil, Value::Nil],
                    Some(arrs) => arrs
                        .into_iter()
                        .map(|a| Value::Arr(a.into_iter().map(|v| v.map(Value::Num).unwrap_or(Value::Nil)).collect()))
                        .collect(),
                })
            })(),
            "GetWormholes" => (|| {
                let (a, b) = (text(args, 0)?, text(args, 1)?);
                let t = self.tiles.as_mut().ok_or("GetWormholes before ConvertToTileMap")?;
                let pt = |p: Option<(f32, f32)>| match p {
                    Some((x, y)) => [Value::Num(x as f64), Value::Num(y as f64)],
                    None => [Value::Nil, Value::Nil],
                };
                Ok(match islands::get_wormholes(&mut self.map, t, &a, &b) {
                    None => vec![Value::Nil, Value::Nil, Value::Nil, Value::Nil],
                    Some([p, q]) => pt(p).into_iter().chain(pt(q)).collect(),
                })
            })(),
            // GenerateSessionIdentifier (0x14012b100): not seed-determined
            // (QPC time); consumes two pcg32_rand calls.
            "GenerateSessionIdentifier" => {
                let a = pcg::rand();
                let b = pcg::rand();
                let hi = (a << 16) ^ b;
                Ok(vec![Value::str(&format!("{hi:08X}{:08X}", 0))])
            }
            "DrawCellLine" => (|| {
                let (a, b) = (text(args, 0)?, text(args, 1)?);
                let (mode, tile) = (int(args, 2), int(args, 3) as u16);
                let (set80, only_invalid) = (boolean(args, 4), boolean(args, 5));
                let (Some(na), Some(nb)) = (self.map.node(&a), self.map.node(&b)) else { return Ok(vec![]) };
                let end = |n: usize| -> Option<(f32, f32)> {
                    if mode == 0 {
                        Some(self.map.nodes[n].centroid)
                    } else {
                        self.map.nodes[n].site.map(|s| (self.map.sites[s].x, self.map.sites[s].y))
                    }
                };
                let (Some(pa), Some(pb)) = (end(na), end(nb)) else { return Err("DrawCellLine: node without a site".into()) };
                let t = self.tiles.as_mut().ok_or("DrawCellLine before ConvertToTileMap")?;
                grid::line(t, pa.0, pa.1, pb.0, pb.1, tile, set80, only_invalid);
                Ok(vec![])
            })(),
            "DrawGroundLine" => (|| {
                let (x0, y0, x1, y1) = (num(args, 0) as f32, num(args, 1) as f32, num(args, 2) as f32, num(args, 3) as f32);
                let tile = int(args, 4) as u16;
                let (set80, only_invalid) = (boolean(args, 5), boolean(args, 6));
                let t = self.tiles.as_mut().ok_or("DrawGroundLine before ConvertToTileMap")?;
                grid::line(t, x0, y0, x1, y1, tile, set80, only_invalid);
                Ok(vec![])
            })(),
            "RunCA" => (|| {
                let id = text(args, 0)?;
                let (iters, mode, npts) = (int(args, 1), int(args, 2), int(args, 3));
                let t = self.tiles.as_mut().ok_or("RunCA before ConvertToTileMap")?;
                caves::run_ca(&self.map, t, &id, iters, mode, npts)?;
                Ok(vec![])
            })(),
            // RunMaze(type, val, impassable, floor, nodes) 0x14012f320.
            "RunMaze" => (|| {
                let (kind, val, imp, floor) = (int(args, 0), int(args, 1), int(args, 2) as u16, int(args, 3) as u16);
                let nodes = self.maze_nodes(args);
                if nodes.is_empty() {
                    return Ok(vec![]);
                }
                let m = maze::Maze::new(&self.map, &nodes, kind, imp, floor, val, 8)?;
                let res = m.run(self.tiles.as_mut())?;
                Ok(maze_values(&res))
            })(),
            // GetPointsForMetaMaze(size, nodes) 0x14012f7e0: type 0, both tiles
            // the default impassable tile, size 8 when arg 1 is the table.
            "GetPointsForMetaMaze" => (|| {
                let size = if is_table(args.first()) { 8 } else { int(args, 0) };
                let nodes = self.maze_nodes(args);
                if nodes.is_empty() {
                    return Ok(vec![]);
                }
                let d = tgm::DEFAULT_IMPASSABLE;
                let m = maze::Maze::new(&self.map, &nodes, 0, d, d, 0, size)?;
                let res = m.run(self.tiles.as_mut())?;
                Ok(maze_values(&res))
            })(),
            _ => return None,
        };
        Some(r)
    }

    /// The node table at the stack top, in lua_next order; unknown ids skipped.
    fn maze_nodes(&self, args: &[Value]) -> Vec<usize> {
        if !is_table(args.last()) {
            return vec![];
        }
        table_values(args.last())
            .iter()
            .filter_map(|v| match v {
                Value::Str(s) => self.map.node(&String::from_utf8_lossy(s)),
                _ => None,
            })
            .collect()
    }

    fn with_node(&self, args: &[Value], f: impl Fn(&DstMap, usize) -> Option<Vec<Value>>) -> Result<Vec<Value>, String> {
        let id = text(args, 0)?;
        Ok(self.map.node(&id).and_then(|n| f(&self.map, n)).unwrap_or_default())
    }

    /// AddChild (0x14012b1a0): arg 8 is the integer NODE_TYPE (1..7, else 0).
    fn add_child(&mut self, args: &[Value]) -> Result<Vec<Value>, String> {
        let parent = text(args, 0)?;
        let id = text(args, 1)?;
        let value = int(args, 2) as u16;
        let kind = if args.len() >= 8 {
            match int(args, 7) {
                t @ 1..=7 => t,
                _ => 0,
            }
        } else {
            0
        };
        let internal = if args.len() >= 9 { int(args, 8) } else { 0 };
        self.map.add_child(&parent, &id, value, kind, internal);
        Ok(vec![])
    }

    /// ConvertToTileMap (0x14012b640): rescale, grids, fill, stamp.
    fn convert_to_tile_map(&mut self, args: &[Value]) -> Result<Vec<Value>, String> {
        self.map.default_tile = tgm::DEFAULT_IMPASSABLE;
        self.tiles = None;
        self.safe = None;
        let size = self.map.convert_rescale(num(args, 0))? as usize;
        let rn = &self.resolve_noise;
        let tiles = grid::convert_to_tile_map(&self.map, size, &mut |tile, noise| {
            call_u16(rn, &[Value::Num(tile as f64), Value::Num(noise)])
        })?;
        self.tiles = Some(tiles);
        Ok(vec![])
    }

    /// CreateNodeIdTileMap (0x14012c200).
    fn create_node_id_tile_map(&mut self, args: &[Value]) -> Result<Vec<Value>, String> {
        let (w, h) = (toint(self.map.width) as usize, toint(self.map.height) as usize);
        let mut g = Grid::new(w, h);
        g.cells.iter_mut().for_each(|c| *c = 0);
        if is_table(args.last()) {
            let mut idx: u16 = 1;
            for v in table_values(args.last()) {
                let s = match &v {
                    Value::Str(s) => String::from_utf8_lossy(s).into_owned(),
                    Value::Num(n) => format!("{n}"),
                    _ => continue,
                };
                if s.contains("LOOP_BLANK_SUB") {
                    continue;
                }
                if let Some(pts) = self.map.site_points(&s) {
                    for p in pts {
                        let i = (p.1 as i64 * w as i64 + p.0 as i64) as usize;
                        if i < g.cells.len() {
                            g.cells[i] = idx;
                        }
                    }
                }
                idx = idx.wrapping_add(1);
            }
        }
        self.nodeid = Some(g);
        Ok(vec![])
    }

    /// GetEncodedMap (0x14012e6f0): tiles, tiledata, nav, adj, nodeidtilemap.
    fn get_encoded_map(&mut self) -> Result<Vec<Value>, String> {
        let Some(t) = self.tiles.as_mut() else { return Ok(vec![Value::str("")]) };
        grid::replace_single_non_land(&mut t.t);
        let tiles = t.t.serialize();
        let tiledata = t.d.serialize();
        let (w, h) = (toint(self.map.width) as usize, toint(self.map.height) as usize);
        let mut nav = Grid::new(w, h);
        nav.cells.iter_mut().for_each(|c| *c = 0x7FFF);
        let mut cells: Vec<usize> = Vec::new();
        for l in &self.map.landmasses {
            for list in l.map.values() {
                for &c in list {
                    cells.push(c);
                    if let Some(pts) = self.map.node_points(c) {
                        for (px, py) in pts {
                            let i = (py as i64 as usize) * w + px as i64 as usize;
                            if i < nav.cells.len() && i < t.t.cells.len() {
                                let v = t.t.cells[i];
                                if tgm::is_land(v) || tgm::is_ocean(v) {
                                    let hi = nav.hi(i) | 0x80;
                                    nav.set_hi(i, hi);
                                }
                            }
                        }
                    }
                }
            }
        }
        for x in 0..w {
            for y in 0..h {
                let i = y * w + x;
                let v = t.t.cells[i];
                if (tgm::is_land(v) || tgm::is_ocean(v)) && nav.hi(i) & 0x80 == 0 {
                    nav.cells[i] = 0xFFFF;
                }
            }
        }
        let nav_s = nav.serialize();
        let adj: Vec<Value> = cells
            .iter()
            .map(|&c| {
                let mut v = Vec::new();
                for &nb in &self.map.nodes[c].lm_neighbours {
                    for (j, &cj) in cells.iter().enumerate() {
                        if cj == nb {
                            v.push((j + 1) as f64);
                        }
                    }
                }
                nums(v)
            })
            .collect();
        let nodeid = self.nodeid.as_ref().ok_or("GetEncodedMap without CreateNodeIdTileMap (NULL in the game)")?.serialize();
        Ok(vec![Value::Str(tiles), Value::Str(tiledata), Value::Str(nav_s), Value::Arr(adj), Value::Str(nodeid)])
    }

    /// GenerateBlendedMap (0x14012bc80).
    fn generate_blended_map(&mut self, args: &[Value]) -> Result<Vec<Value>, String> {
        let t = self.tiles()?;
        let (w, h) = (t.w(), t.h());
        let k = int(args, 0);
        let sigma = num(args, 1) as f32;
        if !is_table(args.get(2)) {
            return Ok(vec![]);
        }
        let def = match args.get(3) {
            Some(Value::Num(d)) => *d as f32,
            Some(Value::Str(_)) => num(args, 3) as f32,
            _ => 0.0,
        };
        let mut lv: std::collections::HashMap<u16, f32> = Default::default();
        for e in table_values(args.get(2)) {
            if let Value::Arr(pair) = e {
                if pair.len() >= 2 {
                    let tile = toint(pair[0].as_num().unwrap_or(0.0)) as u16;
                    lv.insert(tile, pair[1].as_num().unwrap_or(0.0) as f32);
                }
            }
        }
        let buf: Vec<f32> = t.t.cells.iter().map(|c| lv.get(c).copied().unwrap_or(def)).collect();
        let out = blend::blur(&buf, w, h, k, sigma);
        let out: Vec<f64> = out.into_iter().map(|v| v as f64).collect();
        crate::worldsim::helpers::remember_blended(&out);
        Ok(vec![Value::ZeroBased(out), Value::Num(w as f64), Value::Num(h as f64)])
    }
}

/// xs, ys, types of a maze result map (key order, then insertion order);
/// no values when it is empty.
fn maze_values(res: &maze::MazeResult) -> Vec<Value> {
    if res.is_empty() {
        return vec![];
    }
    let (mut xs, mut ys, mut ts) = (Vec::new(), Vec::new(), Vec::new());
    for (&k, pts) in res {
        for p in pts {
            xs.push(p.0 as i64 as i32 as f64);
            ys.push(p.1 as i64 as i32 as f64);
            ts.push(k as f64);
        }
    }
    vec![nums(xs), nums(ys), nums(ts)]
}

impl WorldSim for NativeDst {
    fn call(&mut self, method: &str, args: &[Value]) -> Result<Vec<Value>, String> {
        match NativeDst::call(self, method, args) {
            Some(r) => r,
            None => Err(format!("WorldSim:{method} is not implemented natively for DST yet")),
        }
    }

    fn fast(&mut self, method: crate::worldsim::Fast, a: &[f64]) -> Option<crate::worldsim::FastOut> {
        use crate::worldsim::{helpers, Fast, FastOut};
        let n = |i: usize| a.get(i).copied().unwrap_or(0.0);
        Some(match method {
            Fast::GetTile => {
                let t = self.tiles.as_ref()?;
                let (x, y) = (toint(n(0)), toint(n(1)));
                FastOut::Num(if t.t.in_range(x, y) { t.t.cells[y as usize * t.w() + x as usize] } else { tgm::INVALID_TILE } as f64)
            }
            Fast::IsTileReserved => {
                let t = self.tiles.as_ref()?;
                let (x, y) = (toint(n(0)), toint(n(1)));
                FastOut::Bool(!t.d.in_range(x, y) || t.d.hi(y as usize * t.d.w + x as usize) & HI_RESERVED != 0)
            }
            Fast::SetTile => {
                let t = self.tiles.as_mut()?;
                let (x, y, tile, f) = (toint(n(0)), toint(n(1)), toint(n(2)), toint(n(3)));
                if t.t.in_range(x, y) {
                    let i = y as usize * t.w() + x as usize;
                    t.t.cells[i] = tile as u16;
                    let hi = (t.d.hi(i) & 0xBF) | (((f & 1) as u8) << 6);
                    t.d.set_hi(i, hi);
                }
                FastOut::Nothing
            }
            Fast::Perlin => FastOut::Num(crate::worldsim::native::tiles::perlin01(n(0), n(1), n(2))),
            // (mode 1, land range x4, width, height, tile, offset, depth)
            Fast::GroundFillPass if a.len() >= 10 && a[0] == 1.0 => {
                let lined = |g: f64| helpers::in_group(g, &a[1..5]);
                helpers::ground_fill_pass(self.tiles.as_mut()?, &lined, a[5], a[6], a[7], a[8], a[9]);
                FastOut::Nothing
            }
            // (mode 1, land x4, width, height, radius, OCEAN_COASTAL,
            // IMPASSABLE, impassable x4, ocean x4): IsCloseToImpassable or IsCloseToWater.
            Fast::SquareFillPass if a.len() >= 18 && a[0] == 1.0 => {
                let lined = |g: f64| helpers::in_group(g, &a[1..5]);
                let r = a[7];
                let close = |t: &Tiles, x: f64, y: f64| {
                    helpers::ring_any(x, y, r, |x, y| {
                        let g = helpers::Grid::get(t, x, y);
                        g == a[9] || helpers::in_group(g, &a[10..14])
                    }) || helpers::ring_any(x, y, r, |x, y| helpers::in_group(helpers::Grid::get(t, x, y), &a[14..18]))
                };
                helpers::square_fill_pass(self.tiles.as_mut()?, &lined, close, a[5], a[6], r, a[8]);
                FastOut::Nothing
            }
            // PopulateWaterType's distribute loop: the next x in x0..=xmax on row
            // y whose tile is `tile`, or nothing. (x0, y, xmax, tile)
            Fast::NextTileEq if a.len() >= 4 => {
                let t = self.tiles.as_ref()?;
                let (mut x, y, xmax, tile) = (a[0], a[1], a[2], a[3]);
                while x <= xmax {
                    if helpers::Grid::get(t, x, y) == tile {
                        return Some(FastOut::Num(x));
                    }
                    x = x + 1.0;
                }
                FastOut::Nothing
            }
            // AddShoreline: (width, height, ocean x4, IMPASSABLE, impassable x4,
            // INVALID, OCEAN_BRINEPOOL, OCEAN_COASTAL_SHORE), with ocean_gen's
            // IsSurroundedByWaterOrInvalid(x, y, 1).
            Fast::OceanAddShoreline if a.len() >= 14 => {
                let t = self.tiles.as_mut()?;
                let (width, height) = (a[0], a[1]);
                let ok = |g: f64| helpers::in_group(g, &a[2..6]) || g == a[6] || helpers::in_group(g, &a[7..11]) || g == a[11];
                let mut y = 0.0;
                while y <= height - 1.0 {
                    let mut x = 0.0;
                    while x <= width - 1.0 {
                        let ground = helpers::Grid::get(t, x, y);
                        let tt: &Tiles = t;
                        if helpers::in_group(ground, &a[2..6]) && helpers::ring_any(x, y, 1.0, |x, y| !ok(helpers::Grid::get(tt, x, y))) && ground != a[12] {
                            helpers::Grid::set(t, x, y, a[13]);
                        }
                        x += 1.0;
                    }
                    y += 1.0;
                }
                FastOut::Nothing
            }
            Fast::NoisePass if a.len() >= 4 => {
                helpers::noise_pass(self.tiles.as_mut()?, a)?;
                FastOut::Nothing
            }
            // ocean_gen.lua do_blend's loop over (cm, g, el): (width, height,
            // mindist, maxdist, final shallow, medium, coral, grave, ocean range
            // x4, IMPASSABLE, OCEAN_WATERLOG, OCEAN_BRINEPOOL, OCEAN_COASTAL,
            // OCEAN_SWELL, OCEAN_HAZARDOUS, OCEAN_ROUGH).
            Fast::OceanBlendPass if a.len() >= 19 => {
                let t = self.tiles.as_mut()?;
                let (width, height) = (a[0], a[1]);
                let [cm, gm, el] = helpers::last_blended((width * height) as usize)?;
                let mut y = 0.0;
                while y <= height - 1.0 {
                    let mut x = 0.0;
                    while x <= width - 1.0 {
                        let tile = helpers::Grid::get(t, x, y);
                        if helpers::in_group(tile, &a[8..12]) || tile == a[12] {
                            let falloff = helpers::edge_falloff(x, y, width, height, a[2], a[3], 0.0, 1.0);
                            let i = (y * width + x) as usize;
                            let (ellevel, cmlevel, glevel) = (el[i], cm[i] * falloff, gm[i] * falloff);
                            let to = if ellevel > a[4] {
                                if tile == a[13] {
                                    a[13]
                                } else if cmlevel > a[6] && tile == a[14] {
                                    a[14]
                                } else {
                                    a[15]
                                }
                            } else if ellevel > a[5] {
                                a[16]
                            } else if glevel > a[7] {
                                a[17]
                            } else {
                                a[18]
                            };
                            helpers::Grid::set(t, x, y, to);
                        }
                        x += 1.0;
                    }
                    y += 1.0;
                }
                FastOut::Nothing
            }
            Fast::SimplexNoise2d if a.len() >= 4 => FastOut::Num(helpers::simplex_noise_2d(a[0], a[1], a[2], a[3])),
            // The helpers need every argument (lua_patch.rs always passes them).
            _ if a.len() < helpers::arity(method) => return None,
            Fast::OceanFillGroundType => {
                let lined = |g: f64| helpers::in_group(g, &a[7..11]);
                helpers::fill(self.tiles.as_mut()?, &lined, a[0], a[1], a[2], a[3], a[4], a[5], a[6]);
                FastOut::Nothing
            }
            Fast::OceanPlaceGroundType => {
                let lined = |g: f64| helpers::in_group(g, &a[8..12]);
                let (x, y) = helpers::place(self.tiles.as_mut()?, &lined, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], None);
                FastOut::Num2(x, y)
            }
            Fast::OceanPlaceFilledGroundType => {
                let lined = |g: f64| helpers::in_group(g, &a[10..14]);
                let (x, y) = helpers::place(self.tiles.as_mut()?, &lined, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], Some((a[8], a[9])));
                FastOut::Num2(x, y)
            }
            Fast::OceanSquareFill => {
                let lined = |g: f64| helpers::in_group(g, &a[6..10]);
                helpers::square_fill(self.tiles.as_mut()?, &lined, a[0], a[1], a[2], a[3], a[4], a[5]);
                FastOut::Nothing
            }
            // (x, y, radius, group range x4)
            Fast::OceanIsCloseToGroup => {
                let t = self.tiles.as_ref()?;
                FastOut::Bool(helpers::ring_any(a[0], a[1], a[2], |x, y| helpers::in_group(helpers::Grid::get(t, x, y), &a[3..7])))
            }
            // (x, y, radius, IMPASSABLE, impassable range x4)
            Fast::OceanIsCloseToImpassable => {
                let t = self.tiles.as_ref()?;
                let imp = |g: f64| g == a[3] || helpers::in_group(g, &a[4..8]);
                FastOut::Bool(helpers::ring_any(a[0], a[1], a[2], |x, y| imp(helpers::Grid::get(t, x, y))))
            }
            // (x, y, radius, ocean range x4, IMPASSABLE, impassable range x4, INVALID)
            Fast::OceanIsSurroundedByWaterOrInvalid => {
                let t = self.tiles.as_ref()?;
                let ok = |g: f64| helpers::in_group(g, &a[3..7]) || g == a[7] || helpers::in_group(g, &a[8..12]) || g == a[12];
                FastOut::Bool(!helpers::ring_any(a[0], a[1], a[2], |x, y| !ok(helpers::Grid::get(t, x, y))))
            }
            _ => return None,
        })
    }
}

/// WorldSim:GetTile / SetTile (three arguments) for the native helpers.
impl crate::worldsim::helpers::Grid for Tiles {
    fn get(&self, x: f64, y: f64) -> f64 {
        let (x, y) = (toint(x), toint(y));
        (if self.t.in_range(x, y) { self.t.cells[y as usize * self.w() + x as usize] } else { tgm::INVALID_TILE }) as f64
    }
    fn set(&mut self, x: f64, y: f64, ground: f64) {
        let (x, y, tile) = (toint(x), toint(y), toint(ground));
        if self.t.in_range(x, y) {
            let i = y as usize * self.w() + x as usize;
            self.t.cells[i] = tile as u16;
            let hi = self.d.hi(i) & 0xBF;
            self.d.set_hi(i, hi);
        }
    }
}
