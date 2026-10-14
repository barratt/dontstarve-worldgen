//! Cave natives: RunCA (0x140129940). RunMaze and GetPointsForMetaMaze are
//! in maze.rs.

use crate::pcg;
use crate::worldsim::native_dst::grid::{self, Tiles};
use crate::worldsim::native_dst::map::{point_in_poly, DstMap};

pub fn run_ca(m: &DstMap, t: &mut Tiles, id: &str, iterations: i32, mode: i32, n: i32) -> Result<(), String> {
    let node = m.node(id).ok_or_else(|| format!("RunCA: unknown node {id}"))?;
    let nd = &m.nodes[node];
    let site = nd.site.map(|s| (m.sites[s].x, m.sites[s].y)).ok_or("RunCA: node without a site")?;
    let poly = &nd.poly;
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, -f32::MAX, -f32::MAX);
    for &(x, y) in poly {
        minx = minx.min(x);
        miny = miny.min(y);
        maxx = maxx.max(x);
        maxy = maxy.max(y);
    }
    if poly.is_empty() {
        return Ok(());
    }
    let cw = (maxx - minx) as i64 as i32;
    let ch = (maxy - miny) as i64 as i32;
    if cw <= 0 || ch <= 0 {
        return Ok(());
    }
    let (cw, ch) = (cw as usize, ch as usize);
    let mut a = vec![0u8; cw * ch];
    let mut b = vec![0u8; cw * ch];
    let (tminx, tminy) = (minx as i64 as i32, miny as i64 as i32);
    let (tmaxx, tmaxy) = (maxx as i64 as i32, maxy as i64 as i32);
    let mut pts: Vec<(f32, f32)> = Vec::new();
    for x in tminx..tmaxx - 1 {
        for y in tminy..tmaxy - 1 {
            let (px, py) = (x as f32, y as f32);
            let dx = px - site.0;
            let dy = py - site.1;
            if 20.0f32 > (dx * dx + dy * dy).sqrt() && point_in_poly((px, py), poly) {
                pts.push((px, py));
            }
        }
    }
    let n = if mode != 0 { 1 } else { n };
    let set4 = |a: &mut Vec<u8>, p: (f32, f32)| {
        let lx = (p.0 - minx) as i64;
        let ly = (p.1 - miny) as i64;
        if lx >= 0 && ly >= 0 && (lx as usize) < cw && (ly as usize) < ch {
            a[ly as usize * cw + lx as usize] = 4;
        }
    };
    match mode {
        1 => set4(&mut a, nd.centroid),
        2 => set4(&mut a, site),
        3 => return Err("RunCA seed mode 3 (polygon outline, 0x14046d270) is not ported".into()),
        _ => {
            let k = (n.max(0) as usize).min(pts.len());
            grid::random_shuffle(&mut pts);
            for &p in pts.iter().take(k) {
                set4(&mut a, p);
            }
        }
    }
    let mut it = iterations as u32;
    loop {
        for x in 0..cw {
            for y in 0..ch {
                let mut v = a[y * cw + x];
                if v != 4 {
                    let mut mx = 0u8;
                    for nx in x as i64 - 1..=x as i64 + 1 {
                        for ny in y as i64 - 1..=y as i64 + 1 {
                            if (nx == x as i64 && ny == y as i64) || nx < 0 || ny < 0 || nx as usize >= cw || ny as usize >= ch {
                                continue;
                            }
                            mx = mx.max(a[ny as usize * cw + nx as usize]);
                        }
                    }
                    if mx > 2 {
                        v = (mx as u32 - pcg::boundedrand(mx as u32)) as u8;
                    }
                }
                b[y * cw + x] = v;
            }
        }
        std::mem::swap(&mut a, &mut b);
        it = it.wrapping_sub(1);
        if it == 0 {
            break;
        }
    }
    let w = t.w();
    for p in pts {
        let lx = p.0 - minx;
        let ly = p.1 - miny;
        if !(lx >= 0.0 && ly >= 0.0 && (lx as i64 as usize) < cw && (ly as i64 as usize) < ch) {
            continue;
        }
        let v = a[(ly as i64 as usize) * cw + lx as i64 as usize];
        if v != 0 && t.t.in_range(p.0 as i32, p.1 as i32) {
            let i = p.1 as usize * w + p.0 as usize;
            t.t.cells[i] = v as u16 + 1;
            let hi = t.d.hi(i) & 0xBF;
            t.d.set_hi(i, hi);
        }
    }
    Ok(())
}
