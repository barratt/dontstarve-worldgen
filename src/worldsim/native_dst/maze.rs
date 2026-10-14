//! RunMaze (0x14012f320) and GetPointsForMetaMaze (0x14012f7e0) of the x64
//! dedicated server:
//! - maze object 0x14046e160: float bbox over every node polygon (lattice
//!   min/max rule); lattice points x in [trunc(minx), trunc(maxx)-1), y
//!   likewise, nodes innermost: 20 > dist(site) and PointInPoly;
//! - dispatch 0x140471090: scratch grid S (trunc(max-min) per axis), every
//!   cell 0xCF00 | (type <= 2 ? 4 : 2); region points: flag 0x40 cleared,
//!   state 4; generator by type; result points += min; copy-back 0x14046d740
//!   unless type 0;
//! - growing tree 0x1404702c0 (types 3..5, scale 2) with carve 0x14046d480
//!   and dead-end emitter 0x14046ffd0; meta DFS 0x14046ee10 (type 0, scale =
//!   maze tile size) with dead-end prune 0x14046dc20. Types 1/2
//!   (0x14046e6c0) are not ported: no vanilla script uses them.

use std::collections::BTreeMap;

use crate::pcg;
use crate::worldsim::native_dst::blend;
use crate::worldsim::native_dst::grid::{random_shuffle, Grid, Tiles};
use crate::worldsim::native_dst::map::{point_in_poly, DstMap};

pub struct Maze {
    min: (f32, f32),
    max: (f32, f32),
    points: Vec<(f32, f32)>,
    kind: i32,
    val: i32,
    size: i32,
    imp: u16,
    floor: u16,
}

pub type MazeResult = BTreeMap<i32, Vec<(f32, f32)>>;

fn lo(g: &Grid, i: usize) -> u8 {
    g.cells[i] as u8
}

fn set_lo(g: &mut Grid, i: usize, v: u8) {
    g.cells[i] = (g.cells[i] & 0xFF00) | v as u16;
}

