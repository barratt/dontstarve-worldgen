//! SeparateIslands, ForceConnectivity (GenerateLandmasses, Merge, corridors,
//! ForceValid, border ring) and GetWormholesExtra, in the game's operation
//! order. `fr` marks every float32 store of the game code.

use std::collections::BTreeMap;

use crate::crt;
use crate::worldsim::native::boostmap::{BoostMap, Landmass};
use crate::worldsim::native::points::random_shuffle;
use crate::worldsim::native::tiles::{is_water, TileGrid};

fn fr(x: f64) -> f64 {
    x as f32 as f64
}

const TYPE_BLANK: i32 = 1;
const TYPE_RANDOM: i32 = 3;
const TYPE_BLOCKER: i32 = 4;
const TYPE_ROOM: i32 = 5;
const FLAG_FORCE_CONNECTED: i32 = 2;
const FLAG_ROAD_POISON: i32 = 4;

/// Family (0x64FC20): "same task" in forest worlds.
fn family(m: &BoostMap, x: usize, y: usize) -> bool {
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

fn vertex_of(m: &BoostMap, node: usize) -> Option<usize> {
    m.vertex_by_name.get(&m.nodes[node].name).copied()
}

fn node_of_vertex(m: &BoostMap, v: usize) -> Option<usize> {
    m.sites.get(v).and_then(|s| s.node)
}

/// Related: the nodes are family, or either has a graph neighbour in the
/// other's family.
fn related(m: &BoostMap, a: usize, b: usize) -> bool {
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

/// Steps (0x64BAF0): the stepper behind every line writer.
fn steps(mut x: f64, mut y: f64, x1: f64, y1: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let adx = fr(fr(x1 - x).abs());
    let ady = fr(fr(y1 - y).abs());
    let sx = if x1 > x { 1.0 } else { -1.0 };
    let sy = if y1 > y { 1.0 } else { -1.0 };
    let n = ady + adx;
    let mut err = 0.0f64;
    if !(n > 0.0) {
        return out;
    }
    let mut i = 0i64;
    loop {
        out.push((fr(x.floor()), fr(y.floor())));
        let e1 = fr(err + ady);
        let e2 = fr(err - adx);
        if fr(e2.abs()) > fr(e1.abs()) {
            x = fr(sx + x);
            err = e1;
        } else {
            y = fr(sy + y);
            err = e2;
        }
        i += 1;
        if !(n > i as f64) {
            break;
        }
    }
    out
}

fn clamp(v: f64, dim: usize) -> f64 {
    let mut v = v;
    if 1.0 > v {
        v = 1.0;
    }
    if v >= fr(dim as f64) {
        v = fr((dim - 1) as f64);
    }
    v
}

/// Line (0x64BDB0)(x0, y0, x1, y1, tile, setB7, keepLand).
pub fn line(g: &mut TileGrid, x0: f64, y0: f64, x1: f64, y1: f64, tile: u8, set_b7: bool, keep_land: bool) {
    let (x0, x1) = (clamp(x0, g.w), clamp(x1, g.w));
    let (y0, y1) = (clamp(y0, g.h), clamp(y1, g.h));
    let mut t = tile;
    for (x, y) in steps(x0, y0, x1, y1) {
        let (x, y) = (clamp(x, g.w), clamp(y, g.h));
        let i = (y.floor() as i64 as usize) * g.w + x.floor() as i64 as usize;
        let c = &mut g.cells[i];
        // keepLand overwrites the tile *argument* (0x64BF74 stores into the
        // stack slot), so the kept land id sticks for the rest of the line.
        if keep_land && c[0] > 1 && c[0] < 0x80 {
            t = c[0];
        }
        c[1] &= 0x7F;
        if set_b7 {
            c[1] |= 0x80;
        }
        c[0] = t;
        if t > 0x80 {
            c[1] |= 0x80;
        }
    }
}

/// Corridor line (0x64C0B0): rewrite only id == 1 cells.
fn corridor(g: &mut TileGrid, x0: f64, y0: f64, x1: f64, y1: f64, tile: u8) {
    let (x0, x1) = (clamp(x0, g.w), clamp(x1, g.w));
    let (y0, y1) = (clamp(y0, g.h), clamp(y1, g.h));
    for (x, y) in steps(x0, y0, x1, y1) {
        let (x, y) = (clamp(x, g.w), clamp(y, g.h));
        let i = (y.floor() as i64 as usize) * g.w + x.floor() as i64 as usize;
        if g.cells[i][0] == 1 {
            g.cells[i][0] = tile;
        }
    }
}

/// PaintEdge (0x641BA0): five parallel lines per sample pair.
fn paint_edge(m: &BoostMap, g: &mut TileGrid, e: usize, tile: u8) {
    let Some(pts) = m.edge_samples(e) else { return };
    for k in 0..pts.len().saturating_sub(1) {
        let (p, q) = (pts[k], pts[k + 1]);
        line(g, fr(p.0), fr(p.1), fr(q.0), fr(q.1), tile, false, false);
        line(g, fr(p.0 + 1.0), fr(p.1), fr(q.0 + 1.0), fr(q.1), tile, false, false);
        line(g, fr(p.0 - 1.0), fr(p.1), fr(q.0 - 1.0), fr(q.1), tile, false, false);
        line(g, fr(p.0), fr(p.1 + 1.0), fr(q.0), fr(q.1 + 1.0), tile, false, false);
        line(g, fr(p.0), fr(p.1 - 1.0), fr(q.0), fr(q.1 - 1.0), tile, false, false);
    }
}

fn float_len(p: (f64, f64), q: (f64, f64)) -> f64 {
    let dx = fr(p.0 - q.0);
    let dy = fr(p.1 - q.1);
    fr(fr(dx * dx + dy * dy).sqrt())
}

fn site_xy(m: &BoostMap, node: usize) -> (f64, f64) {
    let s = &m.sites[m.nodes[node].site.unwrap()];
    (s.x as f64, s.y as f64)
}

fn edge_float_ends(m: &BoostMap, e: usize) -> Option<((f64, f64), (f64, f64))> {
    let edge = m.diagram.edges[e];
    let twin = m.diagram.edges[edge.twin as usize];
    if edge.vertex0 < 0 || twin.vertex0 < 0 {
        return None;
    }
    let a = m.diagram.vertices[edge.vertex0 as usize];
    let b = m.diagram.vertices[twin.vertex0 as usize];
    Some(((fr(a.x), fr(a.y)), (fr(b.x), fr(b.y))))
}

fn linked(m: &BoostMap, a: usize, b: usize) -> bool {
    match (vertex_of(m, a), vertex_of(m, b)) {
        (Some(u), Some(v)) => m.has_edge(u, v),
        _ => false,
    }
}

/// SeparateIslands (0x645D40).
pub fn separate_islands(m: &mut BoostMap, g: &mut TileGrid) {
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
                    b_room = kb == TYPE_ROOM;
                    let lk = linked(m, a, b);
                    let elen = edge_float_ends(m, e).map(|(p, q)| float_len(p, q)).unwrap_or(0.0);
                    let bl = related(m, a, b);
                    connected = lk && (elen > 4.0 || bl);
                    if ka != TYPE_BLOCKER && kb != TYPE_BLOCKER {
                        connected = connected || bl;
                    }
                    if float_len(site_xy(m, a), site_xy(m, b)) > 40.0 {
                        connected = false;
                    }
                }
            }
            if !skip {
                if ka == TYPE_ROOM || b_room {
                    paint_edge(m, g, e, tile);
                } else if connected || m.nodes[a].site_flags & FLAG_FORCE_CONNECTED != 0 {
                    m.nodes[a].connected_mark = true;
                } else {
                    paint_edge(m, g, e, tile);
                }
            }
            e = m.diagram.edges[e].next as usize;
            if e == start {
                break;
            }
        }
    }
}

