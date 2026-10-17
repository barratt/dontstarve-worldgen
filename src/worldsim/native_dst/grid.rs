//! DST tile grids (u16 tiles + separate u16 tiledata/flags, node-id and
//! safe grids), the TileGroupManager predicates, ConvertToTileMap's fill,
//! the per-site point natives and the tile accessors.

use crate::pcg;
use crate::worldsim::native_dst::map::{DstMap, P2};

/// Flag bits in the high byte of a tiledata cell.
pub const HI_ROAD: u8 = 0x20;
pub const HI_RESERVED: u8 = 0x40;
pub const HI_UNTOUCHED: u8 = 0x80;

/// TileGroupManager (global 0x1405ec738) with the groups the main state's
/// tilegroups.lua builds from constants.lua (vanilla, no mods).
pub mod tgm {
    pub const INVALID_TILE: u16 = 65535;
    pub const DEFAULT_IMPASSABLE: u16 = 1;
    pub const FAKE_GROUND: u16 = 200;
    fn r(t: u16, a: u16, b: u16) -> bool {
        t >= a && t <= b
    }
    pub fn is_land(t: u16) -> bool {
        r(t, 2, 127) || r(t, 256, 10497)
    }
    pub fn is_ocean(t: u16) -> bool {
        r(t, 201, 247) || r(t, 10498, 14594)
    }
    pub fn is_impassable(t: u16) -> bool {
        t == 1 || r(t, 128, 200) || r(t, 14595, 18691)
    }
    pub fn is_invalid(t: u16) -> bool {
        is_impassable(t) || t == 65535
    }
    pub fn is_noise(t: u16) -> bool {
        r(t, 110, 127) || r(t, 8449, 10497)
    }
}

/// Grid (0x1400ba400): W×H u16 cells, row-major, initialised to 0xFFFF.
#[derive(Clone)]
pub struct Grid {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<u16>,
}

impl Grid {
    pub fn new(w: usize, h: usize) -> Grid {
        Grid { w, h, cells: vec![0xFFFF; w * h] }
    }

    pub fn in_range(&self, x: i32, y: i32) -> bool {
        x >= 0 && (x as usize) < self.w && y >= 0 && (y as usize) < self.h
    }

    pub fn hi(&self, i: usize) -> u8 {
        (self.cells[i] >> 8) as u8
    }

    pub fn set_hi(&mut self, i: usize, v: u8) {
        self.cells[i] = (self.cells[i] & 0x00FF) | ((v as u16) << 8);
    }

    /// Serialize (0x1400ba120): base64("VRSN\0" + u32le 1 + u16le cells).
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(9 + 2 * self.cells.len());
        buf.extend_from_slice(b"VRSN\0");
        buf.extend_from_slice(&1u32.to_le_bytes());
        for c in &self.cells {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        base64(&buf)
    }
}

pub fn base64(data: &[u8]) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63]);
        out.push(T[(n >> 12) as usize & 63]);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] } else { b'=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] } else { b'=' });
    }
    out
}

/// The tile map: ids and tiledata (flags in the high byte).
pub struct Tiles {
    pub t: Grid,
    pub d: Grid,
}

impl Tiles {
    pub fn w(&self) -> usize {
        self.t.w
    }
    pub fn h(&self) -> usize {
        self.t.h
    }

    /// Fill (0x140471310): row-major; one boundedrand(16) per cell.
    pub fn fill(&mut self, tile: u16) {
        for i in 0..self.t.cells.len() {
            self.t.cells[i] = tile;
            let mut hi = self.d.hi(i) & 0x9F;
            let r = pcg::boundedrand(16) as u8;
            hi = (hi & 0xF0) | (r & 0x0F);
            self.d.set_hi(i, hi);
        }
    }

    fn idx_of(&self, p: P2) -> Result<usize, String> {
        let (x, y) = (p.0 as i64, p.1 as i64);
        let i = y * self.w() as i64 + x;
        if x < 0 || y < 0 || i < 0 || i as usize >= self.t.cells.len() {
            return Err(format!("tile index ({x}, {y}) outside the grid (the game writes out of bounds)"));
        }
        Ok(i as usize)
    }
}

