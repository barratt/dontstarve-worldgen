//! RunMaze (0x4AB4A0), GetPointsForMetaMaze (0x4AB830) and DrawCellLine
//! (0x4A5C60): region rasteriser 0x64D2D0, dispatch 0x64F7C0, meta DFS 0x64E230 (type 0),
//! growing tree 0x64EF70 (types 3/4/5), copy-back 0x64C7E0. Types 1/2
//! (0x64D940) are not ported: no shipped Lua uses them.

use std::collections::BTreeMap;

use crate::crt;
use crate::worldsim::native::boostmap::{point_in_poly, BoostMap, P2};
use crate::worldsim::native::points::random_shuffle;
use crate::worldsim::native::tiles::TileGrid;

fn fr(x: f64) -> f64 {
    x as f32 as f64
}

/// fistp with truncation into a 64-bit slot, low dword kept.
fn trunc32(x: f64) -> u32 {
    x.trunc() as i64 as u32
}

const ASSERT: &str = "TileGrid assert idx < W*H (the game crashes here)";

/// Region object built by 0x64D2D0.
struct Region {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    points: Vec<P2>,
    kind: i32,
    val: i32,
}

/// 0x64D2D0: bbox of all node polygons, then lattice points (x outer, y
/// inner, both up to trunc(max) - 1 exclusive) within 20 of a node's site
/// and inside its polygon; one push per node that accepts the point.
fn region(m: &BoostMap, nodes: &[usize], kind: i32, val: i32) -> Region {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, -f32::MAX, -f32::MAX);
    for &n in nodes {
        for &(x, y) in &m.nodes[n].poly {
            // 0x445290: strict compares.
            if min_x > x { min_x = x; }
            if min_y > y { min_y = y; }
            if x > max_x { max_x = x; }
            if y > max_y { max_y = y; }
        }
    }
    let mut points = Vec::new();
    let (x0, y0) = (trunc32(min_x as f64), trunc32(min_y as f64));
    let (x1, y1) = (trunc32(max_x as f64).wrapping_sub(1), trunc32(max_y as f64).wrapping_sub(1));
    let mut x = x0;
    while x < x1 {
        let mut y = y0;
        while y < y1 {
            let p = (x as f32, y as f32);
            for &n in nodes {
                let node = &m.nodes[n];
                let Some(s) = node.site.map(|s| &m.sites[s]) else { continue };
                let dx = fr(p.0 as f64 - s.x as f64);
                let dy = fr(p.1 as f64 - s.y as f64);
                let d = fr(fr(dx * dx + dy * dy).sqrt());
                if 20.0 > d && point_in_poly(p, &node.poly) {
                    points.push(p);
                }
            }
            y += 1;
        }
        x += 1;
    }
    Region { min_x, min_y, max_x, max_y, points, kind, val }
}

struct Grid {
    w: usize,
    h: usize,
    /// [id, flags]
    c: Vec<[u8; 2]>,
}

impl Grid {
    /// TileGrid ctor (one rand() per cell) followed by memset 0xFF.
    fn new(w: usize, h: usize) -> Result<Grid, String> {
        if w.checked_mul(h).map_or(true, |n| n > 1 << 26) {
            return Err(format!("maze scratch grid {w}x{h} out of range"));
        }
        for _ in 0..w * h {
            crt::rand();
        }
        Ok(Grid { w, h, c: vec![[0xFF, 0xFF]; w * h] })
    }

    fn at(&self, i: usize) -> Result<usize, String> {
        if i < self.c.len() { Ok(i) } else { Err(ASSERT.into()) }
    }
}

/// Lua-visible result: std::map<int, vector<float2>> in key order.
pub type MazeOut = BTreeMap<i32, Vec<P2>>;

