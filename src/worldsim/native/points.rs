//! Per-site points and reservations (GetPointsForSite, ReserveSpace,
//! ReserveTile, GetVisualTileAtPosition) and the tile accessors.

use crate::crt;
use crate::worldsim::native::boostmap::{BoostMap, P2};
use crate::worldsim::native::tiles::{is_shore, is_water, TileGrid, FLAG_HIGH, FLAG_RESERVED, FLAG_ROAD};

/// MSVC 2008 std::_Random_shuffle (0x004A7340): n-1 rand() calls for
/// n <= 32769.
pub fn random_shuffle<T>(a: &mut [T]) {
    let n = a.len();
    if n < 2 {
        return;
    }
    let mut idx: u32 = 2;
    for i in 1..n {
        let mut rm: u32 = 0x7FFF;
        let mut rn: u32 = crt::rand() & 0x7FFF;
        while rm < idx && rm != 0xFFFF_FFFF {
            rn = (rn << 15) | (crt::rand() & 0x7FFF);
            rm = (rm << 15) | 0x7FFF;
        }
        a.swap(i, (rn % idx) as usize);
        idx += 1;
    }
}

/// GetPointsForSite (0x004A9760 → push helper 0x004A8790): xs, ys, types.
pub fn points_for_site(map: &BoostMap, tiles: &TileGrid, id: &str) -> Option<(Vec<f64>, Vec<f64>, Vec<f64>)> {
    let mut pts = map.site_points(id)?;
    random_shuffle(&mut pts);
    Some(push_points(map, tiles, &pts))
}

fn push_points(map: &BoostMap, tiles: &TileGrid, pts: &[P2]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let (w, h) = (map.width.trunc() as i32, map.height.trunc() as i32);
    let (mut xs, mut ys, mut types) = (Vec::new(), Vec::new(), Vec::new());
    for &(px, py) in pts {
        if !(px >= 0.0 && (px as f64) < w as f64 && py >= 0.0 && (py as f64) < h as f64) {
            continue;
        }
        let i = (py.trunc() as i32 as usize) * tiles.w + (px.trunc() as i32 as usize);
        let c = tiles.cells[i];
        if c[1] & FLAG_RESERVED != 0 || c[1] & FLAG_HIGH != 0 || c[0] == 0xFF || c[0] == 1 {
            continue;
        }
        xs.push(px.trunc() as i32 as f64);
        ys.push(py.trunc() as i32 as f64);
        let t = if c[1] & (FLAG_ROAD | FLAG_RESERVED | FLAG_HIGH) != 0 { 2 } else { c[0] };
        types.push(t as f64);
    }
    (xs, ys, types)
}

fn in_range(tiles: &TileGrid, x: i32, y: i32) -> bool {
    x > 0 && (x as usize) < tiles.w && y > 0 && (y as usize) < tiles.h
}

/// ReserveTile (0x004A5B60); index 0 counts as out of range.
pub fn reserve_tile(tiles: &mut TileGrid, x: i32, y: i32) -> bool {
    if !in_range(tiles, x, y) {
        return false;
    }
    let i = tiles.idx(x as usize, y as usize);
    let c = &mut tiles.cells[i];
    if c[1] & FLAG_RESERVED != 0 {
        return false;
    }
    c[1] |= FLAG_RESERVED;
    c[1] &= 0x7F;
    true
}

pub fn is_tile_reserved(tiles: &TileGrid, x: i32, y: i32) -> bool {
    in_range(tiles, x, y) && tiles.cells[tiles.idx(x as usize, y as usize)][1] & FLAG_RESERVED != 0
}

pub fn get_tile(tiles: &TileGrid, x: i32, y: i32) -> i32 {
    if in_range(tiles, x, y) { tiles.cells[tiles.idx(x as usize, y as usize)][0] as i32 } else { -1 }
}

pub fn set_tile(tiles: &mut TileGrid, x: i32, y: i32, tile: i32, flag: i32) {
    if in_range(tiles, x, y) {
        let i = tiles.idx(x as usize, y as usize);
        let c = &mut tiles.cells[i];
        c[0] = tile as u8;
        c[1] ^= (c[1] ^ ((flag << 6) as u8)) & 0x40;
    }
}

