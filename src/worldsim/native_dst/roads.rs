//! DST road planner (0x140469d00, behind GetRoad / DrawRoads and the cave
//! branch of ForceConnectivity) and the DrawRoads rasteriser (0x1400e4980).
//! A delta against ../native/roads.rs, in the game's operation order.
//! All float arithmetic is f32 (each SSE op rounds).

use crate::worldsim::native::tiles::PERLIN_PERM;
use crate::worldsim::native_dst::grid::{tgm, Tiles, HI_ROAD};
use crate::worldsim::native_dst::islands::road_edges;
use crate::worldsim::native_dst::map::{DstMap, P2};

/// Hop distances (Johnson over unit weights); f64::MAX when unreachable.
fn apsp(m: &DstMap) -> Vec<Vec<f64>> {
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

/// 0x14043daf0: addss h,g; comiss.
fn pred(a: &ANode, b: &ANode) -> bool {
    (a.h + a.g) > (b.h + b.g)
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

/// VC9 x64 std::sort (0x140459ee0).
fn msvc_sort(a: &mut [ANode], mut first: usize, mut last: usize, mut ideal: usize) -> Result<(), String> {
    let mut count;
    loop {
        count = last - first;
        if !(32 < count && 0 < ideal) {
            break;
        }
        let (m1, m2) = unguarded_partition(a, first, last);
        ideal = (ideal >> 1) + (ideal >> 2);
        if m1 - first < last - m2 {
            msvc_sort(a, first, m1, ideal)?;
            first = m2;
        } else {
            msvc_sort(a, m2, last, ideal)?;
            last = m1;
        }
    }
    if 32 < count {
        return Err("A*: std::sort heap fallback (0x140443570) is not ported".into());
    } else if 1 < count {
        insertion_sort(a, first, last);
    }
    Ok(())
}

/// Klei A* (0x14045e030), cap 1000.
fn astar(m: &DstMap, d: &[Vec<f64>], start: usize, goal: usize, max_iter: usize) -> Result<Option<Vec<usize>>, String> {
    let mut nodes: Vec<ANode> = vec![ANode { parent: None, v: start, g: 0.0, h: d[start][goal] as f32 }];
    let mut open: Vec<usize> = vec![0];
    let mut closed: Vec<usize> = Vec::new();
    let mut resort = true;
    for _ in 0..max_iter {
        if open.is_empty() {
            return Ok(None);
        }
        if resort {
            let mut keys: Vec<ANode> = open.iter().map(|&i| ANode { parent: Some(i), ..nodes[i] }).collect();
            let len = keys.len();
            msvc_sort(&mut keys, 0, len, len)?;
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
            return Ok(Some(path));
        }
        for &nb in &m.out[node.v] {
            let cost = d[node.v][nb] as f32;
            if nb == node.v {
                continue;
            }
            if closed.iter().any(|&c| nodes[c].v == nb) {
                continue;
            }
            let g = cost + node.g;
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
    Ok(None)
}

/// AppendConnection 0x1404716a0(A, &pts, _, B): A's point by its internal
/// connection type (Cell+0x50), then, when B is one of A's landmass
/// neighbours (Cell+0x68 record kind 1), the midpoint of the last point and
/// B's site (B internal type 1) or B's centroid.
pub fn append_connection(m: &DstMap, a: usize, b: usize, pts: &mut Vec<P2>) -> Result<(), String> {
    let na = &m.nodes[a];
    let site = |n: usize| m.nodes[n].site.map(|s| (m.sites[s].x, m.sites[s].y)).ok_or("AppendConnection: cell without a site");
    match na.internal {
        1 => pts.push(site(a)?),
        2 | 5 => {}
        3 | 4 => return Err("AppendConnection: connection type 3/4 (LinkGeometry 0x14045ba80) is not ported".into()),
        _ => pts.push(na.centroid),
    }
    if na.lm_neighbours.contains(&b) {
        let last = *pts.last().ok_or("AppendConnection: midpoint after an empty list (reads before the vector in the game)")?;
        let q = if m.nodes[b].internal == 1 { site(b)? } else { m.nodes[b].centroid };
        pts.push(((q.0 + last.0) * 0.5, (q.1 + last.1) * 0.5));
    }
    Ok(())
}

fn site_p(m: &DstMap, v: usize) -> P2 {
    (m.sites[v].x, m.sites[v].y)
}

/// GetRoads (0x140469d00): cached after the first call (first caller's join wins).
pub fn get_roads(m: &mut DstMap, join: bool) -> Result<Vec<Vec<P2>>, String> {
    if let Some(r) = &m.roads {
        if !r.is_empty() {
            return Ok(r.clone());
        }
    }
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
        let d = apsp(m);
        let path = astar(m, &d, k, v, 1000)?.unwrap_or_default();
        let lm = |vx: usize| m.sites[vx].node.and_then(|nd| m.nodes[nd].landmass);
        if path.len() >= 2 && (join || lm(path[0]) == lm(*path.last().unwrap())) {
            for i in 0..path.len() - 1 {
                if m.has_edge(path[i], path[i + 1]) {
                    let a = m.sites[path[i]].node.ok_or("road vertex without a node")?;
                    let b = m.sites[path[i + 1]].node.ok_or("road vertex without a node")?;
                    append_connection(m, a, b, &mut road0)?;
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

/// 1D gradient noise (0x140136ba0), float products rounded per op.
fn noise1(x: f32) -> f32 {
    let mut i = x as i64;
    if !(x > 0.0) {
        i -= 1;
    }
    let f0 = x - i as f32;
    let f1 = f0 - 1.0;
    let g0 = PERLIN_PERM[(i & 255) as usize] & 15;
    let g1 = PERLIN_PERM[((i + 1) & 255) as usize] & 15;
    let grad = |g: u8| {
        let v = ((g & 7) + 1) as f32;
        if g & 8 != 0 { -v } else { v }
    };
    let t0 = 1.0 - f0 * f0;
    let t02 = t0 * t0;
    let t04 = t02 * t02;
    let t1 = 1.0 - f1 * f1;
    let t12 = t1 * t1;
    let t14 = t12 * t12;
    let n0 = (grad(g0) * t04) * f0;
    let n1 = (grad(g1) * t14) * f1;
    (n1 + n0) * 0.25
}

type Q = Vec<(f32, f32)>;

fn coeffs(q: &Q, s: usize, c: usize) -> (f32, f32, f32, f32) {
    let comp = |p: (f32, f32)| if c == 0 { p.0 } else { p.1 };
    let (p0, p1, p2, p3) = (comp(q[s]), comp(q[s + 1]), comp(q[s + 2]), comp(q[s + 3]));
    let c3 = p3 + ((-p0 + 3.0 * p1) - 3.0 * p2);
    let c2 = (((2.0 * p0) - (5.0 * p1)) + (4.0 * p2)) - p3;
    let c1 = p2 + -p0;
    (p1, c1, c2, c3)
}

fn eval_pos(q: &Q, s: usize, t: f32) -> (f32, f32) {
    let t2 = t * t;
    let t3 = t2 * t;
    let mut out = [0.0f32; 2];
    for (c, o) in out.iter_mut().enumerate() {
        let (p1, c1, c2, c3) = coeffs(q, s, c);
        *o = ((((2.0 * p1) + (c1 * t)) + (c2 * t2)) + (c3 * t3)) * 0.5;
    }
    (out[0], out[1])
}

fn eval_tan(q: &Q, s: usize, t: f32) -> (f32, f32) {
    let t2 = t * t;
    let k3 = t2 * 1.5;
    let mut tan = [0.0f32; 2];
    for (c, o) in tan.iter_mut().enumerate() {
        let (_, c1, c2, c3) = coeffs(q, s, c);
        *o = (c1 + (c2 * t)) + (c3 * k3);
    }
    (tan[0], tan[1])
}

/// DDA 0x1400e27a0: road flag on every cell; impassable ids become the road tile.
fn dda(t: &mut Tiles, x0: f32, y0: f32, x1: f32, y1: f32, tile: u16) -> Result<(), String> {
    let adx = (x1 - x0).abs();
    let ady = (y1 - y0).abs();
    let fx0 = x0.floor();
    let fy0 = y0.floor();
    let (mut x, mut y) = (fx0 as i64, fy0 as i64);
    let mut n: i64 = 1;
    let (xi, yi);
    let mut err: f32;
    if adx == 0.0 {
        xi = 0;
        err = f32::INFINITY;
    } else if x1 > x0 {
        xi = 1;
        n = x1.floor() as i64 - x + 1;
        err = ((fx0 + 1.0) - x0) * ady;
    } else {
        xi = -1;
        n = x - x1.floor() as i64 + 1;
        err = (x0 - fx0) * ady;
    }
    if ady == 0.0 {
        yi = 0;
        err = err + (0.0 - f32::INFINITY);
    } else if y1 > y0 {
        yi = 1;
        n += y1.floor() as i64 - y;
        err = err + (0.0 - ((fy0 + 1.0) - y0) * adx);
    } else {
        yi = -1;
        n += y - y1.floor() as i64;
        err = err + (0.0 - (y0 - fy0) * adx);
    }
    let w = t.w() as i64;
    while n > 0 {
        let idx = w * y + x;
        if idx < 0 || idx as usize >= t.t.cells.len() {
            return Err("DrawRoads: DDA left the tile grid (unchecked in the game)".into());
        }
        let i = idx as usize;
        let hi = t.d.hi(i) | HI_ROAD;
        t.d.set_hi(i, hi);
        if tgm::is_impassable(t.t.cells[i]) {
            t.t.cells[i] = tile;
        }
        if err > 0.0 {
            y += yi;
            err = err + (0.0 - adx);
        } else {
            x += xi;
            err = err + ady;
        }
        n -= 1;
    }
    Ok(())
}

/// SetRoadParameters (0x140129a40).
#[derive(Clone, Copy, Default, Debug)]
pub struct RoadParams {
    pub nsub: i32,
    pub min_w: f32,
    pub max_w: f32,
    pub min_e: f32,
    pub max_e: f32,
    pub jitter: f32,
}

fn raster_road(t: &mut Tiles, q: &Q, p: &RoadParams, tile: u16) -> Result<(), String> {
    let nseg = q.len() - 3;
    let er = p.max_e - p.min_e;
    let wr = p.max_w - p.min_w;
    let (w, h) = (t.w() as f32, t.h() as f32);
    let ox = ((w * -0.5) * 4.0) - 2.0;
    let oy = ((h * -0.5) * 4.0) - 2.0;
    let to_tile = |pt: (f32, f32)| ((pt.0 - ox) * 0.25, (pt.1 - oy) * 0.25);
    let norm = |tn: (f32, f32)| {
        let len = (tn.0 * tn.0 + tn.1 * tn.1).sqrt();
        (tn.0 / len, tn.1 / len)
    };
    let width = |n: f32| ((n * wr) + p.min_w) + ((n * er) + p.min_e);
    for seg in 0..nseg {
        let pos = eval_pos(q, seg, 0.0);
        let nn = norm(eval_tan(q, seg, 0.0));
        let (mut prev_pos, mut prev_perp) = (pos, (nn.1, -0.0 - nn.0));
        let end = eval_pos(q, seg, 1.0);
        let dx = end.0 - pos.0;
        let dy = end.1 - pos.1;
        let chord = (dx * dx + dy * dy).sqrt();
        let dt = 1.0 / (chord * 0.25);
        let sf = seg as f32 + 0.0;
        let mut w0 = width(noise1(sf + 0.0));
        let mut tt = if 1.0 > dt { dt } else { 1.0 };
        loop {
            let p1 = eval_pos(q, seg, tt);
            let nn = norm(eval_tan(q, seg, tt));
            let pp = (nn.1, -0.0 - nn.0);
            let w1 = width(noise1((tt * 5.0) + sf));
            let a = (prev_pos.0 + prev_perp.0 * w0, prev_pos.1 + prev_perp.1 * w0);
            let b = (prev_pos.0 + prev_perp.0 * (-0.0 - w0), prev_pos.1 + prev_perp.1 * (-0.0 - w0));
            let c = (p1.0 + pp.0 * w1, p1.1 + pp.1 * w1);
            let d = (p1.0 + pp.0 * (-0.0 - w1), p1.1 + pp.1 * (-0.0 - w1));
            let (ta, tb, tc, td) = (to_tile(a), to_tile(b), to_tile(c), to_tile(d));
            dda(t, ta.0, ta.1, tb.0, tb.1, tile)?;
            dda(t, tc.0, tc.1, td.0, td.1, tile)?;
            dda(t, ta.0, ta.1, tc.0, tc.1, tile)?;
            dda(t, tb.0, tb.1, td.0, td.1, tile)?;
            if tt == 1.0 {
                break;
            }
            let mut tn = dt + tt;
            tn = if 1.0 > tn { tn } else { 1.0 };
            let pt = eval_pos(q, seg, tt);
            let mut pn = eval_pos(q, seg, tn);
            loop {
                let vy = pt.1 - pn.1;
                let vx = pt.0 - pn.0;
                if !((vy * vy + vx * vx) > 16.0) {
                    break;
                }
                tn = (tn + tt) * 0.5;
                pn = eval_pos(q, seg, tn);
            }
            prev_pos = p1;
            prev_perp = pp;
            w0 = w1;
            tt = tn;
        }
    }
    Ok(())
}

/// DrawRoads (0x14012a800 → 0x14046aa40).
pub fn draw_roads(m: &mut DstMap, t: &mut Tiles, p: &RoadParams, join: bool, tile: u16) -> Result<(), String> {
    let roads = get_roads(m, join)?;
    let (w, h) = (m.width as i64 as i32 as f32, m.height as i64 as i32 as f32);
    for r in roads {
        if r.len() < 2 {
            continue;
        }
        let cp: Q = r
            .iter()
            .map(|&(x, y)| {
                let ax = ((x - w * 0.5) * 4.0) * 10.0;
                let ay = ((y - h * 0.5) * 4.0) * 10.0;
                (ax.floor() / 10.0, ay.floor() / 10.0)
            })
            .collect();
        let mut q = Vec::with_capacity(cp.len() + 2);
        q.push(cp[0]);
        q.extend_from_slice(&cp);
        q.push(*cp.last().unwrap());
        raster_road(t, &q, p, tile)?;
    }
    Ok(())
}