/// Dispatch 0x64F7C0(region, world, &out).
fn dispatch(rg: &Region, world: &mut TileGrid, out: &mut MazeOut) -> Result<(), String> {
    let mw = trunc32(fr(rg.max_x as f64 - rg.min_x as f64)) as usize;
    let mh = trunc32(fr(rg.max_y as f64 - rg.min_y as f64)) as usize;
    let mut s = Grid::new(mw, mh)?;
    let fill = if matches!(rg.kind, 0 | 1 | 2) { 0x7D } else { 0x12 };
    for c in &mut s.c {
        c[1] |= 0x0F;
        c[1] &= 0xEF;
        c[1] |= 0x40;
        c[0] = fill;
    }
    for &(px, py) in &rg.points {
        let lx = fr(px as f64 - rg.min_x as f64);
        let ly = fr(py as f64 - rg.min_y as f64);
        let i = (trunc32(ly) as usize).wrapping_mul(mw).wrapping_add(trunc32(lx) as usize);
        let i = s.at(i)?;
        s.c[i][1] &= 0xBF;
        s.c[i][0] = 0x7D;
    }
    match rg.kind {
        0 => meta_dfs(rg, &s, out)?,
        1 | 2 => return Err("RunMaze: DFS maze types 1/2 (0x64D940) are not ported".into()),
        _ => growing_tree(rg, &mut s, out, 2)?,
    }
    for pts in out.values_mut() {
        for p in pts.iter_mut() {
            p.0 = fr(p.0 as f64 + rg.min_x as f64) as f32;
            p.1 = fr(p.1 as f64 + rg.min_y as f64) as f32;
        }
    }
    if rg.kind != 0 {
        copy_back(rg, world, &s, rg.kind == 5)?;
    }
    Ok(())
}

/// Copy-back 0x64C7E0: region points take the scratch id unless 0x7D
/// (type 5 swaps 0xA0 -> 0x12, anything else -> 0xA0). Ids only.
fn copy_back(rg: &Region, world: &mut TileGrid, s: &Grid, inv: bool) -> Result<(), String> {
    for &(px, py) in &rg.points {
        let lx = fr(px as f64 - rg.min_x as f64);
        let ly = fr(py as f64 - rg.min_y as f64);
        if 0.0 > lx || lx >= s.w as f64 || 0.0 > ly || ly >= s.h as f64 {
            continue; // "Oi! src"
        }
        let si = (trunc32(ly) as usize) * s.w + trunc32(lx) as usize;
        if 0.0 > px || px as f64 >= world.w as f64 || 0.0 > py || py as f64 >= world.h as f64 {
            continue; // "Oi! target"
        }
        let wi = (py.trunc() as usize) * world.w + px.trunc() as usize;
        let v = s.c[s.at(si)?][0];
        if v == 0x7D {
            continue;
        }
        let v = if inv { if v == 0xA0 { 0x12 } else { 0xA0 } } else { v };
        world.cells[wi][0] = v;
    }
    Ok(())
}

/// Half-resolution grid of 0x64EF70 / 0x64E230: `outside` (flag 0x40, and
/// 0x10 = 0x40) if any source cell of the block is outside the region; id =
/// the block's top-left source id (growing tree) or 0x9A (meta).
fn downsample(s: &Grid, m: &mut Grid, b: usize, meta: bool) -> Result<Vec<P2>, String> {
    let mut v = Vec::new();
    for y in 0..m.h {
        for x in 0..m.w {
            let mut outside = false;
            if b != 1 || meta {
                'blk: for sy in y * b..(y + 1) * b {
                    for sx in x * b..x * b + b {
                        if s.c[s.at(sy * s.w + sx)?][1] & 0x40 != 0 {
                            outside = true;
                            break 'blk;
                        }
                    }
                }
            }
            let i = m.w * y + x;
            let c = &mut m.c[i];
            if meta {
                c[1] = (c[1] & !0x40) | ((outside as u8) << 6);
                c[1] = (c[1] & !0x10) | ((c[1] >> 2) & 0x10);
                c[0] = 0x9A;
                c[1] |= 0x0F;
            } else {
                let si = s.at((s.w * y + x) * b)?;
                if b == 1 {
                    outside = s.c[si][1] & 0x40 != 0;
                }
                c[1] = (c[1] & !0x40) | ((outside as u8) << 6);
                c[1] = (c[1] & !0x10) | ((c[1] >> 2) & 0x10);
                c[0] = s.c[si][0];
            }
            if c[1] & 0x40 == 0 {
                v.push((x as f32, y as f32));
            }
        }
    }
    Ok(v)
}