/// VC9 std::random_shuffle with pcg32_rand (0x14012b580); on Linux,
/// libstdc++'s.
pub fn random_shuffle<T>(a: &mut [T]) {
    let n = a.len();
    if n < 2 {
        return;
    }
    if crate::platform::current() == crate::platform::Platform::Linux {
        // libstdc++ with RAND_MAX = INT_MAX: j = rand() % (i + 1).
        for i in 1..n {
            let j = pcg::rand() as usize % (i + 1);
            if j != i {
                a.swap(i, j);
            }
        }
        return;
    }
    let mut idx: u32 = 2;
    for i in 1..n {
        let mut rm: u32 = 0x7FFF;
        let mut rn: u32 = pcg::rand() & 0x7FFF;
        while rm < idx && rm != 0xFFFF_FFFF {
            rn = (rn << 15) | (pcg::rand() & 0x7FFF);
            rm = (rm << 15) | 0x7FFF;
        }
        let j = (rn % idx) as usize;
        if j != i {
            a.swap(i, j);
        }
        idx += 1;
    }
}

/// ConvertToTileMap core (0x140451d60) after the rescale; `resolve` is the
/// Lua noise callback (tile, noise) -> tile.
pub fn convert_to_tile_map(m: &DstMap, size: usize, resolve: &mut dyn FnMut(u16, f64) -> Result<u16, String>) -> Result<Tiles, String> {
    let mut tiles = Tiles { t: Grid::new(size, size), d: Grid::new(size, size) };
    tiles.fill(m.default_tile);
    for s in 0..m.sites.len() {
        let Some(n) = m.sites[s].node else { continue };
        let node = &m.nodes[n];
        if node.kind == 3 || node.kind == 1 || node.parent.is_none() {
            continue;
        }
        let pts = m.site_points(&m.sites[s].name).unwrap_or_default();
        let mut tile = node.value;
        if tile == tgm::INVALID_TILE {
            tile = m.default_tile;
        }
        if tgm::is_noise(tile) {
            for &p in &pts {
                let nx = (p.0 * 0.015625f32 * 8.0f32 + 0.0f32) as f64;
                let ny = (p.1 * 0.015625f32 * 8.0f32 + 0.0f32) as f64;
                let noise = crate::worldsim::native::tiles::perlin01(nx, ny, 0.0);
                let i = tiles.idx_of(p)?;
                let hi = tiles.d.hi(i) & 0xBF;
                tiles.d.set_hi(i, hi);
                tiles.t.cells[i] = resolve(tile, noise)?;
            }
        } else {
            for &p in &pts {
                let i = tiles.idx_of(p)?;
                let hi = tiles.d.hi(i) & 0xBF;
                tiles.d.set_hi(i, hi);
                tiles.t.cells[i] = tile;
            }
        }
    }
    Ok(tiles)
}

/// GetPointsForSite push helper (0x14012d140) on already shuffled points.
pub fn push_points(m: &DstMap, tiles: &Tiles, pts: &[P2], barren: u16) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let (w, h) = (m.width as i64 as i32, m.height as i64 as i32);
    let in_map = |p: P2| p.0 >= 0.0 && (w as f32) > p.0 && p.1 >= 0.0 && (h as f32) > p.1;
    let cell = |p: P2| (p.1 as i64 as usize) * tiles.w() + p.0 as i64 as usize;
    let mut keep = Vec::new();
    for &p in pts {
        let mut t = tgm::INVALID_TILE;
        if in_map(p) {
            let i = cell(p);
            if i < tiles.t.cells.len() && tiles.d.hi(i) & HI_RESERVED == 0 {
                t = tiles.t.cells[i];
            }
        }
        if !tgm::is_invalid(t) {
            keep.push(p);
        }
    }
    let (mut xs, mut ys, mut ts) = (Vec::new(), Vec::new(), Vec::new());
    for p in keep {
        xs.push(p.0 as i64 as i32 as f64);
        ys.push(p.1 as i64 as i32 as f64);
        let mut t = 0xFFFFu16;
        if in_map(p) {
            let i = cell(p);
            t = tiles.t.cells[i];
            if tiles.d.hi(i) & (HI_ROAD | HI_RESERVED) != 0 {
                t = barren;
            }
        }
        ts.push(t as f64);
    }
    (xs, ys, ts)
}

