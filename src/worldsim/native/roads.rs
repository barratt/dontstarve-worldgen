//! Road planning (BoostMap::GetRoads 0x649B60) and DrawRoads
//! (RoadBuilder 0x486750), in the game's operation order; `fr` marks each
//! float32 store.

use crate::worldsim::native::boostmap::{BoostMap, P2};
use crate::worldsim::native::islands::road_edges;
use crate::worldsim::native::tiles::{TileGrid, FLAG_ROAD, PERLIN_PERM};

fn fr(x: f64) -> f64 {
    x as f32 as f64
}

/// Hop distances (Johnson over unit weights); f64::MAX when unreachable.
fn apsp(m: &BoostMap) -> Vec<Vec<f64>> {
    let n = m.vertex_names.len();
    (0..n)
        .map(|s| {
            let mut d = vec![f64::MAX; n];
            d[s] = 0.0;
            let mut q = vec![s];
            let mut i = 0;
            while i < q.len() {
                let u = q[i];
                i += 1;
                for &v in &m.out[u] {
                    if d[v] == f64::MAX {
                        d[v] = d[u] + 1.0;
                        q.push(v);
                    }
                }
            }
            d
        })
        .collect()
}

#[derive(Clone, Copy)]
struct ANode {
    parent: Option<usize>,
    v: usize,
    g: f32,
    h: f32,
}

fn pred(a: &ANode, b: &ANode) -> bool {
    (a.h as f64 + a.g as f64) > (b.h as f64 + b.g as f64)
}

fn insertion_sort(a: &mut [ANode], lo: usize, hi: usize) {
    for nx in lo + 1..hi {
        let v = a[nx];
        if pred(&v, &a[lo]) {
            for k in (lo + 1..=nx).rev() {
                a[k] = a[k - 1];
            }
            a[lo] = v;
        } else {
            let mut h = nx;
            while pred(&v, &a[h - 1]) {
                a[h] = a[h - 1];
                h -= 1;
            }
            a[h] = v;
        }
    }
}

fn med3(a: &mut [ANode], x: usize, y: usize, z: usize) {
    if pred(&a[y], &a[x]) {
        a.swap(y, x);
    }
    if pred(&a[z], &a[y]) {
        a.swap(z, y);
    }
    if pred(&a[y], &a[x]) {
        a.swap(y, x);
    }
}

fn median(a: &mut [ANode], first: usize, mid: usize, last: usize) {
    if 40 < last - first {
        let step = (last - first + 1) / 8;
        med3(a, first, first + step, first + 2 * step);
        med3(a, mid - step, mid, mid + step);
        med3(a, last - 2 * step, last - step, last);
        med3(a, first + step, mid, last - step);
    } else {
        med3(a, first, mid, last);
    }
}

fn unguarded_partition(a: &mut [ANode], first: usize, last: usize) -> (usize, usize) {
    let mid = first + (last - first) / 2;
    median(a, first, mid, last - 1);
    let (mut pf, mut pl) = (mid, mid + 1);
    while first < pf && !pred(&a[pf - 1], &a[pf]) && !pred(&a[pf], &a[pf - 1]) {
        pf -= 1;
    }
    while pl < last && !pred(&a[pl], &a[pf]) && !pred(&a[pf], &a[pl]) {
        pl += 1;
    }
    let (mut gf, mut gl) = (pl, pf);
    loop {
        while gf < last {
            if pred(&a[pf], &a[gf]) {
            } else if pred(&a[gf], &a[pf]) {
                break;
            } else {
                a.swap(pl, gf);
                pl += 1;
            }
            gf += 1;
        }
        while first < gl {
            if pred(&a[gl - 1], &a[pf]) {
            } else if pred(&a[pf], &a[gl - 1]) {
                break;
            } else {
                pf -= 1;
                a.swap(pf, gl - 1);
            }
            gl -= 1;
        }
        if gl == first && gf == last {
            return (pf, pl);
        }
        if gl == first {
            if pl != gf {
                a.swap(pf, pl);
            }
            pl += 1;
            a.swap(pf, gf);
            pf += 1;
            gf += 1;
        } else if gf == last {
            gl -= 1;
            pf -= 1;
            if gl != pf {
                a.swap(gl, pf);
            }
            pl -= 1;
            a.swap(pf, pl);
        } else {
            gl -= 1;
            a.swap(gf, gl);
            gf += 1;
        }
    }
}

