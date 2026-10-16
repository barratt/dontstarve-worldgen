//! Native versions of the game's per-tile Lua helpers (see lua_patch.rs):
//! the waterline fills of water.lua / ocean_gen.lua, the IsCloseTo* ring
//! scans and simplexnoise2d. Each follows its script statement by
//! statement, in Lua's number arithmetic, so it reads and writes the same
//! tiles in the same order.

/// WorldSim:GetTile / SetTile (three arguments) on a tile map.
pub trait Grid {
    fn get(&self, x: f64, y: f64) -> f64;
    fn set(&mut self, x: f64, y: f64, ground: f64);
}

/// fillGroundType: a depth-limited flood of `ground` over tiles that are
/// not `lined`.
#[allow(clippy::too_many_arguments)]
pub fn fill(g: &mut impl Grid, lined: &impl Fn(f64) -> bool, width: f64, height: f64, x: f64, y: f64, offset: f64, depth: f64, ground: f64) {
    if depth <= 0.0 {
        return;
    }
    if !(0.0 <= x && x < width && 0.0 <= y && y < height) {
        return;
    }
    if lined(g.get(x, y)) {
        return;
    }
    g.set(x, y, ground);
    let depth = depth - 1.0;
    fill(g, lined, width, height, x + offset, y, offset, depth, ground);
    fill(g, lined, width, height, x - offset, y, offset, depth, ground);
    fill(g, lined, width, height, x, y + offset, offset, depth, ground);
    fill(g, lined, width, height, x, y - offset, offset, depth, ground);
}

/// placeGroundType, or placeFilledGroundType with `filled` =
/// (fillOffset, fillDepth). Returns the final (x, y).
#[allow(clippy::too_many_arguments)]
pub fn place(g: &mut impl Grid, lined: &impl Fn(f64) -> bool, width: f64, height: f64, mut x: f64, mut y: f64, offx: f64, offy: f64, depth: f64, ground: f64, filled: Option<(f64, f64)>) -> (f64, f64) {
    let mut i = 0.0;
    while i < depth && 0.0 <= x && x < width && 0.0 < y && y < height {
        if lined(g.get(x, y)) {
            break;
        }
        match filled {
            None => g.set(x, y, ground),
            Some((off, fd)) => {
                fill(g, lined, width, height, x + off, y, off, fd, ground);
                fill(g, lined, width, height, x - off, y, off, fd, ground);
                fill(g, lined, width, height, x, y + off, off, fd, ground);
                fill(g, lined, width, height, x, y - off, off, fd, ground);
            }
        }
        x = x + offx;
        y = y + offy;
        i = i + 1.0;
    }
    (x, y)
}

/// squareFill.
#[allow(clippy::too_many_arguments)]
pub fn square_fill(g: &mut impl Grid, lined: &impl Fn(f64) -> bool, width: f64, height: f64, x: f64, y: f64, radius: f64, ground: f64) {
    let mut yy = y - radius;
    while yy <= y + radius {
        let mut xx = x - radius;
        while xx <= x + radius {
            if 0.0 <= xx && xx < width && 0.0 <= yy && yy < height && !lined(g.get(xx, yy)) {
                g.set(xx, yy, ground);
            }
            xx += 1.0;
        }
        yy += 1.0;
    }
}

/// simplexnoise2d over the engine's perlin.
pub fn simplex_noise_2d(x: f64, y: f64, octaves: f64, persistence: f64) -> f64 {
    let (mut noise, mut amps, mut amp, mut freq) = (0.0, 0.0, 1.0, 2.0);
    // math.max(octaves-1, 1)
    let last = if 1.0 > octaves - 1.0 { 1.0 } else { octaves - 1.0 };
    let mut i = 0.0;
    while i <= last {
        noise = noise + amp * super::native::tiles::perlin01(freq * x, freq * y, 0.0);
        amps = amps + amp;
        amp = amp * persistence;
        freq = freq * 2.0;
        i += 1.0;
    }
    noise / amps
}

/// The IsCloseTo* / IsSurroundedBy* ring scan: whether `hit` holds for
/// any tile on the square ring of `radius` around (x, y), tested in the
/// scripts' order (left/right columns, then top/bottom rows).
pub fn ring_any(x: f64, y: f64, radius: f64, mut hit: impl FnMut(f64, f64) -> bool) -> bool {
    let mut i = -radius;
    while i <= radius {
        if hit(x - radius, y + i) || hit(x + radius, y + i) {
            return true;
        }
        i += 1.0;
    }
    let mut i = -(radius - 1.0);
    while i <= radius - 1.0 {
        if hit(x + i, y - radius) || hit(x + i, y + radius) {
            return true;
        }
        i += 1.0;
    }
    false
}

/// `tile >= a[0] and tile <= a[1]` or `tile >= a[2] and tile <= a[3]`: a
/// TileGroupManager group (legacy range, then current range).
pub fn in_group(tile: f64, a: &[f64]) -> bool {
    (tile >= a[0] && tile <= a[1]) || (tile >= a[2] && tile <= a[3])
}

