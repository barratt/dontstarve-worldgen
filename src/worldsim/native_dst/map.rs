//! DST BoostMap: node graph, incremental site placement and Voronoi cells.
//! A delta against the DS port in ../native/boostmap.rs, whose structure
//! this file follows; the Boost 1.53 diagram and Kamada-Kawai layout are
//! reused from ../native.
//! Every float operation the x64 server performs in SSE single precision is
//! done in f32 here.

use std::collections::{BTreeMap, HashMap};

use crate::worldsim::native::layout::{self, Minstd};
use crate::worldsim::native::voronoi::{Diagram, SOURCE_CATEGORY_SEGMENT_START_POINT};

pub type P2 = (f32, f32);

pub const TYPE_BLANK: i32 = 1;
pub const TYPE_BACKGROUND: i32 = 2;
pub const TYPE_RANDOM: i32 = 3;
pub const TYPE_BLOCKER: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Neighbour {
    Node(usize),
    Border,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    /// +0x00 u16 value (AddChild arg 3).
    pub value: u16,
    /// +0x30 parent Cell at AddChild time.
    pub parent: Option<usize>,
    /// +0x54 node type (NODE_TYPE 0..7).
    pub kind: i32,
    /// +0x50 internal connection type.
    pub internal: i32,
    /// +0x58 SetSiteFlags.
    pub site_flags: i32,
    /// +0xc8 state.
    pub state: i32,
    pub site: Option<usize>,
    pub has_vcell: bool,
    pub poly: Vec<P2>,
    pub neighbours: Vec<Neighbour>,
    pub centroid: P2,
    /// +0x28 SeparateIslands "connected" mark.
    pub connected_mark: bool,
    /// +0x29 GenerateLandmasses visited.
    pub visited: bool,
    pub landmass: Option<usize>,
    /// +0x68 landmass neighbours.
    pub lm_neighbours: Vec<usize>,
    /// +0x5c ForceDisconectedCells.
    pub blocked: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Landmass {
    pub map: BTreeMap<String, Vec<usize>>,
    pub partners: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct Site {
    pub x: f32,
    pub y: f32,
    pub name: String,
    pub node: Option<usize>,
}

/// rectangle_topology<minstd_rand> (0x14043e850): seed 1, bounds fixed at creation.
pub struct Topology {
    gen: Minstd,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Topology {
    fn new(w: f64, h: f64) -> Topology {
        Topology { gen: Minstd::new(), left: 0f64.min(w), top: 0f64.min(h), right: 0f64.max(w), bottom: 0f64.max(h) }
    }

    /// random_point (0x140460310): x before y.
    fn random_point(&mut self) -> (f64, f64) {
        let x = self.gen.uniform01() * (self.right - self.left) + self.left;
        let y = self.gen.uniform01() * (self.bottom - self.top) + self.top;
        (x, y)
    }
}

pub struct DstMap {
    pub width: f64,
    pub height: f64,
    pub nodes: Vec<Node>,
    /// std::map<string, Node*> (+0x138), name order.
    pub node_by_name: BTreeMap<String, usize>,
    pub vertex_names: Vec<String>,
    pub vertex_by_name: HashMap<String, usize>,
    /// Global edge list (source, target) in insertion order.
    pub edges: Vec<(usize, usize)>,
    pub out: Vec<Vec<usize>>,
    pub landmasses: Vec<Landmass>,
    pub roads: Option<Vec<Vec<P2>>>,
    pub positions: Vec<(f64, f64)>,
    pub topo: Option<Topology>,
    pub sites: Vec<Site>,
    pub bbox: [f64; 4],
    pub segments: Vec<[f64; 4]>,
    pub diagram: Diagram,
    pub point_data: Vec<(f64, f64)>,
    /// +0x190 default tile (TGM default impassable).
    pub default_tile: u16,
}

impl DstMap {
    /// impl ctor 0x1404634b0 called as (257, 257): W = H = 257, bbox, no segments.
    pub fn new() -> DstMap {
        DstMap {
            width: 257.0,
            height: 257.0,
            nodes: Vec::new(),
            node_by_name: BTreeMap::new(),
            vertex_names: Vec::new(),
            vertex_by_name: HashMap::new(),
            edges: Vec::new(),
            out: Vec::new(),
            landmasses: Vec::new(),
            roads: None,
            positions: Vec::new(),
            topo: None,
            sites: Vec::new(),
            bbox: [0.0, 257.0, 0.0, 257.0],
            segments: Vec::new(),
            diagram: Diagram::default(),
            point_data: Vec::new(),
            default_tile: 1,
        }
    }

    pub fn node(&self, name: &str) -> Option<usize> {
        self.node_by_name.get(name).copied()
    }

    /// AddChild (0x14012b1a0 → 0x140459320): always a new Cell.
    pub fn add_child(&mut self, parent: &str, id: &str, value: u16, kind: i32, internal: i32) {
        let parent = self.node(parent);
        let state = if kind == TYPE_BLANK || kind == TYPE_RANDOM { 1 } else { 5 };
        self.nodes.push(Node {
            name: id.to_string(),
            value,
            parent,
            kind,
            internal,
            site_flags: 0,
            state,
            site: None,
            has_vcell: false,
            poly: Vec::new(),
            neighbours: Vec::new(),
            centroid: (0.0, 0.0),
            connected_mark: false,
            visited: false,
            landmass: None,
            lm_neighbours: Vec::new(),
            blocked: false,
        });
        self.node_by_name.insert(id.to_string(), self.nodes.len() - 1);
    }

    pub fn vertex_for(&mut self, name: &str) -> usize {
        if let Some(&v) = self.vertex_by_name.get(name) {
            return v;
        }
        let v = self.vertex_names.len();
        self.vertex_names.push(name.to_string());
        self.out.push(Vec::new());
        self.vertex_by_name.insert(name.to_string(), v);
        v
    }

    pub fn add_link(&mut self, a: &str, b: &str) {
        let vb = self.vertex_for(b);
        let va = self.vertex_for(a);
        self.edges.push((va, vb));
        self.out[va].push(vb);
        self.out[vb].push(va);
    }

    pub fn has_edge(&self, u: usize, v: usize) -> bool {
        self.out.get(u).is_some_and(|o| o.contains(&v))
    }

    pub fn clear_node_links(&mut self, id: &str) {
        let v = self.vertex_for(id);
        self.edges.retain(|&(a, b)| a != v && b != v);
        let nbrs = std::mem::take(&mut self.out[v]);
        for u in nbrs {
            self.out[u].retain(|&w| w != v);
        }
    }

    pub fn remove_link(&mut self, a: &str, b: &str) -> Result<(), String> {
        let u = self.vertex_for(a);
        let v = self.vertex_for(b);
        match self.edges.iter().position(|&(x, y)| (x == u && y == v) || (x == v && y == u)) {
            Some(i) => {
                self.edges.remove(i);
                if let Some(p) = self.out[u].iter().position(|&w| w == v) {
                    self.out[u].remove(p);
                }
                if let Some(p) = self.out[v].iter().position(|&w| w == u) {
                    self.out[v].remove(p);
                }
                Ok(())
            }
            None => Err(format!("RemoveLink({a}, {b}) without a direct edge is not ported (search fallback)")),
        }
    }

    /// SetWorldSizeCore (0x14045d420): no scatter in DST.
    pub fn set_world_size_core(&mut self, w: u32, h: u32) {
        self.width = w as f64;
        self.height = h as f64;
        let (w, h) = (self.width, self.height);
        self.segments = vec![[0.0, 0.0, w, 0.0], [w, 0.0, w, h], [w, h, 0.0, h], [0.0, h, 0.0, 0.0]];
        self.bbox = [0f64.min(w), 0f64.max(w), 0f64.min(h), 0f64.max(h)];
    }

    /// WorldGen_InitializeNodePoints (0x1404616b0).
    pub fn initialize_node_points(&mut self) {
        let mut topo = Topology::new(self.width, self.height);
        self.positions = (0..self.vertex_names.len()).map(|_| topo.random_point()).collect();
        self.topo = Some(topo);
    }

    /// WorldGen_AddNewPositions (0x140461870): the same MINSTD stream continues.
    pub fn add_new_positions(&mut self) -> Result<(), String> {
        let topo = self.topo.as_mut().ok_or("WorldGen_AddNewPositions before WorldGen_InitializeNodePoints")?;
        for _ in self.positions.len()..self.vertex_names.len() {
            let p = topo.random_point();
            self.positions.push(p);
        }
        Ok(())
    }

    fn kk_and_sites(&mut self) {
        let _ok = layout::kamada_kawai(&mut self.positions, &self.edges);
        self.sites.clear();
        self.point_data.clear();
        for v in 0..self.vertex_names.len() {
            let name = self.vertex_names[v].clone();
            let node = self.node(&name);
            self.sites.push(Site { x: self.positions[v].0 as f32, y: self.positions[v].1 as f32, name, node });
            if let Some(n) = node {
                self.nodes[n].site = Some(v);
            }
        }
    }

    /// WorldGen_VoronoiPass(n) → RunLayout (0x14046c020).
    pub fn voronoi_pass(&mut self, times: i64) -> Result<(), String> {
        if self.positions.len() < self.vertex_names.len() {
            return Err("WorldGen_VoronoiPass: position map smaller than the graph".into());
        }
        for &n in self.node_by_name.values() {
            let node = &mut self.nodes[n];
            node.has_vcell = false;
            node.site = None;
            node.poly.clear();
        }
        // `for (t = 1; t <= n; ++t)` with a 32-bit n.
        let times = times as i32;
        for _ in 0..times.max(0) {
            self.kk_and_sites();
        }
        self.build_cells()
    }

    /// Extent (0x140439e30 / 0x140439ee0): float subtraction, truncation.
    fn extent(&self, x_axis: bool, border: i32) -> i32 {
        let (mut mx, mut mn) = (f32::MIN_POSITIVE, f32::MAX);
        for s in &self.sites {
            let v = if x_axis { s.x } else { s.y };
            if v > mx {
                mx = v;
            }
            if mn > v {
                mn = v;
            }
        }
        ((mx - mn) as i64 as i32).wrapping_add(2 * border)
    }

    /// WorldGen_Commit (0x14012a700).
    pub fn commit(&mut self) -> Result<bool, String> {
        let border = if self.width as i64 as i32 == 1 && self.height as i64 as i32 == 1 { 0 } else { 20 };
        let ky = self.extent(false, border);
        let kx = self.extent(true, border);
        let k = if (kx as u32) >= (ky as u32) { kx } else { ky };
        self.set_world_size_core(k as u32, k as u32);
        self.scale_recentre(1.0, 1.0)?;
        Ok(self.check_for_valid_cells())
    }

    /// ScaleRecentre (0x1404632c0): every op in f32; then VoronoiAssembly.
    pub fn scale_recentre(&mut self, sx: f32, sy: f32) -> Result<(), String> {
        for s in &mut self.sites {
            s.x *= sx;
            s.y *= sy;
        }
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN_POSITIVE);
        let (mut min_y, mut max_y) = (f32::MAX, f32::MIN_POSITIVE);
        for s in &self.sites {
            if min_x > s.x {
                min_x = s.x;
            }
            if s.x > max_x {
                max_x = s.x;
            }
            if min_y > s.y {
                min_y = s.y;
            }
            if s.y > max_y {
                max_y = s.y;
            }
        }
        let c = self.bbox.map(|v| v as f32);
        let h = 0.5f32;
        let dx = ((c[1] - c[0]) * h + c[0]) - ((max_x - min_x) * h + min_x);
        let dy = ((c[3] - c[2]) * h + c[2]) - ((max_y - min_y) * h + min_y);
        if dx != 0.0 || dy != 0.0 {
            for s in &mut self.sites {
                s.x = dx + s.x;
                s.y = dy + s.y;
            }
        }
        self.build_cells()
    }

    /// ConvertToTileMap rescale (0x14012b640); returns the new integer size.
    pub fn convert_rescale(&mut self, size: f64) -> Result<u32, String> {
        let mut s = size as f32;
        let (w, h) = (self.width as i64 as i32, self.height as i64 as i32);
        if !(s > (w as u32) as f32) {
            s = w as f32;
        }
        if !(s > (h as u32) as f32) {
            s = h as f32;
        }
        let sx = s / w as f32;
        let sy = s / h as f32;
        let isz = s as i64 as i32 as u32;
        self.set_world_size_core(isz, isz);
        self.scale_recentre(sx, sy)?;
        Ok(isz)
    }

    // ------------------------------------------------------------ cells

    /// VoronoiAssembly (0x140462ac0).
    pub fn build_cells(&mut self) -> Result<(), String> {
        self.point_data.clear();
        for (i, s) in self.sites.iter().enumerate() {
            let Some(n) = s.node else { return Err(format!("site {} has no node", s.name)) };
            let node = &mut self.nodes[n];
            node.state = 5;
            node.has_vcell = false;
            node.poly.clear();
            node.neighbours.clear();
            node.site = Some(i);
            self.point_data.push((s.x as f64, s.y as f64));
        }
        let points: Vec<(i32, i32)> = self.point_data.iter().map(|&(x, y)| (x as i64 as i32, y as i64 as i32)).collect();
        let segments: Vec<(i32, i32, i32, i32)> =
            self.segments.iter().map(|s| (s[0] as i64 as i32, s[1] as i64 as i32, s[2] as i64 as i32, s[3] as i64 as i32)).collect();
        self.diagram = Diagram::build(&points, &segments)?;
        let nsites = self.sites.len();
        for ci in 0..self.diagram.cells.len() {
            let vc = self.diagram.cells[ci];
            let idx = vc.source_index as usize;
            if idx >= nsites {
                continue;
            }
            let n = self.sites[idx].node.unwrap();
            self.nodes[n].has_vcell = true;
            let poly = self.cell_polygon(ci);
            let centroid = if poly.is_empty() { (self.sites[idx].x, self.sites[idx].y) } else { poly_centroid(&poly) };
            let node = &mut self.nodes[n];
            node.poly = poly;
            node.centroid = centroid;
            if node.kind == TYPE_RANDOM || node.kind == TYPE_BLANK {
                node.state = 1;
            }
            if vc.incident_edge < 0 {
                continue;
            }
            let start = vc.incident_edge as usize;
            let mut e = start;
            loop {
                let edge = self.diagram.edges[e];
                let mut nb = self.diagram.cells[edge.cell as usize].source_index as usize;
                if nb == idx {
                    nb = self.diagram.cells[self.diagram.edges[edge.twin as usize].cell as usize].source_index as usize;
                }
                if nb < nsites {
                    let other = self.sites[nb].node.unwrap();
                    self.nodes[n].neighbours.push(Neighbour::Node(other));
                } else {
                    self.nodes[n].neighbours.push(Neighbour::Border);
                    if self.nodes[n].state == 5 {
                        self.nodes[n].state = 4;
                    }
                }
                e = edge.next as usize;
                if e == start {
                    break;
                }
            }
        }
        for i in 0..nsites {
            let n = self.sites[i].node.unwrap();
            let state = self.nodes[n].state;
            let nb_state = |nb: &Neighbour, nodes: &[Node]| match nb {
                Neighbour::Node(m) => nodes[*m].state,
                Neighbour::Border => 0,
            };
            if state > 3 {
                if self.nodes[n].neighbours.iter().any(|nb| nb_state(nb, &self.nodes) == 1) {
                    self.nodes[n].state = 3;
                }
            } else if state == 1 && self.nodes[n].neighbours.iter().all(|nb| nb_state(nb, &self.nodes) >= 3) {
                self.nodes[n].state = 2;
            }
        }
        Ok(())
    }

    /// SitePolygon (0x14045f3a0) for diagram cell `ci`.
    fn cell_polygon(&self, ci: usize) -> Vec<P2> {
        let vc = self.diagram.cells[ci];
        let mut out = Vec::new();
        if vc.incident_edge < 0 {
            return out;
        }
        let start = vc.incident_edge as usize;
        let mut e = start;
        loop {
            if let Some(samples) = self.edge_samples(e) {
                out.extend(samples.iter().map(|&(x, y)| (x as f32, y as f32)));
            }
            e = self.diagram.edges[e].next as usize;
            if e == start {
                break;
            }
        }
        out
    }

    fn retrieve_point(&self, ci: usize) -> (f64, f64) {
        let cell = self.diagram.cells[ci];
        if cell.source_category == 0 {
            return self.point_data[cell.source_index as usize];
        }
        let seg = self.segments[cell.source_index as usize - self.point_data.len()];
        if cell.source_category == SOURCE_CATEGORY_SEGMENT_START_POINT { (seg[0], seg[1]) } else { (seg[2], seg[3]) }
    }

    /// ClipInfiniteEdge (0x14045d900 = DS 0x006406b0).
    fn clip_infinite_edge(&self, e: usize) -> Vec<(f64, f64)> {
        let edge = self.diagram.edges[e];
        let twin = self.diagram.edges[edge.twin as usize];
        let (c1, c2) = (edge.cell as usize, twin.cell as usize);
        let (cell1, cell2) = (self.diagram.cells[c1], self.diagram.cells[c2]);
        let (origin, direction);
        if cell1.contains_point() && cell2.contains_point() {
            let p1 = self.retrieve_point(c1);
            let p2 = self.retrieve_point(c2);
            origin = ((p1.0 + p2.0) * 0.5, (p1.1 + p2.1) * 0.5);
            direction = (p1.1 - p2.1, p2.0 - p1.0);
        } else {
            let o = if !cell1.contains_point() { self.retrieve_point(c2) } else { self.retrieve_point(c1) };
            let seg_cell = if !cell1.contains_point() { cell1 } else { cell2 };
            let seg = self.segments[seg_cell.source_index as usize - self.point_data.len()];
            let (dx, dy) = (seg[2] - seg[0], seg[3] - seg[1]);
            let low_is_origin = seg[0] == o.0 && seg[1] == o.1;
            direction = if low_is_origin ^ cell1.contains_point() { (dy, -dx) } else { (-dy, dx) };
            origin = o;
        }
        let side = self.bbox[1] - self.bbox[0];
        let koef = side / direction.0.abs().max(direction.1.abs());
        let mut out = Vec::with_capacity(2);
        out.push(if edge.vertex0 < 0 {
            (origin.0 - direction.0 * koef, origin.1 - direction.1 * koef)
        } else {
            let v = self.diagram.vertices[edge.vertex0 as usize];
            (v.x, v.y)
        });
        out.push(if twin.vertex0 < 0 {
            (origin.0 + direction.0 * koef, origin.1 + direction.1 * koef)
        } else {
            let v = self.diagram.vertices[twin.vertex0 as usize];
            (v.x, v.y)
        });
        out
    }

    /// The double samples of one diagram edge (SitePolygon / PaintEdge);
    /// None when dropped ("Infinite edge!").
    pub fn edge_samples(&self, e: usize) -> Option<Vec<(f64, f64)>> {
        let edge = self.diagram.edges[e];
        let twin = self.diagram.edges[edge.twin as usize];
        if edge.vertex0 < 0 || twin.vertex0 < 0 {
            return Some(self.clip_infinite_edge(e));
        }
        let v0 = self.diagram.vertices[edge.vertex0 as usize];
        let v1 = self.diagram.vertices[twin.vertex0 as usize];
        let finite = v0.x.is_finite() && v0.y.is_finite() && v1.x.is_finite() && v1.y.is_finite();
        if !finite || v1.x.abs() > 10000.0 || v0.y.abs() > 10000.0 || v0.x.abs() > 10000.0 || v1.y.abs() > 10000.0 {
            return None;
        }
        let mut samples = vec![(v0.x, v0.y), (v1.x, v1.y)];
        if edge.is_linear == 0 {
            self.sample_curved_edge(e, &mut samples);
        }
        Some(samples)
    }

    pub fn twin_site(&self, e: usize) -> usize {
        let twin = self.diagram.edges[self.diagram.edges[e].twin as usize];
        self.diagram.cells[twin.cell as usize].source_index as usize
    }

    fn sample_curved_edge(&self, e: usize, samples: &mut Vec<(f64, f64)>) {
        let edge = self.diagram.edges[e];
        let twin = self.diagram.edges[edge.twin as usize];
        let cell = self.diagram.cells[edge.cell as usize];
        let twin_cell = self.diagram.cells[twin.cell as usize];
        let max_dist = (self.bbox[1] - self.bbox[0]) * 0.001;
        let sc = if cell.contains_point() { twin_cell } else { cell };
        let pci = if cell.contains_point() { edge.cell } else { twin.cell } as usize;
        let point = self.retrieve_point(pci);
        let seg = self.segments[sc.source_index as usize - self.point_data.len()];
        discretize(point, seg, max_dist, samples);
    }

    /// CheckForValidCells (0x14043e750), in name order.
    pub fn check_for_valid_cells(&self) -> bool {
        for &n in self.node_by_name.values() {
            let c = &self.nodes[n];
            if c.kind == TYPE_BLANK || c.parent.is_none() || c.poly.len() >= 3 {
                continue;
            }
            return false;
        }
        true
    }

    /// SitePoints (0x140451c90 → lattice 0x140439280).
    pub fn site_points(&self, name: &str) -> Option<Vec<P2>> {
        let n = self.node(name)?;
        self.node_points(n)
    }

    pub fn node_points(&self, n: usize) -> Option<Vec<P2>> {
        let node = &self.nodes[n];
        let site = &self.sites[node.site?];
        Some(lattice_points(&node.poly, site.x, site.y))
    }
}

/// Lattice rasteriser 0x140439280 (radius 20.0f): float32, no W/H bound.
/// Results are remembered by their exact inputs (worldgen asks for the
/// same site several times).
pub fn lattice_points(poly: &[P2], sx: f32, sy: f32) -> Vec<P2> {
    type Key = (Vec<(u32, u32)>, u32, u32);
    thread_local! {
        static MEMO: std::cell::RefCell<HashMap<Key, Vec<P2>>> = Default::default();
    }
    let key: Key = (poly.iter().map(|p| (p.0.to_bits(), p.1.to_bits())).collect(), sx.to_bits(), sy.to_bits());
    if let Some(hit) = MEMO.with(|m| m.borrow().get(&key).cloned()) {
        return hit;
    }
    let out = lattice_points_uncached(poly, sx, sy);
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 4096 {
            m.clear();
        }
        m.insert(key, out.clone());
    });
    out
}