impl Maze {
    /// 0x14046e160.
    pub fn new(m: &DstMap, nodes: &[usize], kind: i32, imp: u16, floor: u16, val: i32, size: i32) -> Result<Maze, String> {
        let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, -f32::MAX, -f32::MAX);
        for &n in nodes {
            for &(x, y) in &m.nodes[n].poly {
                if !(y > miny) {
                    miny = y;
                }
                if !(x > minx) {
                    minx = x;
                }
                if !(maxy > y) {
                    maxy = y;
                }
                if !(maxx > x) {
                    maxx = x;
                }
            }
        }
        let mut sites = Vec::with_capacity(nodes.len());
        for &n in nodes {
            let s = m.nodes[n].site.ok_or("maze node without a site (NULL in the game)")?;
            sites.push((m.sites[s].x, m.sites[s].y));
        }
        let mut points = Vec::new();
        let x1 = (maxx as i64 as i32).wrapping_sub(1) as u32;
        let y1 = (maxy as i64 as i32).wrapping_sub(1) as u32;
        let mut x = minx as i64 as u32;
        while x < x1 {
            let mut y = miny as i64 as u32;
            while y < y1 {
                let p = (x as f32, y as f32);
                for (k, &n) in nodes.iter().enumerate() {
                    let s = sites[k];
                    let dy = p.1 - s.1;
                    let dx = p.0 - s.0;
                    if 20.0f32 > (dy * dy + dx * dx).sqrt() && point_in_poly(p, &m.nodes[n].poly) {
                        points.push(p);
                    }
                }
                y += 1;
            }
            x += 1;
        }
        Ok(Maze { min: (minx, miny), max: (maxx, maxy), points, kind, val, size, imp, floor })
    }

    /// Dispatch 0x140471090.
    pub fn run(&self, tiles: Option<&mut Tiles>) -> Result<MazeResult, String> {
        let cw = (self.max.0 - self.min.0) as i64 as i32;
        let ch = (self.max.1 - self.min.1) as i64 as i32;
        if cw <= 0 || ch <= 0 {
            return Err(format!("maze grid {cw}x{ch}"));
        }
        let mut s = Grid::new(cw as usize, ch as usize);
        let lo0: u16 = if (0..=2).contains(&self.kind) { 4 } else { 2 };
        for c in s.cells.iter_mut() {
            *c = 0xCF00 | lo0;
        }
        for p in &self.points {
            let iy = (p.1 - self.min.1) as i64 as i32;
            let ix = (p.0 - self.min.0) as i64 as i32;
            let i = iy.wrapping_mul(s.w as i32).wrapping_add(ix) as u32 as usize;
            if i >= s.cells.len() {
                return Err("maze region point outside the scratch grid".into());
            }
            let hi = s.hi(i) & 0xBF;
            s.set_hi(i, hi);
            set_lo(&mut s, i, 4);
        }
        let mut result = MazeResult::new();
        match self.kind {
            0 => self.meta(&s, &mut result, self.size)?,
            1 | 2 => return Err("RunMaze types 1/2 (DFS, 0x14046e6c0) are not ported".into()),
            _ => self.growing_tree(&mut s, &mut result, 2)?,
        }
        for pts in result.values_mut() {
            for p in pts.iter_mut() {
                p.0 += self.min.0;
                p.1 += self.min.1;
            }
        }
        if self.kind != 0 {
            let t = tiles.ok_or("RunMaze before ConvertToTileMap")?;
            self.copy_back(t, &s, self.kind == 5);
        }
        Ok(result)
    }

    /// 0x14046d740: maze state back to tile ids (flags untouched).
    fn copy_back(&self, t: &mut Tiles, s: &Grid, swap: bool) {
        for &p in &self.points {
            let lx = p.0 - self.min.0;
            let ly = p.1 - self.min.1;
            if 0.0 > lx || lx >= s.w as i32 as f32 || 0.0 > ly || ly >= s.h as i32 as f32 {
                continue;
            }
            if 0.0 > p.0 || p.0 >= t.w() as i32 as f32 || 0.0 > p.1 || p.1 >= t.h() as i32 as f32 {
                continue;
            }
            let si = (ly as i64 as i32 * s.w as i32 + lx as i64 as i32) as usize;
            let mut v = lo(s, si);
            if v == 4 {
                continue;
            }
            if swap {
                v = if v == 1 { 2 } else { 1 };
            }
            let ti = (p.1 as i64 as i32 * t.w() as i32 + p.0 as i64 as i32) as usize;
            t.t.cells[ti] = if v == 2 { self.floor } else { self.imp };
        }
    }

    /// Coarse grid at `scale` with the "any block cell has 0x40" rule.
    /// `meta`: state 5 and all four wall bits; else the state of the block's
    /// first cell.
    fn coarse(&self, s: &Grid, scale: i32, meta: bool) -> (Grid, Vec<(f32, f32)>) {
        let inv = 1.0f32 / scale as f32;
        let cw = ((self.max.0 - self.min.0) * inv) as i64 as i32 as usize;
        let ch = ((self.max.1 - self.min.1) * inv) as i64 as i32 as usize;
        let sc = scale as usize;
        let mut c = Grid::new(cw, ch);
        let mut open = Vec::new();
        for y in 0..ch {
            for x in 0..cw {
                let si = (s.w * y + x) * sc;
                let blocked = if scale == 1 && !meta {
                    s.hi(si) & 0x40 != 0
                } else {
                    let mut b = false;
                    'b: for r in y * sc..(y + 1) * sc {
                        for col in x * sc..x * sc + sc {
                            if s.hi(r * s.w + col) & 0x40 != 0 {
                                b = true;
                                break 'b;
                            }
                        }
                    }
                    b
                };
                let i = c.w * y + x;
                let mut hi = (c.hi(i) & !0x40) | ((blocked as u8) << 6);
                hi = (hi & !0x10) | ((hi >> 2) & 0x10);
                if meta {
                    set_lo(&mut c, i, 5);
                    hi |= 0x0F;
                } else {
                    set_lo(&mut c, i, lo(s, si));
                }
                c.set_hi(i, hi);
                if !blocked {
                    open.push((x as f32, y as f32));
                }
            }
        }
        (c, open)
    }

    /// Growing tree 0x1404702c0.
    fn growing_tree(&self, s: &mut Grid, result: &mut MazeResult, scale: i32) -> Result<(), String> {
        let (mut c, mut f) = self.coarse(s, scale, false);
        let (w, h) = (c.w as i32, c.h as i32);
        if f.is_empty() {
            return Err("RunMaze: no open maze cell (the game reads an empty vector)".into());
        }
        random_shuffle(&mut f);
        let mut frontier: Vec<(i32, i32)> = Vec::new();
        let push_nbrs = |c: &mut Grid, x: i32, y: i32, frontier: &mut Vec<(i32, i32)>| {
            let mut nb = Vec::new();
            for (nx, ny, ok) in [(x - 1, y, x > 0), (x + 1, y, x < w - 1), (x, y - 1, y > 0), (x, y + 1, y < h - 1)] {
                if !ok {
                    continue;
                }
                let i = (ny * w + nx) as usize;
                if c.hi(i) & 0x40 == 0 && lo(c, i) == 4 {
                    set_lo(c, i, 3);
                    nb.push((nx, ny));
                }
            }
            random_shuffle(&mut nb);
            frontier.extend(nb);
        };
        let (sx, sy) = (f[0].0 as i64 as i32, f[0].1 as i64 as i32);
        set_lo(&mut c, (sy * w + sx) as usize, 2);
        push_nbrs(&mut c, sx, sy, &mut frontier);
        if frontier.is_empty() {
            return Err("RunMaze: isolated start cell (the game loops forever)".into());
        }
        // p = (float)(e ^ -val), double square-and-multiply.
        let p = {
            let e = self.val.wrapping_neg();
            let mut n = e.unsigned_abs();
            let (mut base, mut res) = (std::f64::consts::E, 1.0f64);
            loop {
                if n & 1 != 0 {
                    res *= base;
                }
                n >>= 1;
                if n == 0 {
                    break;
                }
                base *= base;
            }
            (if e < 0 { 1.0 / res } else { res }) as f32
        };
        while !frontier.is_empty() {
            let u = pcg::raw() as f32 * 2.3283064365386963e-10f32;
            let k = blend::powf(u, p) * (frontier.len() - 1) as f32;
            let idx = k as i64 as i32 as usize;
            if idx >= frontier.len() {
                return Err("RunMaze: frontier index out of range".into());
            }
            let (x, y) = frontier.remove(idx);
            let i = (y * w + x) as usize;
            if carve(&c, x, y, self.kind == 3) {
                set_lo(&mut c, i, 2);
                push_nbrs(&mut c, x, y, &mut frontier);
            } else {
                set_lo(&mut c, i, 1);
            }
        }
        emit_dead_ends(&c, result, scale as f32);
        let sc = scale as usize;
        for y in 0..c.h {
            for x in 0..c.w {
                let i = y * c.w + x;
                if lo(&c, i) == 4 {
                    set_lo(&mut c, i, 1);
                }
                if scale != 1 {
                    let v = lo(&c, i);
                    for r in y * sc..(y + 1) * sc {
                        for col in x * sc..x * sc + sc {
                            set_lo(s, r * s.w + col, v);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Meta maze DFS 0x14046ee10 (type 0; scale = maze tile size).
    fn meta(&self, s: &Grid, result: &mut MazeResult, scale: i32) -> Result<(), String> {
        let (mut c, mut f) = self.coarse(s, scale, true);
        let (w, h) = (c.w as i32, c.h as i32);
        if f.is_empty() {
            // "Couldn't generate any points for maze. Aborting."
            return Ok(());
        }
        random_shuffle(&mut f);
        let last = *f.last().unwrap();
        let idx = |p: (i32, i32)| (p.1 * w + p.0) as usize;
        let mut cur = (last.0 as i64 as i32, last.1 as i64 as i32);
        let v = c.hi(idx(cur)) | 0x10;
        c.set_hi(idx(cur), v);
        let mut visited = 1u32;
        let mut forward = true;
        let mut stack: Vec<(i32, i32)> = Vec::new();
        let mut dead: Vec<(f32, f32)> = Vec::new();
        loop {
            if stack.is_empty() && visited != 1 {
                break;
            }
            let (x, y) = cur;
            let mut nb = Vec::new();
            for (nx, ny, ok) in [(x, y - 1, y > 0), (x, y + 1, y + 1 < h), (x + 1, y, x + 1 < w), (x - 1, y, x > 0)] {
                if ok && c.hi(idx((nx, ny))) & 0x50 == 0 {
                    nb.push((nx, ny));
                }
            }
            set_lo(&mut c, idx(cur), 5);
            if !nb.is_empty() {
                random_shuffle(&mut nb);
                let next = *nb.last().unwrap();
                stack.push(cur);
                set_lo(&mut c, idx(next), 5);
                let (ci, ni) = (idx(cur), idx(next));
                let (cm, nm) = if y < next.1 {
                    (!1u8, !4u8)
                } else if x > next.0 {
                    (!8, !2)
                } else if y > next.1 {
                    (!4, !1)
                } else if x < next.0 {
                    (!2, !8)
                } else {
                    (0xFF, 0xFF)
                };
                let v = c.hi(ni) & nm;
                c.set_hi(ni, v);
                let v = c.hi(ci) & cm;
                c.set_hi(ci, v);
                forward = true;
                if nb.len() >= 2 {
                    stack.push(cur);
                }
                let v = c.hi(ni) | 0x10;
                c.set_hi(ni, v);
                visited += 1;
                cur = next;
            } else {
                let Some(top) = stack.pop() else {
                    // "cell_stack size is 0?!"
                    break;
                };
                if forward {
                    dead.push((x as f32, y as f32));
                }
                cur = top;
                forward = false;
            }
        }
        // Dead-end prune 0x14046dc20 (iterates a copy, erases from the original).
        for p in dead.clone() {
            let (x, y) = (p.0 as i64 as i32, p.1 as i64 as i32);
            let i = w * y + x;
            let open = !c.hi(i as usize) & 0xF;
            let ni = match open {
                1 => (y - 1) * w + x,
                2 => i - 1,
                4 => (y + 1) * w + x,
                _ => i + 1,
            };
            if ni >= 0 && ni < h * w {
                let nh = c.hi(ni as usize);
                if (nh >> 3 & 1) + (nh >> 2 & 1) + (nh >> 1 & 1) + (nh & 1) == 2 {
                    continue;
                }
            }
            if let Some(k) = dead.iter().position(|&q| q == p) {
                dead.remove(k);
            }
        }
        let sc = scale as u32;
        for y in 0..h {
            for x in 0..w {
                let hh = c.hi(idx((x, y)));
                if hh & 0x40 != 0 {
                    continue;
                }
                let mut key = (!hh & 0xF) as i32;
                if key == 0 {
                    continue;
                }
                if dead.iter().any(|&q| q == (x as f32, y as f32)) {
                    key = -key;
                }
                result.entry(key).or_default().push(((x as u32 * sc) as f32, (y as u32 * sc) as f32));
            }
        }
        Ok(())
    }
}

/// Carve test 0x14046d480: exactly one passage neighbour (W=1, E=2, N=4,
/// S=8), and with `diag` (type 3) no passage on the far diagonals.
fn carve(c: &Grid, x: i32, y: i32, diag: bool) -> bool {
    let (w, h) = (c.w as i32, c.h as i32);
    let at = |x: i32, y: i32| lo(c, (y * w + x) as usize) == 2;
    let mut m = 0;
    if x > 0 && at(x - 1, y) {
        m = 1;
    }
    if x < w - 1 && at(x + 1, y) {
        m += 2;
    }
    if y > 0 && at(x, y - 1) {
        m += 4;
    }
    if y < h - 1 && at(x, y + 1) {
        m += 8;
    }
    if !diag {
        return matches!(m, 1 | 2 | 4 | 8);
    }
    match m {
        1 => !(x < w - 1 && ((y > 0 && at(x + 1, y - 1)) || (y < h - 1 && at(x + 1, y + 1)))),
        2 => !(x > 0 && ((y > 0 && at(x - 1, y - 1)) || (y < h - 1 && at(x - 1, y + 1)))),
        4 => !(y < h - 1 && ((x > 0 && at(x - 1, y + 1)) || (x < w - 1 && at(x + 1, y + 1)))),
        8 => !(y > 0 && ((x > 0 && at(x - 1, y - 1)) || (x < w - 1 && at(x + 1, y - 1)))),
        _ => false,
    }
}

/// 0x14046d670: 4 minus the passage neighbours of (x, y).
fn passages_left(c: &Grid, x: i32, y: i32) -> i32 {
    let (w, h) = (c.w as i32, c.h as i32);
    let at = |x: i32, y: i32| lo(c, (y * w + x) as usize) == 2;
    let mut r = 4;
    if x != 0 && at(x - 1, y) {
        r -= 1;
    }
    if x + 1 < w && at(x + 1, y) {
        r -= 1;
    }
    if y != 0 && at(x, y - 1) {
        r -= 1;
    }
    if y + 1 < h && at(x, y + 1) {
        r -= 1;
    }
    r
}

/// Emitter 0x14046ffd0: dead ends whose neighbour is a corridor, key 0;
/// y descending, x ascending.
fn emit_dead_ends(c: &Grid, result: &mut MazeResult, scale: f32) {
    let (w, h) = (c.w as i32, c.h as i32);
    let at = |x: i32, y: i32| lo(c, (y * w + x) as usize) == 2;
    for y in (0..h).rev() {
        for x in 0..w {
            if !at(x, y) {
                continue;
            }
            let (mut cnt, mut dir) = (4, -1);
            if x != 0 && at(x - 1, y) {
                dir = 2;
                cnt = 3;
            }
            if x + 1 < w && at(x + 1, y) {
                cnt -= 1;
                dir = 8;
            }
            if y != 0 && at(x, y - 1) {
                cnt -= 1;
                dir = 4;
            }
            if y + 1 < h && at(x, y + 1) {
                cnt -= 1;
                dir = 1;
            }
            if cnt != 3 {
                continue;
            }
            let r = match dir {
                1 => passages_left(c, x, y + 1),
                2 => passages_left(c, x - 1, y),
                4 => passages_left(c, x, y - 1),
                8 => passages_left(c, x + 1, y),
                _ => continue,
            };
            if r != 2 {
                continue;
            }
            result.entry(0).or_default().push((x as u32 as f32 * scale, y as f32 * scale));
        }
    }
}