/// Minimum argument count of each helper (lua_patch.rs always passes all).
pub fn arity(m: super::Fast) -> usize {
    use super::Fast::*;
    match m {
        FillGroundType => 9,
        PlaceGroundType => 10,
        PlaceFilledGroundType => 12,
        SquareFill => 8,
        SimplexNoise2d | IsCloseToTileType => 4,
        IsCloseToWater | IsCloseToLand | IsSurroundedByWater => 3,
        IsSurroundedByWaterOrInvalid | CheckTile => 4,
        AddShoreline => 8,
        RemoveSingleWaterTile => 2,
        NextTile => 5,
        NoisePass => 4,
        BlendPass => 15,
        OceanBlendPass => 19,
        GroundFillPass | SquareFillPass => 10,
        WaterlinePass => 11,
        NextTileEq => 4,
        OceanAddShoreline => 14,
        OceanFillGroundType => 11,
        OceanPlaceGroundType => 12,
        OceanPlaceFilledGroundType => 14,
        OceanSquareFill => 10,
        OceanIsCloseToGroup => 7,
        OceanIsCloseToImpassable => 8,
        OceanIsSurroundedByWaterOrInvalid => 13,
        _ => 0,
    }
}

/// Lua's `%` on numbers (luai_nummod).
fn lua_mod(a: f64, b: f64) -> f64 {
    a - (a / b).floor() * b
}

/// water.lua's findLayoutPositions after its math.random calls: the
/// `size`-square positions whose tiles all pass `ok` (checkTile), scanning
/// from (start_x, start_y) with wrap-around. `count` < 0 means no limit.
/// Returns x, y, x2, y2 per position.
#[allow(clippy::too_many_arguments)]
pub fn find_layout_positions(ok: &impl Fn(f64, f64) -> bool, size: f64, edge_dist: f64, adj_width: f64, adj_height: f64, start_x: f64, start_y: f64, count: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let more = |out: &Vec<f64>| count < 0.0 || ((out.len() / 4) as f64) < count;
    let (mut i, mut j) = (0.0, 0.0);
    while j < adj_height && more(&out) {
        let y = lua_mod(start_y + j, adj_height) + edge_dist;
        while i < adj_width && more(&out) {
            let x = lua_mod(start_x + i, adj_width) + edge_dist;
            let (x2, y2) = (x + size - 1.0, y + size - 1.0);
            if ok(x2, y) && ok(x2, y2) {
                if ok(x, y) && ok(x, y2) {
                    match check_all_tiles(ok, x, y, x2, y2) {
                        None => {
                            out.extend([x, y, x2, y2]);
                            i = i + size + 1.0;
                        }
                        Some((last_x, _)) => i = i + last_x - x + 1.0,
                    }
                } else {
                    i = i + 1.0;
                }
            } else {
                i = i + size + 1.0;
            }
        }
        j = j + 1.0;
        i = 0.0;
    }
    out
}

/// checkAllTiles: the first failing tile, if any.
fn check_all_tiles(ok: &impl Fn(f64, f64) -> bool, x1: f64, y1: f64, x2: f64, y2: f64) -> Option<(f64, f64)> {
    let mut j = y1;
    while j <= y2 {
        let mut i = x1;
        while i <= x2 {
            if !ok(i, j) {
                return Some((i, j));
            }
            i += 1.0;
        }
        j += 1.0;
    }
    None
}

/// GetRandomWaterPoints' get_points after its math.random calls: up to
/// `need` tiles passing `ok`, every `inc` tiles. Returns x, y per point.
#[allow(clippy::too_many_arguments)]
pub fn get_points(ok: &impl Fn(f64, f64) -> bool, adj_width: f64, adj_height: f64, start_x: f64, start_y: f64, edge_dist: f64, need: f64, inc: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let more = |out: &Vec<f64>| ((out.len() / 2) as f64) < need;
    let (mut i, mut j) = (0.0, 0.0);
    while j < adj_height && more(&out) {
        let y = lua_mod(start_y + j, adj_height) + edge_dist;
        while i < adj_width && more(&out) {
            let x = lua_mod(start_x + i, adj_width) + edge_dist;
            if ok(x, y) {
                out.extend([x, y]);
            }
            i = i + inc;
        }
        j = j + inc;
        i = 0.0;
    }
    out
}

thread_local! {
    /// The last three maps GenerateBlendedMap returned (the blend passes
    /// read the three they have just requested).
    static BLENDED: std::cell::RefCell<std::collections::VecDeque<std::rc::Rc<Vec<f64>>>> = Default::default();
}

/// Records a GenerateBlendedMap result.
pub fn remember_blended(map: &[f64]) {
    BLENDED.with(|b| {
        let mut b = b.borrow_mut();
        if b.len() == 3 {
            b.pop_front();
        }
        b.push_back(std::rc::Rc::new(map.to_vec()));
    });
}

/// The last three blended maps, oldest first, when each has `len` values.
pub fn last_blended(len: usize) -> Option<[std::rc::Rc<Vec<f64>>; 3]> {
    BLENDED.with(|b| {
        let b = b.borrow();
        if b.len() != 3 || b.iter().any(|m| m.len() != len) {
            return None;
        }
        Some([b[0].clone(), b[1].clone(), b[2].clone()])
    })
}