fn lattice_points_uncached(poly: &[P2], sx: f32, sy: f32) -> Vec<P2> {
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, -f32::MAX, -f32::MAX);
    for &(x, y) in poly {
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
    let mut out = Vec::new();
    if poly.is_empty() {
        return out;
    }
    let mut ix = minx as i64 as u32;
    while maxx > ix as f32 {
        let mut iy = miny as i64 as u32;
        while maxy > iy as f32 {
            let (px, py) = (ix as f32, iy as f32);
            let dx = px - sx;
            let dy = py - sy;
            let d = (dx * dx + dy * dy).sqrt();
            if 20.0f32 > d && point_in_poly((px, py), poly) {
                out.push((px, py));
            }
            iy = iy.wrapping_add(1);
        }
        ix = ix.wrapping_add(1);
    }
    out
}

/// PointInPoly (0x140438d50): winding number, isLeft in f32.
pub fn point_in_poly(p: P2, v: &[P2]) -> bool {
    let n = v.len();
    let mut wn = 0i32;
    for i in 0..n {
        let a = v[i];
        let b = if i == n - 1 { v[0] } else { v[i + 1] };
        let is_left = (p.1 - a.1) * (b.0 - a.0) - (p.0 - a.0) * (b.1 - a.1);
        if p.1 >= a.1 {
            if b.1 > p.1 && is_left > 0.0 {
                wn += 1;
            }
        } else if p.1 >= b.1 && 0.0 > is_left {
            wn -= 1;
        }
    }
    wn != 0
}

