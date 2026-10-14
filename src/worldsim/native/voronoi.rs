//! Boost.Polygon 1.53 voronoi diagrams (the game's builder), via the C++
//! shim in src/c/voronoi.cpp, as index-based Rust structures.

use std::ffi::c_int;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub source_index: u32,
    /// Boost 1.53 SourceCategory (5 bits): 0 single point, 1 segment start,
    /// 2 segment end, 8 initial segment, 9 reverse segment.
    pub source_category: u32,
    pub incident_edge: i32,
}

impl Cell {
    /// `(color & 0x18) == 0` in the game: a point (not segment) source.
    pub fn contains_point(&self) -> bool {
        self.source_category & 0x18 == 0
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub cell: i32,
    pub vertex0: i32,
    pub twin: i32,
    pub next: i32,
    pub prev: i32,
    pub is_linear: u32,
    pub is_primary: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
struct RawDiagram {
    cells: *mut Cell,
    ncells: usize,
    edges: *mut Edge,
    nedges: usize,
    vertices: *mut Vertex,
    nvertices: usize,
}

extern "C" {
    fn ds_voronoi_build(points: *const i32, npoints: usize, segments: *const i32, nsegments: usize, out: *mut RawDiagram) -> c_int;
    fn ds_voronoi_free(vd: *mut RawDiagram);
}

pub const SOURCE_CATEGORY_SEGMENT_START_POINT: u32 = 1;

#[derive(Default, Debug)]
pub struct Diagram {
    pub cells: Vec<Cell>,
    pub edges: Vec<Edge>,
    pub vertices: Vec<Vertex>,
}

impl Diagram {
    /// `points`: (x, y) site coordinates; `segments`: (x0, y0, x1, y1).
    /// Inserted in that order, as the game does (points, then borders).
    pub fn build(points: &[(i32, i32)], segments: &[(i32, i32, i32, i32)]) -> Result<Diagram, String> {
        let flat_points: Vec<i32> = points.iter().flat_map(|&(x, y)| [x, y]).collect();
        let flat_segments: Vec<i32> = segments.iter().flat_map(|&(a, b, c, d)| [a, b, c, d]).collect();
        let mut raw = RawDiagram {
            cells: std::ptr::null_mut(),
            ncells: 0,
            edges: std::ptr::null_mut(),
            nedges: 0,
            vertices: std::ptr::null_mut(),
            nvertices: 0,
        };
        let status = unsafe {
            ds_voronoi_build(flat_points.as_ptr(), points.len(), flat_segments.as_ptr(), segments.len(), &mut raw)
        };
        if status != 0 {
            unsafe { ds_voronoi_free(&mut raw) };
            return Err(format!("boost voronoi construction failed ({status})"));
        }
        let d = unsafe {
            Diagram {
                cells: std::slice::from_raw_parts(raw.cells, raw.ncells).to_vec(),
                edges: std::slice::from_raw_parts(raw.edges, raw.nedges).to_vec(),
                vertices: std::slice::from_raw_parts(raw.vertices, raw.nvertices).to_vec(),
            }
        };
        unsafe { ds_voronoi_free(&mut raw) };
        Ok(d)
    }
}