/// MSVC 2008 std::sort (_Sort): introsort, insertion sort at <= 32,
/// heap sort fallback.
fn msvc_sort(a: &mut [ANode], mut first: usize, mut last: usize, mut ideal: usize) {
    let mut count;
    loop {
        count = last - first;
        if !(32 < count && 0 < ideal) {
            break;
        }
        let (m1, m2) = unguarded_partition(a, first, last);
        ideal = (ideal >> 1) + (ideal >> 2);
        if m1 - first < last - m2 {
            msvc_sort(a, first, m1, ideal);
            first = m2;
        } else {
            msvc_sort(a, m2, last, ideal);
            last = m1;
        }
    }
    if 32 < count {
        // Heap fallback: never reached in observed worlds; a stable order
        // stands in until a world exercises it.
        a[first..last].sort_by(|x, y| if pred(x, y) { std::cmp::Ordering::Less } else if pred(y, x) { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal });
    } else if 1 < count {
        insertion_sort(a, first, last);
    }
}

/// Klei A* (init 0x63D240, run 0x640A10, successors 0x63E500).
fn astar(m: &BoostMap, d: &[Vec<f64>], start: usize, goal: usize, max_iter: usize) -> Option<Vec<usize>> {
    let mut nodes: Vec<ANode> = vec![ANode { parent: None, v: start, g: 0.0, h: d[start][goal] as f32 }];
    let mut open: Vec<usize> = vec![0];
    let mut closed: Vec<usize> = Vec::new();
    let mut resort = true;
    for _ in 0..max_iter {
        if open.is_empty() {
            return None;
        }
        if resort {
            // Sort copies of the open nodes; each copy carries its handle in
            // `parent` (pred only reads g and h).
            let mut keys: Vec<ANode> = open
                .iter()
                .map(|&i| ANode { parent: Some(i), ..nodes[i] })
                .collect();
            let len = keys.len();
            msvc_sort(&mut keys, 0, len, len);
            open = keys.iter().map(|k| k.parent.unwrap()).collect();
        }
        let ni = open.pop().unwrap();
        closed.push(ni);
        resort = false;
        let node = nodes[ni];
        if node.v == goal {
            let mut path = Vec::new();
            let mut cur = Some(ni);
            while let Some(c) = cur {
                path.push(nodes[c].v);
                cur = nodes[c].parent;
            }
            return Some(path);
        }
        for &nb in &m.out[node.v] {
            let cost = d[node.v][nb] as f32;
            if nb == node.v {
                continue;
            }
            if closed.iter().any(|&c| nodes[c].v == nb) {
                continue;
            }
            let g = fr(cost as f64 + node.g as f64) as f32;
            match open.iter().copied().find(|&o| nodes[o].v == nb) {
                None => {
                    nodes.push(ANode { parent: Some(ni), v: nb, g, h: d[nb][goal] as f32 });
                    open.push(nodes.len() - 1);
                    resort = true;
                }
                Some(o) if nodes[o].g > g => {
                    nodes[o].parent = Some(ni);
                    nodes[o].h = d[nb][goal] as f32;
                    nodes[o].g = g;
                    resort = true;
                }
                _ => {}
            }
        }
    }
    None
}

fn site_p(m: &BoostMap, v: usize) -> P2 {
    (m.sites[v].x, m.sites[v].y)
}

