//! Klei's `BoostMap` (maplib/boostmap/boostmap.cpp): the node graph, the
//! site placement and the Voronoi cells.
//!
//! Covers the graph mirror, AddEmptyNodes, layout, extents, ScaleRecentre,
//! cell assembly, polygons, centroid, site points and CheckForValidCells.
//! Comments cite the game function addresses.

use std::collections::{BTreeMap, HashMap};

use crate::crt;
use crate::worldsim::native::layout;
use crate::worldsim::native::voronoi::{Diagram, SOURCE_CATEGORY_SEGMENT_START_POINT};

pub type P2 = (f32, f32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Neighbour {
    Node(usize),
    /// The embedded dummy cell for border segments/corners (state 0).
    Border,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    /// AddChild arg 3: tile id (-1 for blanks).
    pub value: i32,
    pub parent: Option<usize>,
    /// +0x30: 0 default, 1 blank, 2 background, 3 random, 4 blocker.
    pub kind: i32,
    /// +0x2c (AddChild arg 9).
    pub internal: i32,
    /// +0x34 (SetSiteFlags).
    pub site_flags: i32,
    /// +0x74: NODE_TYPE after cell assembly.
    pub state: i32,
    /// +0x20: index into `BoostMap::sites`.
    pub site: Option<usize>,
    pub has_vcell: bool,
    pub poly: Vec<P2>,
    pub neighbours: Vec<Neighbour>,
    pub centroid: P2,
    /// +0x18: SeparateIslands "connected" mark.
    pub connected_mark: bool,
    /// +0x19: GenerateLandmasses visited.
    pub visited: bool,
    /// +0x28: Landmass index.
    pub landmass: Option<usize>,
    /// +0x40: landmass neighbours (GenerateLandmasses flood).
    pub lm_neighbours: Vec<usize>,
    /// +0x38: ForceDisconectedCells.
    pub blocked: bool,
}

/// Landmass (0x1C bytes): task name -> cells, and wormhole partners.
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

#[derive(Default)]
pub struct BoostMap {
    pub width: f64,
    pub height: f64,
    pub nodes: Vec<Node>,
    /// std::map<string, Node*> (+0xb8), name order.
    pub node_by_name: BTreeMap<String, usize>,
    /// Graph vertices (vecS) by index, and the name -> vertex map (+0x3c).
    pub vertex_names: Vec<String>,
    pub vertex_by_name: HashMap<String, usize>,
    /// Graph edges in m_edges (insertion) order: (source, target).
    pub edges: Vec<(usize, usize)>,
    /// Per-vertex out-edge lists (listS, insertion order).
    pub out: Vec<Vec<usize>>,
    /// Landmasses (BoostMap+0xD8), in vector order.
    pub landmasses: Vec<Landmass>,
    /// Road cache (+0xC4): computed once, first caller's join wins.
    pub roads: Option<Vec<Vec<P2>>>,
    /// Position map (doubles), one per vertex.
    pub positions: Vec<(f64, f64)>,
    pub sites: Vec<Site>,
    /// Corner bbox (+0x88..+0xa0): minX, maxX, minY, maxY.
    pub bbox: [f64; 4],
    /// Border segments (doubles) pushed by SetWorldSizeCore.
    pub segments: Vec<[f64; 4]>,
    pub diagram: Diagram,
    /// point_data (+0x68): double copies of the float sites, per assembly.
    pub point_data: Vec<(f64, f64)>,
    /// +0xe4: impassable tile id (SetImpassibleTileType).
    pub default_tile: u8,
}

const TYPE_BLANK: i32 = 1;
const TYPE_BACKGROUND: i32 = 2;
const TYPE_RANDOM: i32 = 3;
const TYPE_BLOCKER: i32 = 4;

impl BoostMap {
    pub fn node(&self, name: &str) -> Option<usize> {
        self.node_by_name.get(name).copied()
    }

    fn new_node(&mut self, name: &str, parent: Option<usize>, value: i32, kind: i32, state: i32) -> usize {
        let node = Node {
            name: name.to_string(),
            value,
            parent,
            kind,
            internal: 0,
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
        };
        self.nodes.push(node);
        let idx = self.nodes.len() - 1;
        // nodes[id] = n overwrites any existing entry.
        self.node_by_name.insert(name.to_string(), idx);
        idx
    }

    /// AddChild (0x004a6d70): creates a node, never a graph vertex.
    pub fn add_child(&mut self, parent: &str, id: &str, value: i32, type_str: Option<&[u8]>, internal: Option<i32>) {
        let kind = match type_str {
            Some(b"blank") => TYPE_BLANK,
            Some(b"background") => TYPE_BACKGROUND,
            Some(b"random") => TYPE_RANDOM,
            Some(b"blocker") => TYPE_BLOCKER,
            _ => 0,
        };
        let state = if kind == TYPE_BLANK || kind == TYPE_RANDOM { 1 } else { 5 };
        let parent = self.node(parent);
        let idx = self.new_node(id, parent, value, kind, state);
        self.nodes[idx].internal = internal.unwrap_or(0);
    }

    /// vertex_for (0x00645bf0).
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

    /// AddLink / AddExternalLink (0x00645ce0): the second name's vertex first.
    pub fn add_link(&mut self, a: &str, b: &str) {
        let vb = self.vertex_for(b);
        let va = self.vertex_for(a);
        self.add_edge(va, vb);
    }

    /// add_edge (0x00645710 / 0x00639740): global list + both out-lists.
    pub fn add_edge(&mut self, u: usize, v: usize) {
        self.edges.push((u, v));
        self.out[u].push(v);
        self.out[v].push(u);
    }

    /// edge(u, v) exists (0x006320e0).
    pub fn has_edge(&self, u: usize, v: usize) -> bool {
        self.out.get(u).is_some_and(|o| o.contains(&v))
    }

    /// ClearNodeLinks (0x00647aa0): clear_vertex(vertex_for(id)).
    pub fn clear_node_links(&mut self, id: &str) {
        let v = self.vertex_for(id);
        self.edges.retain(|&(a, b)| a != v && b != v);
        let nbrs = std::mem::take(&mut self.out[v]);
        for u in nbrs {
            self.out[u].retain(|&w| w != v);
        }
    }

    /// RemoveLink (0x00649a10), direct-edge case only.
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

    /// SetWorldSizeCore (0x00644530): dims, corners/borders, then Build.
    pub fn set_world_size_core(&mut self, w: u32, h: u32) {
        self.width = w as f64;
        self.height = h as f64;
        let (w, h) = (self.width, self.height);
        self.bbox = [0.0, w, 0.0, h];
        self.segments = vec![[0.0, 0.0, w, 0.0], [w, 0.0, w, h], [w, h, 0.0, h], [0.0, h, 0.0, 0.0]];
        self.build();
    }

    /// Build (0x00643a30): re-scatter the position map from minstd_rand(1).
    pub fn build(&mut self) {
        self.positions = layout::random_layout(self.vertex_names.len(), self.width, self.height);
    }

    /// KKAndSites (0x0064a820): one KK run, then sites = float(positions).
    fn kk_and_sites(&mut self) {
        let _ok = layout::kamada_kawai(&mut self.positions, &self.edges);
        self.sites.clear();
        for v in 0..self.vertex_names.len() {
            let name = self.vertex_names[v].clone();
            let node = self.node(&name);
            self.sites.push(Site { x: self.positions[v].0 as f32, y: self.positions[v].1 as f32, name, node });
            if let Some(n) = node {
                self.nodes[n].site = Some(v);
            }
        }
    }

    /// RunLayout(times) (0x0064aad0); the temperature argument is unused.
    pub fn run_layout(&mut self, times: u32) -> Result<(), String> {
        for t in 0..times {
            let _ = t;
            self.kk_and_sites();
        }
        self.build_cells()
    }

    /// AddEmptyNodes (0x00646e10): the only CRT rand() user in GVM.
    pub fn add_empty_nodes(&mut self) {
        let blank_parent = match self.node("BLANK_PARENT") {
            Some(n) => n,
            None => self.new_node("BLANK_PARENT", None, -1, TYPE_BLANK, 1),
        };
        let mut deg: HashMap<String, i32> = HashMap::new();
        for &(u, v) in &self.edges {
            for w in [v, u] {
                *deg.entry(self.sites[w].name.clone()).or_insert(0) += 1;
            }
        }
        let count = self.sites.len();
        for s in 0..count {
            let site = self.sites[s].clone();
            let kind = site.node.map(|n| self.nodes[n].kind).unwrap_or(0);
            let mut nb = 0;
            if deg.get(&site.name).copied().unwrap_or(0) == 1 || kind == TYPE_BACKGROUND {
                nb = (crt::rand() as i32) % 2;
            }
            let mut prev = String::new();
            for j in 0..nb {
                let name = format!("{}:blank{j}", site.name);
                let bx = ((crt::rand() as i32 % 5 - 2) as f64 + self.sites[s].x as f64) as f32;
                let by = ((crt::rand() as i32 % 5 - 2) as f64 + self.sites[s].y as f64) as f32;
                let nd = self.new_node(&name, Some(blank_parent), -1, TYPE_BLANK, 1);
                self.nodes[nd].internal = 1;
                self.sites.push(Site { x: bx, y: by, name: name.clone(), node: Some(nd) });
                let vb = self.vertex_for(&name);
                let vs = self.vertex_for(&site.name);
                self.add_edge(vs, vb);
                if !prev.is_empty() && kind != TYPE_BLOCKER {
                    let vp = self.vertex_for(&prev);
                    let vb2 = self.vertex_for(&name);
                    self.add_edge(vb2, vp);
                }
                prev = name;
            }
        }
    }

    /// Extent getters (0x0062b070 / 0x0062b120).
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
        ((mx as f64 - mn as f64).trunc() as i32) + 2 * border
    }

    /// ScaleRecentre (0x00644cb0), then the cell assembly.
    pub fn scale_recentre(&mut self, sx: f32, sy: f32) -> Result<(), String> {
        for s in &mut self.sites {
            s.x = (s.x as f64 * sx as f64) as f32;
            s.y = (s.y as f64 * sy as f64) as f32;
        }
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN_POSITIVE);
        let (mut min_y, mut max_y) = (f32::MAX, f32::MIN_POSITIVE);
        for s in &self.sites {
            if min_x > s.x { min_x = s.x; }
            if s.x > max_x { max_x = s.x; }
            if min_y > s.y { min_y = s.y; }
            if s.y > max_y { max_y = s.y; }
        }
        let c = self.bbox.map(|v| v as f32);
        let cx = (c[0] as f64 + (c[1] as f64 - c[0] as f64) * 0.5) as f32;
        let scx = (min_x as f64 + (max_x as f64 - min_x as f64) * 0.5) as f32;
        let dx = (cx as f64 - scx as f64) as f32;
        let cy = (c[2] as f64 + (c[3] as f64 - c[2] as f64) * 0.5) as f32;
        let scy = (min_y as f64 + (max_y as f64 - min_y as f64) * 0.5) as f32;
        let dy = (cy as f64 - scy as f64) as f32;
        if dx != 0.0 || dy != 0.0 {
            for s in &mut self.sites {
                s.x = (s.x as f64 + dx as f64) as f32;
                s.y = (s.y as f64 + dy as f64) as f32;
            }
        }
        self.build_cells()
    }

    /// GenerateVoronoiMap (0x004a60a0); Lua args are discarded.
    pub fn generate_voronoi_map(&mut self, border: i32) -> Result<bool, String> {
        self.run_layout(1)?;
        self.add_empty_nodes();
        self.build();
        self.build_cells()?;
        self.run_layout(100)?;
        let kx = self.extent(true, border);
        let ky = self.extent(false, border);
        let k = if kx < ky { ky } else { kx };
        self.set_world_size_core(k as u32, k as u32);
        self.scale_recentre(1.0, 1.0)?;
        Ok(self.check_for_valid_cells())
    }

    /// The rescale ConvertToTileMap applies before building the tile map.
    pub fn convert_rescale(&mut self, size: f64, cap: Option<f64>) -> Result<(u32, u32), String> {
        let mut s = size as f32;
        let (w, h) = (self.width.trunc() as i32, self.height.trunc() as i32);
        if !(s as f64 > w as f64) {
            s = w as f32;
        }
        if !(s as f64 > h as f64) {
            s = h as f32;
        }
        if let Some(cap) = cap {
            let m = cap as f32;
            if !(m > s) {
                s = m;
            }
        }
        let sx = (s as f64 / w as f64) as f32;
        let sy = (s as f64 / h as f64) as f32;
        let isz = s.trunc() as i32 as u32;
        self.set_world_size_core(isz, isz);
        self.scale_recentre(sx, sy)?;
        Ok((isz, isz))
    }

    // ------------------------------------------------------------ cells

    /// The cell assembly (0x00644670).
    pub fn build_cells(&mut self) -> Result<(), String> {
        self.point_data.clear();
        for (i, s) in self.sites.iter().enumerate() {
            let Some(n) = s.node else { return Err(format!("site {} has no node (iter->pData is null)", s.name)) };
            let node = &mut self.nodes[n];
            node.state = 5;
            node.has_vcell = false;
            node.poly.clear();
            node.neighbours.clear();
            node.site = Some(i);
            self.point_data.push((s.x as f64, s.y as f64));
        }
        let points: Vec<(i32, i32)> = self.point_data.iter().map(|&(x, y)| (x.trunc() as i32, y.trunc() as i32)).collect();
        let segments: Vec<(i32, i32, i32, i32)> = self
            .segments
            .iter()
            .map(|s| (s[0].trunc() as i32, s[1].trunc() as i32, s[2].trunc() as i32, s[3].trunc() as i32))
            .collect();
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
            let poly = self.cell_polygon(ci)?;
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
        // State fix-up in Site order, in place.
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
            } else if state == 1 {
                if self.nodes[n].neighbours.iter().all(|nb| nb_state(nb, &self.nodes) >= 3) {
                    self.nodes[n].state = 2;
                }
            }
        }
        Ok(())
    }

    /// SitePolygon (0x00641840) for diagram cell `ci`.
    fn cell_polygon(&self, ci: usize) -> Result<Vec<P2>, String> {
        let vc = self.diagram.cells[ci];
        let mut out = Vec::new();
        if vc.incident_edge < 0 {
            return Ok(out);
        }
        let start = vc.incident_edge as usize;
        let mut e = start;
        loop {
            let edge = self.diagram.edges[e];
            let twin = self.diagram.edges[edge.twin as usize];
            if edge.vertex0 < 0 || twin.vertex0 < 0 {
                let samples = self.clip_infinite_edge(e);
                out.extend(samples.iter().map(|&(x, y)| (x as f32, y as f32)));
                e = edge.next as usize;
                if e == start {
                    break;
                }
                continue;
            }
            let v0 = self.diagram.vertices[edge.vertex0 as usize];
            let v1 = self.diagram.vertices[twin.vertex0 as usize];
            let finite = v0.x.is_finite() && v0.y.is_finite() && v1.x.is_finite() && v1.y.is_finite();
            if !finite || v1.x.abs() > 10000.0 || v0.y.abs() > 10000.0 || v0.x.abs() > 10000.0 || v1.y.abs() > 10000.0 {
                // "Infinite edge!": the edge is dropped.
            } else {
                let mut samples = vec![(v0.x, v0.y), (v1.x, v1.y)];
                if edge.is_linear == 0 {
                    self.sample_curved_edge(e, &mut samples);
                }
                out.extend(samples.iter().map(|&(x, y)| (x as f32, y as f32)));
            }
            e = edge.next as usize;
            if e == start {
                break;
            }
        }
        Ok(out)
    }

    /// retrieve_point (0x00631cc0, Boost 1.53 voronoi_visualizer example).
    fn retrieve_point(&self, ci: usize) -> (f64, f64) {
        let cell = self.diagram.cells[ci];
        if cell.source_category == 0 {
            return self.point_data[cell.source_index as usize];
        }
        let seg = self.segments[cell.source_index as usize - self.point_data.len()];
        if cell.source_category == SOURCE_CATEGORY_SEGMENT_START_POINT { (seg[0], seg[1]) } else { (seg[2], seg[3]) }
    }

    /// clip_infinite_edge (0x006406b0, Boost 1.53 voronoi_visualizer example).
    /// Reached when a site lies outside the border box between layout passes.
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

    /// The double samples of one diagram edge as SitePolygon/PaintEdge build
    /// them; None when the edge is dropped ("Infinite edge!").
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

    /// Node of the cell on the other side of edge `e` (None for borders).
    pub fn twin_site(&self, e: usize) -> usize {
        let twin = self.diagram.edges[self.diagram.edges[e].twin as usize];
        self.diagram.cells[twin.cell as usize].source_index as usize
    }

    /// sample_curved_edge (0x006408f0) + discretize (0x0063f000).
    fn sample_curved_edge(&self, e: usize, samples: &mut Vec<(f64, f64)>) {
        let edge = self.diagram.edges[e];
        let twin = self.diagram.edges[edge.twin as usize];
        let cell = self.diagram.cells[edge.cell as usize];
        let twin_cell = self.diagram.cells[twin.cell as usize];
        let max_dist = (self.bbox[1] - self.bbox[0]) * 0.001;
        let (pc, sc) = if cell.contains_point() { (cell, twin_cell) } else { (twin_cell, cell) };
        let npoints = self.point_data.len();
        let pci = if cell.contains_point() { edge.cell } else { twin.cell } as usize;
        let point = self.retrieve_point(pci);
        let _ = pc;
        let seg = self.segments[sc.source_index as usize - npoints];
        discretize(point, seg, max_dist, samples);
    }

    /// CheckForValidCells (0x0062fd70), in name order.
    pub fn check_for_valid_cells(&self) -> bool {
        for (_name, &n) in &self.node_by_name {
            let c = &self.nodes[n];
            if c.kind == TYPE_BLANK || c.parent.is_none() || c.poly.len() >= 3 {
                continue;
            }
            return false;
        }
        true
    }

    /// SitePoints (0x0063e5f0): integer lattice points in the polygon within
    /// 20 of the site, x outer.
    pub fn site_points(&self, name: &str) -> Option<Vec<P2>> {
        let n = self.node(name)?;
        let node = &self.nodes[n];
        let site = &self.sites[node.site?];
        Some(lattice_points(&node.poly, site.x, site.y, self.width, self.height))
    }
}

