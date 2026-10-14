//! GetEncodedMap (0x4AA0D0): tiles, nav and adjacency.

use crate::worldsim::native::boostmap::BoostMap;
use crate::worldsim::native::tiles::TileGrid;

pub struct Encoded {
    pub tiles: Vec<u8>,
    pub nav: Vec<u8>,
    /// 1-based cell indices per cell.
    pub adj: Vec<Vec<f64>>,
}

pub fn get_encoded_map(m: &BoostMap, g: &mut TileGrid) -> Encoded {
    // Pass 1: single-tile fix-up on the real grid, column-major.
    let blk = |t: u8| t == 1 || t >= 0x7C;
    let land = |t: u8| t > 1 && t < 0x7C;
    for x in 1..g.w.saturating_sub(1) {
        for y in 1..g.h.saturating_sub(1) {
            let id = |g: &TileGrid, x: usize, y: usize| g.cells[g.idx(x, y)][0];
            let v = id(g, x, y);
            let (l, r, u, d) = (id(g, x - 1, y), id(g, x + 1, y), id(g, x, y - 1), id(g, x, y + 1));
            let i = g.idx(x, y);
            if (2..=0x7B).contains(&v) {
                if blk(l) && blk(r) && blk(u) && blk(d) {
                    g.cells[i][0] = l;
                }
            } else if (v == 1 || v >= 0x7C) && land(l) && land(r) && land(u) && land(d) {
                g.cells[i][0] = l;
            }
        }
    }
    let tiles = g.serialize();

    // Nav grid: its constructor's W*H rand() draws are discarded (memset).
    let (w, h) = (m.width.trunc() as usize, m.height.trunc() as usize);
    let mut nav = TileGrid::new(w, h);
    for c in nav.cells.iter_mut() {
        *c = [0xFF, 0x7F];
    }
    let mut cells: Vec<usize> = Vec::new();
    for (lm, l) in m.landmasses.iter().enumerate() {
        for list in l.map.values() {
            for &c in list {
                let ci = cells.len();
                cells.push(c);
                if let Some(pts) = m.site_points(&m.nodes[c].name) {
                    for (px, py) in pts {
                        let i = (py.trunc() as usize) * g.w + px.trunc() as usize;
                        let t = g.cells[i][0];
                        if t > 1 && t < 0x80 {
                            nav.cells[i][1] = (nav.cells[i][1] & 0x80) | (lm as u8 & 0x7F);
                            nav.cells[i][0] = ci as u8;
                            nav.cells[i][1] |= 0x80;
                        }
                    }
                }
            }
        }
    }
    for x in 0..w {
        for y in 0..h {
            let i = y * g.w + x;
            let t = g.cells[i][0];
            if t > 1 && t < 0x80 && nav.cells[i][1] & 0x80 == 0 {
                nav.cells[i] = [0xFF, 0xFF];
            }
        }
    }
    for i in 0..g.w * g.h {
        let t = g.cells[i][0];
        if matches!(t, 0x3A | 0x3C | 0x3D | 0x3F | 0x40) {
            nav.cells[i] = [0xFF, 0x7F];
        } else if matches!(t, 0x37 | 0x38 | 0x39 | 0x3B | 0x3E | 0x30 | 0x34) {
            nav.cells[i] = [0xFF, 0xFF];
        }
    }
    let nav_s = nav.serialize();

    // Adjacency: landmass neighbour lists as 1-based cell indices (no break
    // on duplicates).
    let adj = cells
        .iter()
        .map(|&c| {
            let mut v = Vec::new();
            for &nb in &m.nodes[c].lm_neighbours {
                for (j, &cj) in cells.iter().enumerate() {
                    if cj == nb {
                        v.push((j + 1) as f64);
                    }
                }
            }
            v
        })
        .collect();
    Encoded { tiles, nav: nav_s, adj }
}