/// GetRoads (0x649B60): cached after the first call.
pub fn get_roads(m: &mut BoostMap, join: bool) -> Result<Vec<Vec<P2>>, String> {
    if let Some(r) = &m.roads {
        return Ok(r.clone());
    }
    let d = apsp(m);
    let n = m.vertex_names.len();
    let (mut k, mut v, mut fk, mut fv) = (0, 0, false, false);
    for i in 0..n {
        if fk && fv {
            break;
        }
        if m.vertex_names[i].contains("PigKingdom") {
            k = i;
            fk = true;
        } else if m.vertex_names[i].contains("PigVillage") {
            v = i;
            fv = true;
        }
    }
    let mut roads: Vec<Vec<P2>> = Vec::new();
    let mut road0: Vec<P2> = Vec::new();
    if fk && fv {
        let path = astar(m, &d, k, v, 1000).unwrap_or_default();
        let lm = |vx: usize| m.sites[vx].node.and_then(|nd| m.nodes[nd].landmass);
        if path.len() >= 2 && (join || lm(path[0]) == lm(*path.last().unwrap())) {
            for i in 0..path.len() - 1 {
                if m.has_edge(path[i], path[i + 1]) {
                    let a = m.sites[path[i]].node.ok_or("road vertex without a node")?;
                    match m.nodes[a].internal {
                        1 => road0.push(site_p(m, path[i])),
                        2 | 5 => {}
                        0 => road0.push(m.nodes[a].centroid),
                        t => return Err(format!("road connection type {t} (LinkGeometry 0x63EC10) is not ported")),
                    }
                }
            }
            road0.push(site_p(m, *path.last().unwrap()));
            roads.push(road0.clone());
        }
    }
    let mut segs: Vec<Vec<P2>> = Vec::new();
    for (a, b) in road_edges(m, join) {
        if (m.nodes[a].site_flags | m.nodes[b].site_flags) & 4 != 0 {
            continue;
        }
        let (Some(sa), Some(sb)) = (m.nodes[a].site, m.nodes[b].site) else { continue };
        let (pa, pb) = ((m.sites[sa].x, m.sites[sa].y), (m.sites[sb].x, m.sites[sb].y));
        let in_road0 = road0.windows(2).any(|w| (w[0] == pa && w[1] == pb) || (w[0] == pb && w[1] == pa));
        if !in_road0 {
            segs.push(vec![pa, pb]);
        }
    }
    let mut merged: Vec<Vec<P2>> = Vec::new();
    for s in segs {
        let (sf, sl) = (s[0], *s.last().unwrap());
        let mut done = false;
        for mi in 0..merged.len() {
            let (mf, ml) = (merged[mi][0], *merged[mi].last().unwrap());
            if sf == ml {
                merged[mi].extend_from_slice(&s[1..]);
                done = true;
                break;
            }
            if sl == mf {
                let mut joined = s.clone();
                joined.extend_from_slice(&merged[mi][1..]);
                merged[mi] = joined;
                done = true;
                break;
            }
        }
        if !done {
            merged.push(s);
        }
    }
    roads.extend(merged);
    m.roads = Some(roads.clone());
    Ok(roads)
}

/// 1D gradient noise (0x560260).
fn noise1(x: f64) -> f64 {
    let mut i = x.trunc() as i64;
    if !(x > 0.0) {
        i -= 1;
    }
    let f0 = fr(x - i as f64);
    let f1 = fr(f0 - 1.0);
    let g0 = PERLIN_PERM[(i & 255) as usize] & 15;
    let g1 = PERLIN_PERM[((i + 1) & 255) as usize] & 15;
    let t0 = fr(1.0 - fr(f0 * f0));
    let t02 = fr(t0 * t0);
    let t04 = fr(t02 * t02);
    let grad = |g: u8| {
        let v = fr(((g & 7) + 1) as f64);
        if g & 8 != 0 { -v } else { v }
    };
    let n0 = fr(grad(g0) * t04 * f0);
    let t1 = fr(1.0 - fr(f1 * f1));
    let t12 = fr(t1 * t1);
    let t14 = fr(t12 * t12);
    let n1 = fr(f1 * (grad(g1) * t14));
    fr((n1 + n0) * 0.25)
}

type Q = Vec<(f64, f64)>;

fn coeffs(q: &Q, s: usize, c: usize) -> (f64, f64, f64, f64) {
    let comp = |p: (f64, f64)| if c == 0 { p.0 } else { p.1 };
    let (p0, p1, p2, p3) = (comp(q[s]), comp(q[s + 1]), comp(q[s + 2]), comp(q[s + 3]));
    let c3 = fr(p3 + fr(fr(-p0 + fr(3.0 * p1)) - fr(3.0 * p2)));
    let c2 = fr(fr(fr(fr(2.0 * p0) - fr(5.0 * p1)) + fr(4.0 * p2)) - p3);
    let c1 = fr(p2 + -p0);
    (p1, c1, c2, c3)
}