/// GetRandomPointsForSite core (0x1404597a0): shuffled, filtered, capped.
pub fn random_points(m: &DstMap, tiles: Option<&Tiles>, n: usize, count: i64) -> Vec<P2> {
    let mut pts = m.node_points(n).unwrap_or_default();
    random_shuffle(&mut pts);
    let mut out = Vec::new();
    for p in pts {
        if out.len() as i64 >= count {
            break;
        }
        let Some(t) = tiles else {
            out.push(p);
            continue;
        };
        let mut id = tgm::INVALID_TILE;
        if p.0 >= 0.0 && (t.w() as f32) > p.0 && p.1 >= 0.0 && (t.h() as f32) > p.1 {
            let i = (p.1 as i64 as usize) * t.w() + p.0 as i64 as usize;
            if t.d.hi(i) & HI_RESERVED == 0 {
                id = t.t.cells[i];
            }
        }
        if !tgm::is_invalid(id) {
            out.push(p);
        }
    }
    out
}

/// ReserveSpace core (0x140452180) on a candidate list; returns (x0, y0).
pub fn reserve_core(tiles: &mut Tiles, mut cand: Vec<P2>, size: f32, start_mask: i32, fill_mask: i32, mask: &[u16]) -> Option<(f32, f32)> {
    random_shuffle(&mut cand);
    let (w, h) = (tiles.w() as i32, tiles.h() as i32);
    let fl = |v: f32| v.floor() as i64 as i32;
    for p in cand {
        let i0 = (p.1 as i64 * w as i64 + p.0 as i64) as usize;
        if start_mask & 1 == 0 && tgm::is_invalid(*tiles.t.cells.get(i0)?) {
            continue;
        }
        let x0 = fl(p.0 - size + 0.5);
        let x1 = fl(p.0 + size + 0.5);
        let y0 = fl(p.1 - size + 0.5);
        let y1 = fl(p.1 + size + 0.5);
        if !(x0 > 0 && x1 < w - 1 && y0 > 0 && y1 < h - 1) {
            continue;
        }
        let mut bad = false;
        'check: for x in x0..x1 {
            for y in y0..y1 {
                let i = (y * w + x) as usize;
                if fill_mask & 1 == 0 && tgm::is_invalid(tiles.t.cells[i]) {
                    bad = true;
                    break 'check;
                }
                let hi = tiles.d.hi(i);
                if fill_mask & 2 == 0 && hi & HI_ROAD != 0 {
                    bad = true;
                    break 'check;
                }
                if fill_mask & 4 == 0 && hi & HI_RESERVED != 0 {
                    bad = true;
                    break 'check;
                }
            }
        }
        if bad {
            continue;
        }
        let mut k = 0usize;
        for x in x0..x1 {
            for y in y0..y1 {
                let i = (y * w + x) as usize;
                let hi = tiles.d.hi(i) | HI_RESERVED;
                tiles.d.set_hi(i, hi);
                if k < mask.len() && mask[k] != 0 {
                    tiles.t.cells[i] = mask[k];
                }
                k += 1;
            }
        }
        return Some((x0 as f32, y0 as f32));
    }
    None
}

/// ReserveSpace (0x14012d8c0 → 0x140452680).
pub fn reserve_space(m: &DstMap, tiles: &mut Tiles, id: &str, size: f32, start_mask: i32, fill_mask: i32, layout: i32, mask: &[u16]) -> Option<(f32, f32)> {
    let n = m.node(id)?;
    let pts = m.node_points(n)?;
    let cand = if layout == 1 {
        let s = &m.sites[m.nodes[n].site?];
        pts.iter()
            .find(|p| p.0 as i64 as i32 == s.x as i64 as i32 && p.1 as i64 as i32 == s.y as i64 as i32)
            .map(|p| vec![*p])
            .unwrap_or_default()
    } else {
        pts
    };
    reserve_core(tiles, cand, size, start_mask, fill_mask, mask)
}