/// Lattice rasteriser shared by SitePoints, GetSiteArea and the tile map.
/// Worldgen asks for the same site several times (area, then points), so
/// results are remembered by their exact inputs.
pub fn lattice_points(poly: &[P2], sx: f32, sy: f32, width: f64, height: f64) -> Vec<P2> {
    type Key = (Vec<(u32, u32)>, u32, u32, u64, u64);
    thread_local! {
        static MEMO: std::cell::RefCell<HashMap<Key, Vec<P2>>> = Default::default();
    }
    let key: Key = (poly.iter().map(|p| (p.0.to_bits(), p.1.to_bits())).collect(), sx.to_bits(), sy.to_bits(), width.to_bits(), height.to_bits());
    if let Some(hit) = MEMO.with(|m| m.borrow().get(&key).cloned()) {
        return hit;
    }
    let out = lattice_points_uncached(poly, sx, sy, width, height);
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 4096 {
            m.clear();
        }
        m.insert(key, out.clone());
    });
    out
}

fn lattice_points_uncached(poly: &[P2], sx: f32, sy: f32, width: f64, height: f64) -> Vec<P2> {
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, -f32::MAX, -f32::MAX);
    for &(x, y) in poly {
        if x < minx { minx = x; }
        if x > maxx { maxx = x; }
        if y < miny { miny = y; }
        if y > maxy { maxy = y; }
    }
    let mut out = Vec::new();
    let mut ix = minx.trunc() as i32 as u32;
    while maxx > ix as f32 {
        if width <= ix as f64 {
            return out;
        }
        let mut iy = miny.trunc() as i32 as u32;
        while maxy > iy as f32 {
            if height <= iy as f64 {
                break;
            }
            let (px, py) = (ix as f32, iy as f32);
            let dx = (px as f64 - sx as f64) as f32;
            let dy = (py as f64 - sy as f64) as f32;
            let d2 = (dy as f64 * dy as f64 + dx as f64 * dx as f64) as f32;
            let d = (d2 as f64).sqrt() as f32;
            if 20.0f32 > d && point_in_poly((px, py), poly) {
                out.push((px, py));
            }
            iy += 1;
        }
        ix += 1;
    }
    out
}

