//! DST SeparateIslands, GenerateLandmasses / MergeConnectedLandmasses, the
//! rewritten ForceConnectivity core (land bridges, corridors, border ring),
//! the disconnect natives and the wormhole pickers, as a delta against
//! ../native/islands.rs. Every float op is an SSE single-precision op.

use std::collections::BTreeMap;

use crate::pcg;
use crate::worldsim::native_dst::grid::{self, tgm, Grid, Tiles, HI_ROAD};
use crate::worldsim::native_dst::map::{DstMap, Landmass, P2, TYPE_BLANK, TYPE_BLOCKER, TYPE_RANDOM};

const FLAG_FORCE_CONNECTED: i32 = 2;
const FLAG_ROAD_POISON: i32 = 4;

/// Types 5 (Room), 6 (BackgroundRoom), 7 (SeparatedRoom).
pub fn room_type(k: i32) -> bool {
    (5..=7).contains(&k)
}

/// The ValidateGround callback (0x140129600) as a closure over the Lua function.
pub type Validate<'a> = dyn FnMut(u16) -> Result<u16, String> + 'a;

/// Family (0x140471630).
fn family(m: &DstMap, x: usize, y: usize) -> bool {
    let (nx, ny) = (&m.nodes[x], &m.nodes[y]);
    if nx.blocked || ny.blocked {
        return false;
    }
    let (xp, yp) = (nx.parent, ny.parent);
    let pp = |p: Option<usize>| p.and_then(|p| m.nodes[p].parent);
    yp == xp
        || yp == Some(x)
        || xp == Some(y)
        || (yp.is_some() && pp(yp) == Some(x))
        || (xp.is_some() && pp(xp) == Some(y))
        || (yp.is_some() && pp(yp) == xp)
        || (xp.is_some() && pp(xp) == yp)
        || (yp.is_some() && xp.is_some() && pp(yp) == pp(xp) && pp(yp).is_some())
}

fn vertex_of(m: &DstMap, node: usize) -> Option<usize> {
    m.vertex_by_name.get(&m.nodes[node].name).copied()
}

pub fn node_of_vertex(m: &DstMap, v: usize) -> Option<usize> {
    m.sites.get(v).and_then(|s| s.node)
}

fn related(m: &DstMap, a: usize, b: usize) -> bool {
    if family(m, a, b) {
        return true;
    }
    let (Some(va), Some(vb)) = (vertex_of(m, a), vertex_of(m, b)) else { return false };
    for &w in &m.out[va] {
        if let Some(cw) = node_of_vertex(m, w) {
            if family(m, b, cw) {
                return true;
            }
        }
    }
    for &w in &m.out[vb] {
        if m.out[va].contains(&w) {
            return true;
        }
        if let Some(cw) = node_of_vertex(m, w) {
            if family(m, a, cw) {
                return true;
            }
        }
    }
    let site_vertex = |n: Option<usize>| n.filter(|&p| m.nodes[p].site.is_some()).and_then(|p| vertex_of(m, p));
    if let Some(bp) = site_vertex(m.nodes[b].parent) {
        if m.has_edge(va, bp) {
            return true;
        }
    }
    if let Some(ap) = site_vertex(m.nodes[a].parent) {
        if m.has_edge(ap, vb) {
            return true;
        }
    }
    false
}

fn linked(m: &DstMap, a: usize, b: usize) -> bool {
    match (vertex_of(m, a), vertex_of(m, b)) {
        (Some(u), Some(v)) => m.has_edge(u, v),
        _ => false,
    }
}

/// sqrtf(f(f(dy*dy) + f(dx*dx))) on float points.
fn float_len(p: P2, q: P2) -> f32 {
    let dx = p.0 - q.0;
    let dy = p.1 - q.1;
    (dy * dy + dx * dx).sqrt()
}

fn site_xy(m: &DstMap, node: usize) -> P2 {
    let s = &m.sites[m.nodes[node].site.unwrap()];
    (s.x, s.y)
}

fn edge_float_ends(m: &DstMap, e: usize) -> Option<(P2, P2)> {
    let edge = m.diagram.edges[e];
    let twin = m.diagram.edges[edge.twin as usize];
    if edge.vertex0 < 0 || twin.vertex0 < 0 {
        return None;
    }
    let a = m.diagram.vertices[edge.vertex0 as usize];
    let b = m.diagram.vertices[twin.vertex0 as usize];
    Some(((a.x as f32, a.y as f32), (b.x as f32, b.y as f32)))
}