/// GetVisualTileAtPosition (0x14012ab20).
pub fn visual_tile(tiles: &Tiles, x: f64, y: f64) -> Result<u16, String> {
    let xx = x as f32 + 0.5f32;
    let yy = y as f32 + 0.5f32;
    let (ix, iy) = (xx as i64 as i32, yy as i64 as i32);
    let w = tiles.w() as i64;
    let get = |cx: i64, cy: i64| -> Result<u16, String> {
        let i = cy * w + cx;
        if i < 0 || i as usize >= tiles.t.cells.len() {
            return Err("GetVisualTileAtPosition outside the grid (unchecked in the game)".into());
        }
        Ok(tiles.t.cells[i as usize])
    };
    let t = get(ix as i64, iy as i64)?;
    if !tgm::is_impassable(t) {
        return Ok(t);
    }
    let fx = xx - (ix as u32) as f32;
    let fy = yy - (iy as u32) as f32;
    let step = |f: f32| -> i64 {
        if 0.24f32 > f {
            -1
        } else if f > 0.76f32 {
            1
        } else {
            0
        }
    };
    let t2 = get(ix as i64 + step(fx), iy as i64 + step(fy))?;
    Ok(if t2 > t { t2 } else { t })
}

/// Line steps (0x14046cd20), float32.
pub fn steps(mut x: f32, mut y: f32, x1: f32, y1: f32) -> Vec<P2> {
    let mut out = Vec::new();
    let adx = (x1 - x).abs();
    let ady = (y1 - y).abs();
    let sx = if x1 > x { 1.0f32 } else { -1.0 };
    let sy = if y1 > y { 1.0f32 } else { -1.0 };
    let n = ady + adx;
    let mut err = 0.0f32;
    if !(n > 0.0) {
        return out;
    }
    let mut i: i64 = 0;
    loop {
        out.push((x.floor(), y.floor()));
        let e1 = err + ady;
        let e2 = err - adx;
        if e2.abs() > e1.abs() {
            x += sx;
            err = e1;
        } else {
            y += sy;
            err = e2;
        }
        i += 1;
        if !(n > i as f32) {
            break;
        }
    }
    out
}

fn clamp(v: f32, dim: usize) -> f32 {
    let mut v = v;
    if 0.0 > v {
        v = 0.0;
    }
    if v >= dim as f32 {
        v = (dim - 1) as f32;
    }
    v
}

/// Line painter (0x14046d050)(p0, p1, tile, setB7, keepLand/only_invalid).
pub fn line(tiles: &mut Tiles, x0: f32, y0: f32, x1: f32, y1: f32, tile: u16, set_b7: bool, only_invalid: bool) {
    let (w, h) = (tiles.w(), tiles.h());
    let (x0, x1) = (clamp(x0, w), clamp(x1, w));
    let (y0, y1) = (clamp(y0, h), clamp(y1, h));
    for (x, y) in steps(x0, y0, x1, y1) {
        let (x, y) = (clamp(x, w), clamp(y, h));
        let i = (y.floor() as i64 as usize) * w + x.floor() as i64 as usize;
        if !only_invalid || tgm::is_invalid(tiles.t.cells[i]) {
            tiles.t.cells[i] = tile;
        }
        let mut hi = tiles.d.hi(i) & 0x7F;
        if set_b7 {
            hi |= 0x80;
        }
        tiles.d.set_hi(i, hi);
    }
}

/// ReplaceSingleNonLandTiles (0x14012a8e0); returns the count.
pub fn replace_single_non_land(g: &mut Grid) -> usize {
    let (w, h) = (g.w, g.h);
    let mut n = 0;
    for x in 1..w.saturating_sub(1) {
        for y in 1..h.saturating_sub(1) {
            let i = y * w + x;
            let (c, l, r, u, d) = (g.cells[i], g.cells[i - 1], g.cells[i + 1], g.cells[i - w], g.cells[i + w]);
            let il = tgm::is_land;
            if (il(c) && !il(l) && !il(r) && !il(u) && !il(d)) || (!il(c) && il(l) && il(r) && il(u) && il(d)) {
                g.cells[i] = l;
                n += 1;
            }
        }
    }
    n
}
