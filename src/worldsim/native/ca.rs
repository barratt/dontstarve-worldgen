//! RunCA (0x004A47C0): the cave cellular automaton (0x64B7E0 init,
//! 0x64B480 region, 0x64B170 seed, 0x64ADD0 iterate, 0x64AC90 rule,
//! 0x64AF70 apply).

use crate::crt;
use crate::worldsim::native::boostmap::{BoostMap, P2};
use crate::worldsim::native::islands::line;
use crate::worldsim::native::points::random_shuffle;
use crate::worldsim::native::tiles::TileGrid;

fn fr(x: f64) -> f64 {
    x as f32 as f64
}

/// fistp with truncation into a 64-bit slot, low dword kept.
fn trunc32(x: f64) -> u32 {
    x.trunc() as i64 as u32
}

/// RunCA(id, iterations, seed_mode, num_random_points).
pub fn run_ca(m: &BoostMap, world: &mut TileGrid, n: usize, iterations: i32, mode: i32, count: i32) -> Result<(), String> {
    let node = &m.nodes[n];
    // Init 0x64B7E0: polygon bbox (0x445290), two cw x ch grids whose ctor
    // draws one rand() per cell (grid A, then grid B), ids then zeroed.
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, -f32::MAX, -f32::MAX);
    for &(x, y) in &node.poly {
        if x < minx { minx = x; }
        if y < miny { miny = y; }
        if x > maxx { maxx = x; }
        if y > maxy { maxy = y; }
    }
    let cw = trunc32(fr(maxx as f64 - minx as f64)) as usize;
    let ch = trunc32(fr(maxy as f64 - miny as f64)) as usize;
    if cw.checked_mul(ch).map_or(true, |c| c > 1 << 26) {
        return Err(format!("RunCA: CA grid {cw}x{ch} out of range"));
    }
    let mut a = TileGrid::new(cw, ch);
    let mut b = TileGrid::new(cw, ch);
    for c in a.cells.iter_mut().chain(b.cells.iter_mut()) {
        c[0] = 0;
    }

    // Region 0x64B480: lattice points within 20 of the site and inside the
    // polygon (x outer, y inner; both bounds exclusive of trunc(max) - 1).
    let site = node.site.map(|s| &m.sites[s]).ok_or("RunCA: node has no site")?;
    let (x0, y0) = (trunc32(minx as f64), trunc32(miny as f64));
    let (x1, y1) = (trunc32(maxx as f64).wrapping_sub(1), trunc32(maxy as f64).wrapping_sub(1));
    let mut pts: Vec<P2> = Vec::new();
    let mut x = x0;
    while x < x1 {
        let mut y = y0;
        while y < y1 {
            let (px, py) = (x as f32, y as f32);
            let dx = fr(px as f64 - site.x as f64);
            let dy = fr(py as f64 - site.y as f64);
            let d = fr(fr(dx * dx + dy * dy).sqrt());
            let (lx, ly) = (x.wrapping_sub(x0) as usize, y.wrapping_sub(y0) as usize);
            if ly * cw + lx >= cw * ch {
                return Err("RunCA: region index outside the CA grid (TileGrid assert)".into());
            }
            if 20.0 > d && crate::worldsim::native::boostmap::point_in_poly((px, py), &node.poly) {
                pts.push((px, py));
            }
            y += 1;
        }
        x += 1;
    }

    // Seed 0x64B170.
    let local = |v: f32, lo: f32| (v as f64 - lo as f64).trunc() as i64 as u32 as usize;
    let set4 = |a: &mut TileGrid, lx: usize, ly: usize| -> Result<(), String> {
        let i = ly.wrapping_mul(cw).wrapping_add(lx);
        if i >= a.cells.len() {
            return Err("RunCA: seed outside the CA grid (TileGrid assert)".into());
        }
        a.cells[i][0] = 4;
        Ok(())
    };
    match mode {
        1 => set4(&mut a, local(node.centroid.0, minx), local(node.centroid.1, miny))?,
        2 => set4(&mut a, local(site.x, minx), local(site.y, miny))?,
        3 => {
            let p = &node.poly;
            let l = |q: P2| (fr(q.0 as f64 - minx as f64), fr(q.1 as f64 - miny as f64));
            for k in 1..p.len() {
                let (s, e) = (l(p[k - 1]), l(p[k]));
                line(&mut a, s.0, s.1, e.0, e.1, 4, false, false);
            }
            if let (Some(&last), Some(&first)) = (p.last(), p.first()) {
                let (s, e) = (l(last), l(first));
                line(&mut a, s.0, s.1, e.0, e.1, 4, false, false);
            }
        }
        _ => {
            let k = (count as u32).min(pts.len() as u32) as usize;
            random_shuffle(&mut pts);
            for p in &pts[..k] {
                set4(&mut a, local(p.0, minx), local(p.1, miny))?;
            }
        }
    }

    // Iterate 0x64ADD0 (do-while on the count): x outer, y inner, rule
    // 0x64AC90 on every non-4 cell, then swap the grids.
    let mut left = iterations as u32;
    if left == 0 {
        return Err("RunCA: 0 iterations loops 2^32 times in the game".into());
    }
    loop {
        for x in 0..cw {
            for y in 0..ch {
                let mut v = a.cells[y * cw + x][0];
                if v != 4 {
                    v = rule(&a, v, x as i32, y as i32, cw as i32, ch as i32);
                }
                b.cells[y * cw + x][0] = v;
            }
        }
        std::mem::swap(&mut a, &mut b);
        left -= 1;
        if left == 0 {
            break;
        }
    }

    // Apply 0x64AF70: world id = v + 1 where the CA value is non-zero.
    for &(px, py) in &pts {
        let lx = fr(px as f64 - minx as f64);
        let ly = fr(py as f64 - miny as f64);
        if 0.0 > lx || lx >= cw as f64 || 0.0 > ly || ly >= ch as f64 {
            continue; // "Oi! src"
        }
        let v = a.cells[(ly.trunc() as usize) * cw + lx.trunc() as usize][0];
        if v == 0 {
            continue;
        }
        if 0.0 > px || px as f64 >= world.w as f64 || 0.0 > py || py as f64 >= world.h as f64 {
            continue; // "Oi! target"
        }
        let i = (py.trunc() as usize) * world.w + px.trunc() as usize;
        let c = &mut world.cells[i];
        c[0] = v.wrapping_add(1);
        c[1] &= 0xBF;
        c[1] &= 0x7F;
    }
    Ok(())
}

/// Rule 0x64AC90: max over the 8 Moore neighbours inside the grid (centre
/// excluded); above 2 it returns m - rand() % m, else the old value.
fn rule(a: &TileGrid, v: u8, x: i32, y: i32, w: i32, h: i32) -> u8 {
    let mut m = 0u8;
    for nx in x - 1..=x + 1 {
        for ny in y - 1..=y + 1 {
            if nx == x && ny == y {
                continue;
            }
            if nx < 0 || nx >= w || ny < 0 || ny >= h {
                continue;
            }
            let c = a.cells[(ny * w + nx) as usize][0];
            if c > m {
                m = c;
            }
        }
    }
    if m > 2 {
        let r = crt::rand() as i32;
        m.wrapping_sub((r % m as i32) as u8)
    } else {
        v
    }
}