/// GetVisualTileAtPosition (0x004A5690).
pub fn visual_tile(tiles: &TileGrid, x: f64, y: f64) -> i32 {
    let (fx, fy) = (x as f32, y as f32);
    let xx = fx as f64 + 0.5;
    let yy = 0.5 + fy as f64;
    let (ix, iy) = (xx.trunc() as i32, yy.trunc() as i32);
    let i = (iy as i64 * tiles.w as i64 + ix as i64) as usize;
    let c = tiles.cells[i];
    let mut t = c[0] as i32;
    if c[1] & FLAG_HIGH != 0 {
        t = 1;
    } else if t != 1 {
        return t;
    }
    let frx = (xx - ix as u32 as f64) as f32;
    let fry = (yy - iy as u32 as f64) as f32;
    let lo = 0.24f32 as f64; // 0.23999999463558197 (double @0x68C9D8)
    let step = |f: f32| -> i64 { if (f as f64) < lo { -1 } else if f > 0.76f32 { 1 } else { 0 } };
    let (dx, dy) = (step(frx), step(fry));
    let j = ((iy as i64 + dy) * tiles.w as i64 + (ix as i64 + dx)) as usize;
    let t2 = tiles.cells[j][0] as i32;
    if t2 > 1 { t2 } else { t }
}

/// ReserveSpace (0x004A9370 → 0x00638840 → BoostMap::ReserveSpace 0x006381B0).
pub fn reserve_space(
    map: &BoostMap,
    tiles: &mut TileGrid,
    id: &str,
    size: f32,
    extra: f32,
    start_mask: i32,
    fill_mask: i32,
    layout_position: i32,
    mask: &[i32],
) -> Option<(f64, f64)> {
    let pts = map.site_points(id)?;
    let mut cand: Vec<P2> = if layout_position == 1 {
        let n = map.node(id)?;
        let s = &map.sites[map.nodes[n].site?];
        pts.iter()
            .find(|p| p.0.trunc() as i32 == s.x.trunc() as i32 && p.1.trunc() as i32 == s.y.trunc() as i32)
            .map(|p| vec![*p])
            .unwrap_or_default()
    } else {
        pts
    };
    random_shuffle(&mut cand);
    let (w, h) = (tiles.w as i32, tiles.h as i32);
    // floor((float)((float)(P - R) + 0.5)): float subtraction, double add,
    // stored as float, then MSVCR90 floor.
    let fl = |v: f64| (v as f32 as f64).floor() as i32;
    for p in cand {
        let id0 = tiles.cells[(p.1.trunc() as i32 * w + p.0.trunc() as i32) as usize][0];
        if start_mask & 1 == 0 && (id0 == 0xFF || id0 == 1 || id0 == 0x32) {
            continue;
        }
        if matches!(id0, 0x37 | 0x38 | 0x39 | 0x3B | 0x3E | 0x30 | 0x34 | 0x40) || is_shore(id0) {
            continue;
        }
        let r = (size as f64 + extra as f64) as f32;
        let x0 = fl(((p.0 as f64 - r as f64) as f32) as f64 + 0.5);
        let x1 = fl(((p.0 as f64 + r as f64) as f32) as f64 + 0.5);
        let y0 = fl(((p.1 as f64 - r as f64) as f32) as f64 + 0.5);
        let y1 = fl(((p.1 as f64 + r as f64) as f32) as f64 + 0.5);
        if !(x0 > 0 && x1 < w - 1 && y0 > 0 && y1 < h - 1) {
            continue;
        }
        let mut bad = false;
        'check: for x in x0..x1 {
            for y in y0..y1 {
                let c = tiles.cells[(y * w + x) as usize];
                if fill_mask & 1 == 0 && (c[0] == 0xFF || c[0] == 1 || c[0] == 0x32 || c[0] == map.default_tile) {
                    bad = true;
                    break 'check;
                }
                if fill_mask & 2 == 0 && c[1] & FLAG_ROAD != 0 {
                    bad = true;
                    break 'check;
                }
                if fill_mask & 4 == 0 && c[1] & FLAG_RESERVED != 0 {
                    bad = true;
                    break 'check;
                }
                if fill_mask & 8 == 0 && is_water(c[0]) {
                    bad = true;
                    break 'check;
                }
            }
        }
        if bad {
            continue;
        }
        let s = size;
        let rx0 = fl(((p.0 as f64 - s as f64) as f32) as f64 + 0.5);
        let rx1 = fl(((p.0 as f64 + s as f64) as f32) as f64 + 0.5);
        let ry0 = fl(((p.1 as f64 - s as f64) as f32) as f64 + 0.5);
        let ry1 = fl(((p.1 as f64 + s as f64) as f32) as f64 + 0.5);
        let mut k = 0usize;
        for x in rx0..rx1 {
            for y in ry0..ry1 {
                let c = &mut tiles.cells[(y * w + x) as usize];
                c[1] |= FLAG_RESERVED;
                if k < mask.len() && mask[k] != 0 {
                    c[0] = mask[k] as u8;
                }
                k += 1;
            }
        }
        return Some((rx0 as f32 as f64, ry0 as f32 as f64));
    }
    None
}