/// Centroid (0x140439140): all-float shoelace.
pub fn poly_centroid(p: &[P2]) -> P2 {
    let n = p.len();
    let (mut a, mut sx, mut sy) = (0f32, 0f32, 0f32);
    for i in 0..n - 1 {
        let (x0, y0, x1, y1) = (p[i].0, p[i].1, p[i + 1].0, p[i + 1].1);
        let c = y1 * x0 - x1 * y0;
        sy += (y1 + y0) * c;
        sx += (x1 + x0) * c;
        a += c;
    }
    let (x0, y0, x1, y1) = (p[n - 1].0, p[n - 1].1, p[0].0, p[0].1);
    let c = y1 * x0 - x1 * y0;
    let fsy = (y1 + y0) * c + sy;
    let fsx = (x1 + x0) * c + sx;
    a = c + a;
    let half = (a as f64 * 0.5) as f32;
    let k = half * 6.0f32;
    (fsx / k, fsy / k)
}

/// Boost 1.53 discretize with Klei's non-finite guard (0x14045be10).
fn discretize(point: (f64, f64), seg: [f64; 4], max_dist: f64, samples: &mut Vec<(f64, f64)>) {
    let (x0, y0, x1, y1) = (seg[0], seg[1], seg[2], seg[3]);
    let svx = x1 - x0;
    let svy = y1 - y0;
    let sq = svx * svx + svy * svy;
    let proj = |p: (f64, f64)| -> f64 {
        let (pvx, pvy) = (p.0 - x0, p.1 - y0);
        let slen = svx * svx + svy * svy;
        let dot = svx * pvx + svy * pvy;
        dot / slen
    };
    let ps = sq * proj(samples[0]);
    let pe = sq * proj(samples[1]);
    let (pvx, pvy) = (point.0 - x0, point.1 - y0);
    let rx = svx * pvx + svy * pvy;
    let ry = svx * pvy - svy * pvx;
    let par = |x: f64| ((x - rx) * (x - rx) + ry * ry) / (ry + ry);
    let last = samples[1];
    samples.pop();
    let mut stack = vec![pe];
    let mut cx = ps;
    let mut cy = par(cx);
    let mdt = max_dist * max_dist * sq;
    while let Some(&nx) = stack.last() {
        let ny = par(nx);
        let mx = (ny - cy) / (nx - cx) * ry + rx;
        let my = par(mx);
        if !mx.is_finite() || !my.is_finite() {
            stack.pop();
            continue;
        }
        let mut d = (ny - cy) * (mx - cx) - (nx - cx) * (my - cy);
        d = d * d / ((ny - cy) * (ny - cy) + (nx - cx) * (nx - cx));
        if d <= mdt {
            stack.pop();
            samples.push(((svx * nx - svy * ny) / sq + x0, (svx * ny + svy * nx) / sq + y0));
            cx = nx;
            cy = ny;
        } else {
            stack.push(mx);
        }
    }
    *samples.last_mut().unwrap() = last;
}