fn task_key(name: &str) -> String {
    name.split(':').next().unwrap_or("").to_string()
}

/// GenerateLandmasses (0x646A00) + flood (0x646360).
fn generate_landmasses(m: &mut BoostMap) {
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

fn flood(m: &mut BoostMap, ci: usize, map: &mut BTreeMap<String, Vec<usize>>) {
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
                if lk || kb != TYPE_ROOM {
                    let bl = related(m, a, b);
                    let elen = edge_float_ends(m, e).map(|(p, q)| float_len(p, q)).unwrap_or(0.0);
                    let sd = float_len(site_xy(m, a), site_xy(m, b));
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

/// Edge records (0x641740): road-eligible links in global edge order.
pub fn road_edges(m: &BoostMap, join: bool) -> Vec<(usize, usize)> {
    let ok_type = |k: i32| k == 0 || k == 4 || k == 5;
    let mut out = Vec::new();
    for &(u, v) in &m.edges {
        let (Some(a), Some(b)) = (node_of_vertex(m, u), node_of_vertex(m, v)) else { continue };
        let (na, nb) = (&m.nodes[a], &m.nodes[b]);
        if na.value == -1 || nb.value == -1 || !ok_type(na.kind) || !ok_type(nb.kind) {
            continue;
        }
        if join || na.landmass == nb.landmass || (na.site_flags | nb.site_flags) & FLAG_FORCE_CONNECTED != 0 {
            out.push((a, b));
        }
    }
    out
}

/// MergeConnectedLandmasses (0x642E20).
fn merge_landmasses(m: &mut BoostMap, join: bool) {
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
    // Erase removed landmasses, keeping survivors in order; remap indices.
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

/// ForceConnectivity (0x4A6240): landmasses, merge, corridors, ForceValid, border.
pub fn force_connectivity(m: &mut BoostMap, g: &mut TileGrid, join: bool, tile: u8) -> Result<(), String> {
    generate_landmasses(m);
    merge_landmasses(m, join);
    let recs = road_edges(m, true);
    for (k, &(a, b)) in recs.iter().enumerate() {
        let (fa, fb) = (m.nodes[a].site_flags, m.nodes[b].site_flags);
        let (pa, pb) = (site_xy(m, a), site_xy(m, b));
        if (fa | fb) & FLAG_ROAD_POISON != 0 && (fa | fb) & FLAG_FORCE_CONNECTED != 0 {
            for (ox, oy) in [(0.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                corridor(g, fr(pa.0 + ox), fr(pa.1 + oy), fr(pb.0 + ox), fr(pb.1 + oy), tile);
            }
        }
        if m.nodes[a].kind == TYPE_ROOM || m.nodes[b].kind == TYPE_ROOM {
            // Cave lines; the game indexes Sites with the pair index (sic).
            let v = m.sites.get(k).and_then(|s| s.node).map(|n| m.nodes[n].value).unwrap_or(0) as u8;
            line(g, pa.0, pa.1, pb.0, pb.1, v, false, true);
            line(g, fr(pa.0 + 1.0), pa.1, fr(pb.0 + 1.0), pb.1, v, false, true);
            line(g, pa.0, fr(pa.1 + 1.0), pb.0, fr(pb.1 + 1.0), v, false, true);
        }
    }
    // Cave rooms (0x6440C5): outline every type-5 cell, in site order.
    for s in 0..m.sites.len() {
        let Some(n) = m.sites[s].node else { continue };
        if m.nodes[n].kind != TYPE_ROOM {
            continue;
        }
        let mut v = m.nodes[n].value;
        if (v as u32) > 100 || v == 1 {
            v = 3;
        }
        room_outline(m, g, n, v as u8);
    }
    force_valid(g);
    border(g);
    Ok(())
}

/// Diagram cell index of a node's Voronoi cell (Cell+0x24).
fn vcell_of(m: &BoostMap, node: usize) -> Option<usize> {
    let site = m.nodes[node].site? as u32;
    m.diagram.cells.iter().position(|c| c.source_index == site && c.contains_point())
}

/// Link geometry 0x63EC10(out, cellA, cellB, mode): walk A's edges (next;
/// prev for mode 3) for the one whose twin lies in B and return its two
/// float vertices; modes 3/4 also collect the vertices of every edge passed.
fn link_geometry(m: &BoostMap, a: usize, b: usize, mode: i32) -> Option<Vec<(f64, f64)>> {
    let start = usize::try_from(m.diagram.cells[a].incident_edge).ok()?;
    let mut out = Vec::new();
    let vtx = |v: i32| {
        let p = m.diagram.vertices[v as usize];
        (fr(p.x), fr(p.y))
    };
    let mut e = start;
    loop {
        let edge = m.diagram.edges[e];
        let twin = m.diagram.edges[edge.twin as usize];
        if twin.cell as usize == b {
            if edge.vertex0 >= 0 && twin.vertex0 >= 0 {
                out.push(vtx(edge.vertex0));
                out.push(vtx(twin.vertex0));
                return Some(out);
            }
        } else if (mode == 3 || mode == 4) && edge.vertex0 >= 0 && twin.vertex0 >= 0 {
            out.push(vtx(edge.vertex0));
            out.push(vtx(twin.vertex0));
        }
        e = if mode == 3 { edge.prev } else { edge.next } as usize;
        if e == start {
            return None;
        }
    }
}

/// Connection polylines 0x64FE70(cell, &lines, 1): one per landmass
/// neighbour, start point by internal type, then the midpoint of the shared
/// Voronoi edge.
fn connection_lines(m: &BoostMap, n: usize) -> Vec<Vec<(f64, f64)>> {
    let mut lines = Vec::new();
    let node = &m.nodes[n];
    let va = vcell_of(m, n);
    for &b in &node.lm_neighbours {
        let vb = vcell_of(m, b);
        let mut pts: Vec<(f64, f64)> = Vec::new();
        match node.internal {
            1 => pts.push(site_xy(m, n)),
            2 | 5 => {}
            3 | 4 => {
                if let (Some(va), Some(vb)) = (va, vb) {
                    if let Some(g) = link_geometry(m, va, vb, node.internal) {
                        pts.extend_from_slice(&g[..g.len() - 1]);
                    }
                }
            }
            _ => pts.push((node.centroid.0 as f64, node.centroid.1 as f64)),
        }
        // Neighbour kind is always 1 (0 < kind <= 2): append the edge midpoint.
        if let (Some(va), Some(vb)) = (va, vb) {
            if let Some(g) = link_geometry(m, va, vb, 2) {
                let (p0, p1) = (g[0], g[1]);
                let hx = fr(fr(p1.0 - p0.0) * 0.5);
                let hy = fr(fr(p1.1 - p0.1) * 0.5);
                pts.push((fr(p0.0 + hx), fr(p0.1 + hy)));
            }
        }
        lines.push(pts);
    }
    lines
}

/// RoomOutline 0x643B70(grid, cell, v): three keepLand lines per segment.
fn room_outline(m: &BoostMap, g: &mut TileGrid, n: usize, v: u8) {
    for pts in connection_lines(m, n) {
        for k in 0..pts.len().saturating_sub(1) {
            let (p, q) = (pts[k], pts[k + 1]);
            line(g, p.0, p.1, q.0, q.1, v, false, true);
            line(g, fr(p.0 + 1.0), p.1, fr(q.0 + 1.0), q.1, v, false, true);
            line(g, p.0, fr(p.1 + 1.0), q.0, fr(q.1 + 1.0), v, false, true);
        }
    }
}

/// ForceValid (0x650540).
fn force_valid(g: &mut TileGrid) {
    for y in 1..g.h.saturating_sub(1) {
        for x in 1..g.w.saturating_sub(1) {
            let i = g.idx(x, y);
            if g.cells[i][0] >= 0x80 {
                g.cells[i][1] |= 0x20;
                continue;
            }
            let wall = match g.cells[i][0] {
                4 => 0x98,
                8 => 0x99,
                13 => 0x9A,
                14 => 0x9B,
                15 => 0x9C,
                _ => 0x97,
            };
            for nx in x - 1..=x + 1 {
                for ny in y - 1..=y + 1 {
                    if nx == x && ny == y {
                        continue;
                    }
                    let j = g.idx(nx, ny);
                    if g.cells[j][0] >= 0x80 || g.cells[j][1] & 0x80 != 0 {
                        g.cells[j][1] |= 0x20;
                        g.cells[j][0] = wall;
                    }
                }
            }
        }
    }
}

/// Border ring (0x650450): ids only.
fn border(g: &mut TileGrid) {
    for y in 0..g.h {
        let (l, r) = (g.idx(0, y), g.idx(g.w - 1, y));
        g.cells[l][0] = 1;
        g.cells[r][0] = 1;
        if y == 0 || y == g.h - 1 {
            for x in 0..g.w {
                let i = g.idx(x, y);
                g.cells[i][0] = 1;
            }
        }
    }
}

/// GetRandomPointsForSite core (0x6404B0) with count 1.
fn random_point(m: &BoostMap, g: &TileGrid, node: usize) -> Option<(f32, f32)> {
    let mut pts = m.site_points(&m.nodes[node].name)?;
    random_shuffle(&mut pts);
    for p in pts {
        if (p.0 as f64) < 0.0 || p.0 as f64 >= m.width || (p.1 as f64) < 0.0 || p.1 as f64 >= m.height {
            continue;
        }
        let c = g.cells[(p.1.floor() as usize) * g.w + p.0.floor() as usize];
        if c[1] & 0x40 != 0 || c[0] == 0xFF || c[0] == 1 || c[0] == m.default_tile {
            continue;
        }
        return Some(p);
    }
    None
}

/// GetWormholesExtra (0x4A8E30): four arrays (Nil where a pick failed).
pub fn get_wormholes_extra(m: &BoostMap, g: &mut TileGrid) -> Option<[Vec<Option<f64>>; 4]> {
    if m.landmasses.is_empty() {
        return None;
    }
    let mut rl = vec![m.landmasses.len() - 1];
    let rest: Vec<usize> = (0..m.landmasses.len() - 1).collect();
    let mut out: [Vec<Option<f64>>; 4] = Default::default();
    let pick = |li: usize, g: &mut TileGrid| -> Option<(f64, f64)> {
        let l = &m.landmasses[li];
        let k = crt::rand() as usize % l.map.len();
        let cells = l.map.values().nth(k).unwrap();
        let cell = cells[crt::rand() as usize % cells.len()];
        let p = random_point(m, g, cell)?;
        // ReserveSpace core, size 0.5f: 1x1 check + reserve.
        let id = g.cells[(p.1.trunc() as usize) * g.w + p.0.trunc() as usize][0];
        if id == 0xFF || id == 1 || id == 0x32 || is_water(id) {
            return None;
        }
        let fl = |v: f64| (v as f32 as f64).floor() as i64;
        let x0 = fl((p.0 as f64 - 0.5) as f32 as f64 + 0.5);
        let x1 = fl((p.0 as f64 + 0.5) as f32 as f64 + 0.5);
        let y0 = fl((p.1 as f64 - 0.5) as f32 as f64 + 0.5);
        let y1 = fl((p.1 as f64 + 0.5) as f32 as f64 + 0.5);
        let (w, h) = (g.w as i64, g.h as i64);
        if !(x0 > 0 && x1 < w - 1 && y0 > 0 && y1 < h - 1) {
            return None;
        }
        for x in x0..x1 {
            for y in y0..y1 {
                let c = g.cells[(y * w + x) as usize];
                if c[0] == 0xFF || c[0] == 1 || c[0] == 0x32 || c[0] == m.default_tile || c[1] & 0x60 != 0 || is_water(c[0]) {
                    return None;
                }
            }
        }
        for x in x0..x1 {
            for y in y0..y1 {
                g.cells[(y * w + x) as usize][1] |= 0x40;
            }
        }
        Some((x0 as f32 as f64, y0 as f32 as f64))
    };
    for lm in rest {
        let src = rl[crt::rand() as usize % rl.len()];
        let a = pick(src, g);
        let b = pick(lm, g);
        out[0].push(a.map(|p| p.0));
        out[1].push(a.map(|p| p.1));
        out[2].push(b.map(|p| p.0));
        out[3].push(b.map(|p| p.1));
        rl.push(lm);
    }
    Some(out)
}

/// DetectDisconnect (0x4A98D0): 8-neighbour flood from the START site over
/// walkable ids (1 < id < 0x80) inside the 1-tile border; every walkable
/// tile not reached becomes id 1. Returns that count. The scratch TileGrid
/// copy costs W*H rand() draws (ctor), then is memset to 0xFF.
pub fn detect_disconnect(m: &BoostMap, g: &mut TileGrid) -> Result<u32, String> {
    let (w, h) = (g.w, g.h);
    for _ in 0..w * h {
        crt::rand();
    }
    let walkable = |id: u8| id > 1 && id < 0x80;
    // Visited mark (copy flags bit 4): set up front for non-walkable tiles.
    let mut seen: Vec<bool> = g.cells.iter().map(|c| !walkable(c[0])).collect();
    let start = m.node("START").and_then(|n| m.nodes[n].site).ok_or("DetectDisconnect: no START site")?;
    let s = &m.sites[start];
    let mut stack: Vec<(i32, i32)> = vec![(s.x as i32, s.y as i32)];
    while let Some((x0, y0)) = stack.pop() {
        for nx in x0 - 1..=x0 + 1 {
            for ny in y0 - 1..=y0 + 1 {
                if nx == x0 && ny == y0 {
                    continue;
                }
                if !(nx < w as i32 - 1 && nx > 0 && ny < h as i32 - 1 && ny > 0) {
                    continue;
                }
                let i = ny as usize * w + nx as usize;
                if !walkable(g.cells[i][0]) || seen[i] {
                    continue;
                }
                seen[i] = true;
                stack.push((nx, ny));
            }
        }
    }
    let mut count = 0;
    for i in 0..w * h {
        if !seen[i] {
            g.cells[i][0] = 1;
            count += 1;
        }
    }
    Ok(count)
}