/// PaintEdge (0x14045f8b0): five lines per sample pair, ±1 added in double.
fn paint_edge(m: &DstMap, t: &mut Tiles, e: usize, tile: u16) {
    let Some(pts) = m.edge_samples(e) else { return };
    for k in 0..pts.len().saturating_sub(1) {
        let (p, q) = (pts[k], pts[k + 1]);
        for (ox, oy) in [(0.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            grid::line(t, (p.0 + ox) as f32, (p.1 + oy) as f32, (q.0 + ox) as f32, (q.1 + oy) as f32, tile, false, false);
        }
    }
}

/// SeparateIslands (0x140464640).
pub fn separate_islands(m: &mut DstMap, t: &mut Tiles, validate: &mut Validate) -> Result<(), String> {
    let nsites = m.sites.len();
    let tile = m.default_tile;
    for ci in 0..m.diagram.cells.len() {
        let vc = m.diagram.cells[ci];
        let idx = vc.source_index as usize;
        if idx >= nsites {
            continue;
        }
        let Some(a) = m.sites[idx].node else { continue };
        let ka = m.nodes[a].kind;
        if ka == TYPE_RANDOM || ka == TYPE_BLANK || vc.incident_edge < 0 {
            continue;
        }
        let start = vc.incident_edge as usize;
        let mut e = start;
        loop {
            let nb = m.twin_site(e);
            let mut connected = false;
            let mut b_room = false;
            let mut skip = false;
            if nb < nsites {
                let b = m.sites[nb].node.unwrap();
                let kb = m.nodes[b].kind;
                if kb == TYPE_RANDOM || kb == TYPE_BLANK {
                    skip = true;
                } else {
                    b_room = room_type(kb);
                    let lk = linked(m, a, b);
                    let elen = edge_float_ends(m, e).map(|(p, q)| float_len(p, q)).unwrap_or(0.0);
                    let bl = related(m, a, b);
                    connected = lk && (elen > 4.0 || bl);
                    if ka != TYPE_BLOCKER && kb != TYPE_BLOCKER {
                        connected = connected || bl;
                    }
                    if float_len(site_xy(m, b), site_xy(m, a)) > 40.0 {
                        connected = false;
                    }
                }
            }
            if !skip {
                if room_type(ka) || b_room {
                    let _unused = validate(m.nodes[a].value)?;
                    paint_edge(m, t, e, tile);
                } else if connected || m.nodes[a].site_flags & FLAG_FORCE_CONNECTED != 0 {
                    m.nodes[a].connected_mark = true;
                } else {
                    paint_edge(m, t, e, tile);
                }
            }
            e = m.diagram.edges[e].next as usize;
            if e == start {
                break;
            }
        }
    }
    Ok(())
}

fn task_key(name: &str) -> String {
    name.split(':').next().unwrap_or("").to_string()
}

/// GenerateLandmasses (0x140465a60) + flood (0x140465040).
pub fn generate_landmasses(m: &mut DstMap) {
    let nsites = m.sites.len();
    let mut seeds = Vec::new();
    for ci in 0..m.diagram.cells.len() {
        let idx = m.diagram.cells[ci].source_index as usize;
        if idx >= nsites {
            continue;
        }
        let Some(a) = m.sites[idx].node else { continue };
        m.nodes[a].lm_neighbours.clear();
        let k = m.nodes[a].kind;
        let parent_blank = m.nodes[a].parent.is_some_and(|p| m.nodes[p].kind == TYPE_BLANK);
        if k == TYPE_RANDOM || k == TYPE_BLANK || parent_blank {
            m.nodes[a].visited = true;
        } else {
            m.nodes[a].visited = false;
            seeds.push(ci);
        }
    }
    for ci in seeds {
        let mut map: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        flood(m, ci, &mut map);
        if map.is_empty() {
            continue;
        }
        let existing = map.values().flatten().find_map(|&c| m.nodes[c].landmass);
        let li = match existing {
            Some(l) => l,
            None => {
                m.landmasses.push(Landmass::default());
                m.landmasses.len() - 1
            }
        };
        m.landmasses[li].map.clear();
        for (key, cells) in map {
            for &c in &cells {
                m.nodes[c].landmass = Some(li);
            }
            m.landmasses[li].map.entry(key).or_default().extend(cells);
        }
    }
}

fn flood(m: &mut DstMap, ci: usize, map: &mut BTreeMap<String, Vec<usize>>) {
    let nsites = m.sites.len();
    let idx = m.diagram.cells[ci].source_index as usize;
    let a = m.sites[idx].node.unwrap();
    if m.nodes[a].visited {
        return;
    }
    m.nodes[a].visited = true;
    map.entry(task_key(&m.nodes[a].name)).or_default().push(a);
    let Ok(start) = usize::try_from(m.diagram.cells[ci].incident_edge) else { return };
    let mut e = start;
    loop {
        let nb = m.twin_site(e);
        if nb < nsites {
            let b = m.sites[nb].node.unwrap();
            let kb = m.nodes[b].kind;
            if kb != TYPE_RANDOM && kb != TYPE_BLANK {
                let lk = linked(m, a, b);
                if lk || !room_type(kb) {
                    let bl = related(m, a, b);
                    let elen = edge_float_ends(m, e).map(|(p, q)| float_len(p, q)).unwrap_or(0.0);
                    let sd = float_len(site_xy(m, b), site_xy(m, a));
                    if bl || (!(elen < 4.0) && !(sd > 40.0)) {
                        m.nodes[a].lm_neighbours.push(b);
                        if !m.nodes[b].visited && (lk || bl) {
                            let twin_cell = m.diagram.edges[m.diagram.edges[e].twin as usize].cell as usize;
                            flood(m, twin_cell, map);
                        }
                    }
                }
            }
        }
        e = m.diagram.edges[e].next as usize;
        if e == start {
            break;
        }
    }
}

/// RoadEdges (0x14045d7c0): both types != 1 (the value test is dead), and
/// join, the same landmass (None == None passes) or ForceConnected.
pub fn road_edges(m: &DstMap, join: bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for &(u, v) in &m.edges {
        let (Some(a), Some(b)) = (node_of_vertex(m, u), node_of_vertex(m, v)) else { continue };
        let (na, nb) = (&m.nodes[a], &m.nodes[b]);
        if na.kind == TYPE_BLANK || nb.kind == TYPE_BLANK {
            continue;
        }
        if join || na.landmass == nb.landmass || (na.site_flags | nb.site_flags) & FLAG_FORCE_CONNECTED != 0 {
            out.push((a, b));
        }
    }
    out
}

/// MergeConnectedLandmasses (0x140461a00), as DS.
pub fn merge_landmasses(m: &mut DstMap, join: bool) {
    let mut removed: Vec<usize> = Vec::new();
    for (a, b) in road_edges(m, join) {
        let (Some(mut tgt), Some(mut src)) = (m.nodes[a].landmass, m.nodes[b].landmass) else { continue };
        if tgt == src {
            continue;
        }
        let mut add = true;
        let mut skip = false;
        for &r in &removed {
            if r == tgt {
                tgt = src;
                src = r;
                add = false;
            } else if r == src {
                skip = true;
            }
        }
        if skip {
            continue;
        }
        let mut tmp: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (key, cells) in &m.landmasses[src].map {
            if key.is_empty() {
                break;
            }
            tmp.insert(key.clone(), cells.clone());
        }
        for cells in tmp.values() {
            for &c in cells {
                m.nodes[c].landmass = Some(tgt);
            }
        }
        for (key, cells) in tmp {
            m.landmasses[tgt].map.entry(key).or_default().extend(cells);
        }
        if add {
            removed.push(src);
        }
    }
    if removed.is_empty() {
        return;
    }
    let mut remap = vec![None; m.landmasses.len()];
    let mut kept = Vec::new();
    for (i, l) in std::mem::take(&mut m.landmasses).into_iter().enumerate() {
        if !removed.contains(&i) {
            remap[i] = Some(kept.len());
            kept.push(l);
        }
    }
    m.landmasses = kept;
    for n in &mut m.nodes {
        n.landmass = n.landmass.and_then(|l| remap[l]);
    }
}

/// ForceConnectivity core (0x14046ad30, mode 0) after landmasses and merge.
pub fn force_connectivity_core(
    m: &mut DstMap,
    t: &mut Tiles,
    validate: &mut Validate,
    tile: u16,
    is_cave: bool,
) -> Result<(), String> {
    let recs = road_edges(m, true);
    for &(a, b) in &recs {
        let (fa, fb) = (m.nodes[a].site_flags, m.nodes[b].site_flags);
        let (ta, tb) = (m.nodes[a].kind, m.nodes[b].kind);
        let (pa, pb) = (site_xy(m, a), site_xy(m, b));
        let v = validate(m.nodes[a].value)?;
        if (fa | fb) & FLAG_ROAD_POISON != 0 && (fa | fb) & FLAG_FORCE_CONNECTED != 0 && ta != 7 && tb != 7 {
            for (ox, oy) in [(0.0f32, 0.0f32), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                grid::line(t, pa.0 + ox, pa.1 + oy, pb.0 + ox, pb.1 + oy, v, false, true);
            }
        }
        if is_cave || (ta != TYPE_BLANK && ta != 7) {
            for (ox, oy) in [(0.0f32, 0.0f32), (1.0, 0.0), (0.0, 1.0)] {
                grid::line(t, pa.0 + ox, pa.1 + oy, pb.0 + ox, pb.1 + oy, v, false, true);
            }
        }
    }
    // [k=4], [k=5]: RoomLine 0x140451f10 both ways on records touching a type 5 / 6 cell.
    for kind in [5, 6] {
        let recs: Vec<(usize, usize)> = m
            .edges
            .iter()
            .filter_map(|&(u, v)| Some((node_of_vertex(m, u)?, node_of_vertex(m, v)?)))
            .filter(|&(a, b)| m.nodes[a].kind == kind || m.nodes[b].kind == kind)
            .collect();
        for (a, b) in recs {
            let v = validate(m.nodes[a].value)?;
            room_line(m, t, a, b, v)?;
            room_line(m, t, b, a, v)?;
        }
    }
    if is_cave {
        let roads = crate::worldsim::native_dst::roads::get_roads(m, true)?;
        for r in roads {
            for w in r.windows(2) {
                let (p, q) = (w[0], w[1]);
                for (ox, oy) in [(0.0f32, 0.0f32), (1.0, 0.0), (0.0, 1.0)] {
                    grid::line(t, p.0 + ox, p.1 + oy, q.0 + ox, q.1 + oy, tile, false, true);
                }
            }
        }
    }
    border(&mut t.t);
    Ok(())
}

/// RoomLine 0x140451f10: the AppendConnection polyline, 3 keepLand lines per pair.
fn room_line(m: &DstMap, t: &mut Tiles, a: usize, b: usize, tile: u16) -> Result<(), String> {
    let mut pts = Vec::new();
    crate::worldsim::native_dst::roads::append_connection(m, a, b, &mut pts)?;
    for w in pts.windows(2) {
        let (p, q) = (w[0], w[1]);
        for (ox, oy) in [(0.0f32, 0.0f32), (1.0, 0.0), (0.0, 1.0)] {
            grid::line(t, p.0 + ox, p.1 + oy, q.0 + ox, q.1 + oy, tile, false, true);
        }
    }
    Ok(())
}

/// Border fill 0x1404713c0: columns 0 and W-1 on every row, rows 0 and H-1.
fn border(g: &mut Grid) {
    let (w, h) = (g.w, g.h);
    for y in 0..h {
        g.cells[y * w] = tgm::DEFAULT_IMPASSABLE;
        g.cells[y * w + w - 1] = tgm::DEFAULT_IMPASSABLE;
        if y == 0 || y == h - 1 {
            for x in 0..w {
                g.cells[y * w + x] = tgm::DEFAULT_IMPASSABLE;
            }
        }
    }
}

/// DetectDisconnect (0x14012dff0): flood from START over land/fake ground;
/// unreached cells (not safe) become impassable with the road flag.
pub fn detect_disconnect(m: &DstMap, t: &mut Tiles, safe: Option<&Grid>) -> Result<usize, String> {
    let (w, h) = (t.w(), t.h());
    let pass = |v: u16| tgm::is_land(v) || v == tgm::FAKE_GROUND;
    let mut vis = vec![false; w * h];
    let start = m.node("START").and_then(|n| m.nodes[n].site).ok_or("DetectDisconnect: no START site")?;
    let s = &m.sites[start];
    let mut stack = vec![(s.x as i64 as i32, s.y as i64 as i32)];
    // Impassable cells start "visited" (bit 0x10 set when !pass).
    for i in 0..w * h {
        vis[i] = !pass(t.t.cells[i]);
    }
    while let Some((x, y)) = stack.pop() {
        for nx in x - 1..=x + 1 {
            for ny in y - 1..=y + 1 {
                if (nx == x && ny == y) || nx < 0 || ny < 0 || nx as usize >= w || ny as usize >= h {
                    continue;
                }
                let i = ny as usize * w + nx as usize;
                if pass(t.t.cells[i]) && !vis[i] {
                    vis[i] = true;
                    stack.push((nx, ny));
                }
            }
        }
    }
    let mut n = 0;
    for i in 0..w * h {
        let is_safe = safe.is_some_and(|g| i < g.cells.len() && g.hi(i) & 0x10 != 0);
        if !vis[i] && !is_safe {
            t.t.cells[i] = tgm::DEFAULT_IMPASSABLE;
            let hi = t.d.hi(i) | HI_ROAD;
            t.d.set_hi(i, hi);
            n += 1;
        }
    }
    Ok(n)
}

/// Pick 0x14012c500: a random non-blank cell of a landmass, one random
/// valid point, reserved with size 0.5.
fn pick(m: &DstMap, t: &mut Tiles, li: usize) -> Option<(f32, f32)> {
    let l = &m.landmasses[li];
    let k = pcg::boundedrand(l.map.len() as u32) as usize;
    let cells = l.map.values().nth(k).unwrap();
    let mut c = pcg::boundedrand(cells.len() as u32) as usize;
    let mut guard = 0;
    while m.nodes[cells[c]].kind == TYPE_BLANK {
        c = (c + 1) % cells.len();
        guard += 1;
        if guard > cells.len() {
            return None; // the game loops forever here
        }
    }
    let pts = grid::random_points(m, Some(t), cells[c], 1);
    grid::reserve_core(t, pts, 0.5, 0, 0, &[])
}

/// GetWormholesExtra (0x14012c7d0).
pub fn get_wormholes_extra(m: &DstMap, t: &mut Tiles) -> Option<[Vec<Option<f64>>; 4]> {
    if m.landmasses.is_empty() {
        return None;
    }
    let mut rl = vec![m.landmasses.len() - 1];
    let mut out: [Vec<Option<f64>>; 4] = Default::default();
    for lm in 0..m.landmasses.len() - 1 {
        let src = rl[pcg::boundedrand(rl.len() as u32) as usize];
        let a = pick(m, t, src);
        let b = pick(m, t, lm);
        out[0].push(a.map(|p| p.0 as f64));
        out[1].push(a.map(|p| p.1 as f64));
        out[2].push(b.map(|p| p.0 as f64));
        out[3].push(b.map(|p| p.1 as f64));
        rl.push(lm);
    }
    Some(out)
}

/// GetWormholes (0x14012d4b0)(id1, id2).
pub fn get_wormholes(m: &mut DstMap, t: &mut Tiles, id1: &str, id2: &str) -> Option<[Option<(f32, f32)>; 2]> {
    let (a, b) = (m.node(id1)?, m.node(id2)?);
    if m.nodes[a].landmass == m.nodes[b].landmass {
        return None;
    }
    let mut res = [None, None];
    for (k, n) in [a, b].into_iter().enumerate() {
        let pts = grid::random_points(m, Some(t), n, 1);
        res[k] = grid::reserve_core(t, pts, 1.0, 0, 0, &[]);
    }
    if let ([Some(_), Some(_)], Some(la), Some(lb)) = (res, m.nodes[a].landmass, m.nodes[b].landmass) {
        m.landmasses[la].partners.push(lb);
        m.landmasses[lb].partners.push(la);
    }
    Some(res)
}

/// MakeSafeFromDisconnect (0x14012a1a0).
pub fn make_safe(m: &DstMap, t: &Tiles, safe: &mut Option<Grid>, x: i32, y: i32) {
    if x < 0 || x as usize >= t.w() || y < 0 || y as usize >= t.h() {
        return;
    }
    let g = safe.get_or_insert_with(|| {
        let mut g = Grid::new(m.width as i64 as usize, m.height as i64 as usize);
        for i in 0..g.cells.len() {
            let hi = g.hi(i) & !0x10;
            g.set_hi(i, hi);
        }
        g
    });
    let i = y as usize * g.w + x as usize;
    if i < g.cells.len() {
        let hi = g.hi(i) | 0x10;
        g.set_hi(i, hi);
    }
}