/// Frontier claim 0x64DCC0: mark p visited (0x12); claim the in-region
/// unvisited (0x7D) 4-neighbours (x-1, x+1, y-1, y+1) as 1, shuffle them
/// and append to the frontier.
fn claim(m: &mut Grid, f: &mut Vec<(i32, i32)>, p: (i32, i32)) -> Result<(), String> {
    let (x, y) = p;
    let (w, h) = (m.w as i32, m.h as i32);
    let i = m.at(y.wrapping_mul(w).wrapping_add(x) as u32 as usize)?;
    m.c[i][0] = 0x12;
    let mut n = Vec::new();
    for (ok, q) in [(x > 0, (x - 1, y)), (x < w - 1, (x + 1, y)), (y > 0, (x, y - 1)), (y < h - 1, (x, y + 1))] {
        if !ok {
            continue;
        }
        let j = m.at((q.1 * w + q.0) as u32 as usize)?;
        if m.c[j][1] & 0x40 == 0 && m.c[j][0] == 0x7D {
            m.c[j][0] = 1;
            n.push(q);
        }
    }
    random_shuffle(&mut n);
    f.extend(n);
    Ok(())
}

/// 0x64C3B0: exactly one visited 4-neighbour; with `strict` (type 3) also no
/// visited cell diagonally on the far side.
fn check(m: &Grid, p: (i32, i32), strict: bool) -> bool {
    let (x, y) = p;
    let (w, h) = (m.w as i32, m.h as i32);
    let id = |x: i32, y: i32| m.c.get((y * w + x) as u32 as usize).map_or(0, |c| c[0]);
    let mut mask = 0;
    if x > 0 && id(x - 1, y) == 0x12 { mask |= 1; }
    if x < w - 1 && id(x + 1, y) == 0x12 { mask += 2; }
    if y > 0 && id(x, y - 1) == 0x12 { mask += 4; }
    if y < h - 1 && id(x, y + 1) == 0x12 { mask += 8; }
    if !strict {
        return matches!(mask, 1 | 2 | 4 | 8);
    }
    match mask {
        1 => !(x < w - 1 && ((y > 0 && id(x + 1, y - 1) == 0x12) || (y < h - 1 && id(x + 1, y + 1) == 0x12))),
        2 => !(x > 0 && ((y > 0 && id(x - 1, y - 1) == 0x12) || (y < h - 1 && id(x - 1, y + 1) == 0x12))),
        4 => !(y < h - 1 && ((x > 0 && id(x - 1, y + 1) == 0x12) || (x < w - 1 && id(x + 1, y + 1) == 0x12))),
        8 => !(y > 0 && ((x > 0 && id(x - 1, y - 1) == 0x12) || (x < w - 1 && id(x + 1, y - 1) == 0x12))),
        _ => false,
    }
}

/// 0x64C670: open (0x12) 4-neighbour count as 4 - visited, with the last
/// open direction (2 = x-1, 8 = x+1, 4 = y-1, 1 = y+1).
fn walls(m: &Grid, x: u32, y: u32, dir: &mut i32) -> i32 {
    let (w, h) = (m.w as u32, m.h as u32);
    let id = |x: u32, y: u32| m.c.get(y.wrapping_mul(w).wrapping_add(x) as usize).map_or(0, |c| c[0]);
    let mut n = 4;
    if x > 0 && id(x - 1, y) == 0x12 { n = 3; *dir = 2; }
    if x < w.wrapping_sub(1) && id(x + 1, y) == 0x12 { n -= 1; *dir = 8; }
    if y > 0 && id(x, y - 1) == 0x12 { n -= 1; *dir = 4; }
    if y < h.wrapping_sub(1) && id(x, y + 1) == 0x12 { n -= 1; *dir = 1; }
    n
}

/// x87 integer power loop of 0x64F48C: e^|n| by squaring, inverted for n < 0.
fn exp_int(n: i32) -> f64 {
    let base0 = std::f64::consts::E;
    let mut a = n.unsigned_abs();
    let mut result = 1.0f64;
    let mut base = base0;
    loop {
        if a & 1 != 0 {
            result *= base;
        }
        a >>= 1;
        if a == 0 {
            break;
        }
        base *= base;
    }
    if n < 0 { 1.0 / result } else { result }
}