fn eval_pos(q: &Q, s: usize, t: f64) -> (f64, f64) {
    let t2 = fr(t * t);
    let t3 = fr(t * t * t);
    let mut out = [0.0; 2];
    for (c, o) in out.iter_mut().enumerate() {
        let (p1, c1, c2, c3) = coeffs(q, s, c);
        *o = fr(fr(fr(fr(fr(2.0 * p1) + fr(c1 * t)) + fr(c2 * t2)) + fr(c3 * t3)) * 0.5);
    }
    (out[0], out[1])
}

fn eval_pos_tan(q: &Q, s: usize, t: f64) -> ((f64, f64), (f64, f64)) {
    let t2 = fr(t * t);
    let k3 = fr(t2 * 1.5);
    let mut tan = [0.0; 2];
    for (c, o) in tan.iter_mut().enumerate() {
        let (_, c1, c2, c3) = coeffs(q, s, c);
        *o = fr(fr(c1 + fr(c2 * t)) + fr(c3 * k3));
    }
    (eval_pos(q, s, t), (tan[0], tan[1]))
}

/// DDA (0x484D10): flag cells along a segment; impassable becomes dirt.
fn dda(g: &mut TileGrid, x0: f64, y0: f64, x1: f64, y1: f64) -> Result<(), String> {
    let adx = fr(fr(x1 - x0).abs());
    let ady = fr(fr(y1 - y0).abs());
    let fx0 = fr(x0.floor());
    let fy0 = fr(y0.floor());
    let (mut x, mut y) = (fx0 as i64, fy0 as i64);
    let mut n: i64 = 1;
    let (xi, yi);
    let mut err;
    if adx == 0.0 {
        xi = 0;
        err = f64::INFINITY;
    } else if x1 > x0 {
        xi = 1;
        n += fr(x1.floor()) as i64 - x;
        err = fr((fx0 + 1.0 - x0) * ady);
    } else {
        xi = -1;
        n += x - fr(x1.floor()) as i64;
        err = fr((x0 - fx0) * ady);
    }
    if ady == 0.0 {
        yi = 0;
        err = fr(err - f64::INFINITY);
    } else if y1 > y0 {
        yi = 1;
        n += fr(y1.floor()) as i64 - y;
        err = fr(err - (fy0 + 1.0 - y0) * adx);
    } else {
        yi = -1;
        n += y - fr(y1.floor()) as i64;
        err = fr(err - (y0 - fy0) * adx);
    }
    while n > 0 {
        let idx = g.w as i64 * y + x;
        if idx < 0 || idx as usize >= g.cells.len() {
            return Err("DrawRoads: DDA left the tile grid".into());
        }
        let c = &mut g.cells[idx as usize];
        c[1] |= FLAG_ROAD;
        if c[0] == 1 {
            c[0] = 4;
        }
        if err > 0.0 {
            y += yi;
            err = fr(err - adx);
        } else {
            x += xi;
            err = fr(err + ady);
        }
        n -= 1;
    }
    Ok(())
}

/// Road parameters (SetRoadParameters 0x4A4880).
#[derive(Clone, Copy, Default, Debug)]
pub struct RoadParams {
    pub nsub: i32,
    pub min_w: f32,
    pub max_w: f32,
    pub min_e: f32,
    pub max_e: f32,
    pub jitter: f32,
}