/// Sunday winding number (0x00629cd0).
pub fn point_in_poly(p: P2, v: &[P2]) -> bool {
    let n = v.len();
    let mut wn = 0i32;
    for i in 0..n {
        let a = v[i];
        let b = if i == n - 1 { v[0] } else { v[i + 1] };
        let is_left = (b.0 as f64 - a.0 as f64) * (p.1 as f64 - a.1 as f64) - (p.0 as f64 - a.0 as f64) * (b.1 as f64 - a.1 as f64);
        if p.1 >= a.1 {
            if b.1 > p.1 && is_left > 0.0 {
                wn += 1;
            }
        } else if b.1 <= p.1 && is_left < 0.0 {
            wn -= 1;
        }
    }
    wn != 0
}

/// Float shoelace centroid (0x0062a110).
pub fn poly_centroid(p: &[P2]) -> P2 {
    let n = p.len();
    let (mut a, mut sx, mut sy) = (0f32, 0f32, 0f32);
    let f = |v: f32| v as f64;
    for i in 0..n - 1 {
        let (x0, y0, x1, y1) = (p[i].0, p[i].1, p[i + 1].0, p[i + 1].1);
        let c = (f(x0) * f(y1) - f(x1) * f(y0)) as f32;
        a = (f(c) + f(a)) as f32;
        sx = (f(sx) + f(c) * (f(x0) + f(x1))) as f32;
        sy = (f(c) * (f(y0) + f(y1)) + f(sy)) as f32;
    }
    let (x0, y0, x1, y1) = (p[n - 1].0, p[n - 1].1, p[0].0, p[0].1);
    let c = (f(x0) * f(y1) - f(x1) * f(y0)) as f32;
    a = (f(c) + f(a)) as f32;
    let half = (f(a) * 0.5) as f32;
    let k = f(half) * 6.0;
    let rx = ((f(sx) + f(c) * (f(x0) + f(x1))) as f32) as f64 / k;
    let ry = ((f(c) * (f(y0) + f(y1)) + f(sy)) as f32) as f64 / k;
    (rx as f32, ry as f32)
}

/// Boost 1.53 voronoi_visual_utils::discretize with Klei's non-finite guard.
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