/// Growing tree 0x64EF70(S, out, region, type, val, block = 2).
fn growing_tree(rg: &Region, s: &mut Grid, out: &mut MazeOut, block: u32) -> Result<(), String> {
    let bf = block as f32 as f64;
    let inv = fr(1.0 / bf);
    let gw = trunc32(fr(rg.max_x as f64 - rg.min_x as f64) * inv) as usize;
    let gh = trunc32(fr(rg.max_y as f64 - rg.min_y as f64) * inv) as usize;
    let b = block as usize;
    let mut m = Grid::new(gw, gh)?;
    let mut v = downsample(s, &mut m, b, false)?;
    random_shuffle(&mut v);
    let mut f: Vec<(i32, i32)> = Vec::new();
    let Some(&first) = v.first() else { return Err("RunMaze: empty maze region (the game reads past an empty vector)".into()) };
    claim(&mut m, &mut f, (first.0 as i32, first.1 as i32))?;
    if f.is_empty() {
        return Err("RunMaze: first maze cell has no free neighbour (the game loops forever)".into());
    }
    let strict = rg.kind == 3;
    let g = exp_int(rg.val.wrapping_neg()) as f32 as f64;
    while !f.is_empty() {
        let r = crt::rand() as i32;
        let u = fr(r as f64 / 32767.0);
        let h = fr(crate::crt::pow(u, g));
        let idx = (h * (f.len() - 1) as u32 as f64).trunc() as i64 as usize;
        let cell = f.remove(idx);
        if check(&m, cell, strict) {
            claim(&mut m, &mut f, cell)?;
        } else {
            let i = m.at((cell.1 * gw as i32 + cell.0) as u32 as usize)?;
            m.c[i][0] = 0xA0;
        }
    }
    // Dead ends 0x64EDA0(M, out, block): y from H-1 down, x up.
    for y in (0..gh as u32).rev() {
        for x in 0..gw as u32 {
            if m.c[(y as usize) * gw + x as usize][0] != 0x12 {
                continue;
            }
            let mut dir = -1;
            if walls(&m, x, y, &mut dir) != 3 {
                continue;
            }
            let (nx, ny) = match dir {
                1 => (x, y.wrapping_add(1)),
                2 => (x.wrapping_sub(1), y),
                4 => (x, y.wrapping_sub(1)),
                8 => (x.wrapping_add(1), y),
                _ => continue,
            };
            if walls(&m, nx, ny, &mut dir) == 2 {
                let px = fr(x as f64 * bf) as f32;
                let py = fr(bf * y as i32 as f64) as f32;
                out.entry(0).or_default().push((px, py));
            }
        }
    }
    for y in 0..gh {
        for x in 0..gw {
            let i = m.w * y + x;
            if m.c[i][0] == 0x7D {
                m.c[i][0] = 0xA0;
            }
            if b != 1 {
                for sy in y * b..(y + 1) * b {
                    for sx in x * b..x * b + b {
                        let j = s.at(s.w * sy + sx)?;
                        s.c[j][0] = m.c[i][0];
                    }
                }
            }
        }
    }
    Ok(())
}

/// 4-neighbour enumerator 0x64D600 (order y-1, y+1, x+1, x-1), skipping
/// visited (0x10) and outside (0x40) cells.
fn neighbours(m: &Grid, p: (i32, i32)) -> Result<Vec<(i32, i32)>, String> {
    let (x, y) = p;
    let (w, h) = (m.w as i32, m.h as i32);
    let mut n = Vec::new();
    for (ok, q) in [(y > 0, (x, y - 1)), (y + 1 < h, (x, y + 1)), (x + 1 < w, (x + 1, y)), (x > 0, (x - 1, y))] {
        if !ok {
            continue;
        }
        let j = m.at((q.1 * w + q.0) as u32 as usize)?;
        if m.c[j][1] & 0x10 == 0 && m.c[j][1] & 0x40 == 0 {
            n.push(q);
        }
    }
    Ok(n)
}

/// Carve 0x64C270(M, a, b): b := 0x0D and clear the shared exit bits.
fn carve(m: &mut Grid, a: (i32, i32), b: (i32, i32)) -> Result<(), String> {
    let w = m.w as i32;
    let bi = m.at((b.1 * w + b.0) as u32 as usize)?;
    let ai = m.at((a.1 * w + a.0) as u32 as usize)?;
    m.c[bi][0] = 0x0D;
    let (ab, bb) = if a.1 < b.1 {
        (1, 4)
    } else if a.0 > b.0 {
        (8, 2)
    } else if a.1 > b.1 {
        (4, 1)
    } else if a.0 < b.0 {
        (2, 8)
    } else {
        return Ok(());
    };
    m.c[bi][1] &= !bb;
    m.c[ai][1] &= !ab;
    Ok(())
}