/// getEdgeFalloff (water.lua / ocean_gen.lua), for any of the games'
/// math.clamp definitions (they agree on every finite or NaN input once
/// scaled: they differ only in the sign of a zero result).
#[allow(clippy::too_many_arguments)]
pub fn edge_falloff(x: f64, y: f64, width: f64, height: f64, mindist: f64, maxdist: f64, min: f64, max: f64) -> f64 {
    let lmin = |a: f64, b: f64| if b < a { b } else { a };
    let distx = lmin(x, width - x);
    let disty = lmin(y, height - y);
    let edgedist = lmin(distx, disty);
    let dist = (edgedist - mindist) / (maxdist - mindist);
    let c = { let n = lmin(dist, 1.0); if 0.0 > n { 0.0 } else { n } };
    (max - min) * c + min
}

/// do_noise's per-tile loop: on every `imp` tile, the first layer
/// (scale, offx, offy, octaves, persistence, level, tile) whose noise
/// exceeds its level sets its tile, else `last`. `a` = (width, height,
/// imp, n, layers..., last).
pub fn noise_pass(g: &mut impl Grid, a: &[f64]) -> Option<()> {
    let (width, height, imp, n) = (a[0], a[1], a[2], a[3] as usize);
    if a.len() != 4 + 7 * n + 1 || n == 0 {
        return None;
    }
    let layers: Vec<&[f64]> = a[4..4 + 7 * n].chunks(7).collect();
    let last = a[4 + 7 * n];
    let mut y = 0.0;
    while y <= height - 1.0 {
        let mut x = 0.0;
        while x <= width - 1.0 {
            if g.get(x, y) == imp {
                let (nx, ny) = (x / width - 0.5, y / height - 0.5);
                let mut to = last;
                for l in &layers {
                    if simplex_noise_2d(l[0] * (nx + l[1]), l[0] * (ny + l[2]), l[3], l[4]) > l[5] {
                        to = l[6];
                        break;
                    }
                }
                g.set(x, y, to);
            }
            x += 1.0;
        }
        y += 1.0;
    }
    Some(())
}

/// do_groundfill: around every waterlined tile, fillGroundType in the four
/// directions.
#[allow(clippy::too_many_arguments)]
pub fn ground_fill_pass(g: &mut impl Grid, lined: &impl Fn(f64) -> bool, width: f64, height: f64, tile: f64, offset: f64, depth: f64) {
    let mut y = 0.0;
    while y <= height - 1.0 {
        let mut x = 0.0;
        while x <= width - 1.0 {
            if lined(g.get(x, y)) {
                fill(g, lined, width, height, x + 1.0, y, offset, depth, tile);
                fill(g, lined, width, height, x - 1.0, y, offset, depth, tile);
                fill(g, lined, width, height, x, y + 1.0, offset, depth, tile);
                fill(g, lined, width, height, x, y - 1.0, offset, depth, tile);
            }
            x += 1.0;
        }
        y += 1.0;
    }
}

/// do_squarefill: squareFill around every waterlined tile that `close` accepts.
#[allow(clippy::too_many_arguments)]
pub fn square_fill_pass<G: Grid>(g: &mut G, lined: &impl Fn(f64) -> bool, close: impl Fn(&G, f64, f64) -> bool, width: f64, height: f64, radius: f64, tile: f64) {
    let mut y = 0.0;
    while y <= height - 1.0 {
        let mut x = 0.0;
        while x <= width - 1.0 {
            if lined(g.get(x, y)) && close(g, x, y) {
                square_fill(g, lined, width, height, x, y, radius, tile);
            }
            x += 1.0;
        }
        y += 1.0;
    }
}

/// do_waterline (placeWaterline) or, with `filled` = (fillOffset,
/// fillDepth), do_filledwaterline (placeWaterlineFilled), in the eight
/// directions in the script's order.
#[allow(clippy::too_many_arguments)]
pub fn waterline_pass(g: &mut impl Grid, lined: &impl Fn(f64) -> bool, width: f64, height: f64, shallow_depth: f64, medium_depth: f64, shallow: f64, medium: f64, filled: Option<(f64, f64)>) {
    const DIRS: [(f64, f64); 8] = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];
    let mut y = 0.0;
    while y <= height - 1.0 {
        let mut x = 0.0;
        while x <= width - 1.0 {
            if lined(g.get(x, y)) {
                for (dx, dy) in DIRS {
                    let (x0, y0) = (x + dx, y + dy);
                    let (px, py) = place(g, lined, width, height, x0, y0, dx, dy, shallow_depth, shallow, None);
                    place(g, lined, width, height, px, py, dx, dy, medium_depth, medium, None);
                    if let Some(f) = filled {
                        let (qx, qy) = place(g, lined, width, height, x0, y0, dx, dy, medium_depth, medium, Some(f));
                        place(g, lined, width, height, qx, qy, dx, dy, shallow_depth, shallow, Some(f));
                    }
                }
            }
            x += 1.0;
        }
        y += 1.0;
    }
}