fn raster_road(g: &mut TileGrid, q: &Q, p: &RoadParams) -> Result<(), String> {
    let nseg = q.len() - 3;
    let (minw, maxw, mine, maxe) = (p.min_w as f64, p.max_w as f64, p.min_e as f64, p.max_e as f64);
    let er = maxe - mine;
    let wr = maxw - minw;
    let s = 4.0;
    let ox = fr(-0.5 * g.w as f64 * s - 0.5 * s + 0.0);
    let oy = fr(-0.5 * g.h as f64 * s - 0.5 * s + 0.0);
    let to_tile = |pt: (f64, f64)| (fr(fr(pt.0 - ox) / s), fr(fr(pt.1 - oy) / s));
    for seg in 0..nseg {
        let (pos, tan) = eval_pos_tan(q, seg, 0.0);
        let mut len = fr(fr(tan.0 * tan.0 + tan.1 * tan.1).sqrt());
        let (mut nx, mut ny) = (fr(tan.0 / len), fr(tan.1 / len));
        let perp = (ny, -nx);
        let end = eval_pos(q, seg, 1.0);
        let dx = fr(end.0 - pos.0);
        let dy = fr(end.1 - pos.1);
        let chord = fr(fr(dx * dx + dy * dy).sqrt());
        let dt = fr(1.0 / (chord * 0.25));
        let (mut prev_pos, mut prev_perp) = (pos, perp);
        let n0 = noise1(fr(seg as f64 + 0.0 + 0.0));
        let mut w0 = fr((minw + wr * n0) + fr(mine + er * n0));
        let mut t = if 1.0 > dt { dt } else { 1.0 };
        loop {
            let (p1, tn1) = eval_pos_tan(q, seg, t);
            len = fr(fr(tn1.0 * tn1.0 + tn1.1 * tn1.1).sqrt());
            nx = fr(tn1.0 / len);
            ny = fr(tn1.1 / len);
            let pp = (ny, -nx);
            let n1 = noise1(fr(t * 5.0 + seg as f64));
            let w1 = fr((wr * n1 + minw) + fr(mine + er * n1));
            let a = (fr(prev_pos.0 + fr(prev_perp.0 * w0)), fr(prev_pos.1 + fr(prev_perp.1 * w0)));
            let b = (fr(prev_pos.0 + fr(prev_perp.0 * -w0)), fr(prev_pos.1 + fr(prev_perp.1 * -w0)));
            let c = (fr(p1.0 + fr(pp.0 * w1)), fr(p1.1 + fr(pp.1 * w1)));
            let dd = (fr(p1.0 + fr(pp.0 * -w1)), fr(p1.1 + fr(pp.1 * -w1)));
            let (ta, tb, tc, td) = (to_tile(a), to_tile(b), to_tile(c), to_tile(dd));
            dda(g, ta.0, ta.1, tb.0, tb.1)?;
            dda(g, tc.0, tc.1, td.0, td.1)?;
            dda(g, ta.0, ta.1, tc.0, tc.1)?;
            dda(g, tb.0, tb.1, td.0, td.1)?;
            if t == 1.0 {
                break;
            }
            let mut tn = fr(t + dt);
            tn = if 1.0 > tn { tn } else { 1.0 };
            let pt = eval_pos(q, seg, t);
            let mut pn = eval_pos(q, seg, tn);
            loop {
                let vx = fr(pt.0 - pn.0);
                let vy = fr(pt.1 - pn.1);
                let d2 = fr(vx * vx + vy * vy);
                if !(d2 > 16.0) {
                    break;
                }
                tn = fr((tn + t) * 0.5);
                if !(tn > t) {
                    return Err("DrawRoads: assert new_t > t".into());
                }
                pn = eval_pos(q, seg, tn);
            }
            prev_pos = p1;
            prev_perp = pp;
            w0 = w1;
            t = tn;
        }
    }
    Ok(())
}

/// DrawRoads (0x4A6370 → 0x64A350).
pub fn draw_roads(m: &mut BoostMap, g: &mut TileGrid, p: &RoadParams, join: bool) -> Result<(), String> {
    let roads = get_roads(m, join)?;
    let (w, h) = (g.w as f64, g.h as f64);
    for r in roads {
        if r.len() < 2 {
            continue;
        }
        let cp: Q = r
            .iter()
            .map(|&(x, y)| {
                let ax = fr((x as f64 - w * 0.5) * 4.0 * 10.0);
                let ay = fr((y as f64 - h * 0.5) * 4.0 * 10.0);
                (fr(fr(ax.floor()) / 10.0), fr(fr(ay.floor()) / 10.0))
            })
            .collect();
        let mut q = Vec::with_capacity(cp.len() + 2);
        q.push(cp[0]);
        q.extend_from_slice(&cp);
        q.push(*cp.last().unwrap());
        raster_road(g, &q, p)?;
    }
    Ok(())
}