/// Meta maze 0x64E230(S, points, region, 0, out): DFS on 8x8 blocks.
fn meta_dfs(rg: &Region, s: &Grid, out: &mut MazeOut) -> Result<(), String> {
    let gw = trunc32(fr(rg.max_x as f64 - rg.min_x as f64) * 0.125) as usize;
    let gh = trunc32(fr(rg.max_y as f64 - rg.min_y as f64) * 0.125) as usize;
    let mut m = Grid::new(gw, gh)?;
    let mut v = downsample(s, &mut m, 8, true)?;
    random_shuffle(&mut v);
    let Some(&start) = v.last() else { return Err("GetPointsForMetaMaze: empty meta grid (the game reads past an empty vector)".into()) };
    let mut cur = (start.0 as i32, start.1 as i32);
    let ci = m.at((cur.1 * gw as i32 + cur.0) as u32 as usize)?;
    m.c[ci][1] |= 0x10;
    let mut stack: Vec<(i32, i32)> = Vec::new();
    let mut dead: Vec<P2> = Vec::new();
    let mut visited = 1u32;
    let mut advanced = true;
    loop {
        let n = stack.len();
        if n == 0 && visited != 1 {
            break;
        }
        let mut nb = neighbours(&m, cur)?;
        let ci = m.at((cur.1 * gw as i32 + cur.0) as u32 as usize)?;
        m.c[ci][0] = 0x0D;
        if !nb.is_empty() {
            random_shuffle(&mut nb);
            let next = *nb.last().unwrap();
            stack.push(cur);
            carve(&mut m, cur, next)?;
            if nb.len() > 1 {
                stack.push(cur);
            }
            advanced = true;
            cur = next;
            let ni = m.at((cur.1 * gw as i32 + cur.0) as u32 as usize)?;
            m.c[ni][1] |= 0x10;
            visited += 1;
        } else {
            if n == 0 {
                break;
            }
            if advanced {
                dead.push((cur.0 as f32, cur.1 as f32));
            }
            cur = stack.pop().unwrap();
            advanced = false;
        }
    }
    if gw == 0 || gh == 0 {
        return Err("GetPointsForMetaMaze: zero-sized meta grid (division by zero in the game)".into());
    }
    // Four border openings (rand() % W or % H, unsigned).
    let r = crt::rand() as usize % gw;
    let i = m.at(r)?;
    m.c[i][1] &= 0xFB;
    let r = crt::rand() as usize % gw;
    let i = m.at((gh - 1) * gw + r)?;
    m.c[i][1] &= 0xFE;
    let r = crt::rand() as usize % gh;
    let i = m.at(r * gw)?;
    m.c[i][1] &= 0xF7;
    let r = crt::rand() as usize % gh;
    let i = m.at(r * gw + gw - 1)?;
    m.c[i][1] &= 0xFD;
    // Dead-end filter 0x64CC10: its exit tests compare a nibble with
    // 0xFE/0xFD/0xFB (never true), so every dead end looks at its x+1
    // neighbour and is kept only if that cell has exactly two closed exits.
    for d in dead.clone() {
        let j = (d.1.trunc() as i64 as u32 as usize).wrapping_mul(gw).wrapping_add(d.0.trunc() as i64 as u32 as usize).wrapping_add(1);
        let j = m.at(j).map_err(|e| format!("{e}, dead-end filter 0x64CC10 reading the x+1 cell of ({}, {}) in a {gw}x{gh} meta grid", d.0, d.1))?;
        let f = m.c[j][1];
        let closed = ((f >> 3) & 1) + ((f >> 2) & 1) + ((f >> 1) & 1) + (f & 1);
        if closed != 2 {
            if let Some(k) = dead.iter().position(|e| *e == d) {
                dead.remove(k);
            }
        }
    }
    for y in 0..gh {
        for x in 0..gw {
            let fl = m.c[y * gw + x][1];
            if fl & 0x40 != 0 {
                continue;
            }
            let mut mask = (!fl & 0x0F) as i32;
            if mask == 0 {
                continue;
            }
            if dead.iter().any(|e| *e == (x as f32, y as f32)) {
                mask = -mask;
            }
            out.entry(mask).or_default().push(((8 * x) as u32 as f32, (8 * y) as u32 as f32));
        }
    }
    Ok(())
}

/// RunMaze / GetPointsForMetaMaze core: region, dispatch, copy-back.
pub fn run(m: &BoostMap, world: &mut TileGrid, nodes: &[usize], kind: i32, val: i32) -> Result<MazeOut, String> {
    let mut out = MazeOut::new();
    if nodes.is_empty() {
        return Ok(out);
    }
    let rg = region(m, nodes, kind, val);
    dispatch(&rg, world, &mut out)?;
    Ok(out)
}
