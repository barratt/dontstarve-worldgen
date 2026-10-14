//! The tile grid (game/level/TileGrid.cpp) and ConvertToTileMap's tile
//! passes.
//!
//! Cell = {id, flags}; flags bits: 0-3 random variant nibble (CRT rand),
//! 5 road, 6 reserved, 7 "blocked visual" (id >= 0x80).

use crate::crt;

pub const FLAG_ROAD: u8 = 0x20;
pub const FLAG_RESERVED: u8 = 0x40;
pub const FLAG_HIGH: u8 = 0x80;

#[derive(Clone)]
pub struct TileGrid {
    pub w: usize,
    pub h: usize,
    /// [id, flags], idx = y*w + x.
    pub cells: Vec<[u8; 2]>,
}

impl TileGrid {
    /// TileGrid ctor (0x42ACA0 → cell ctor 0x42AC20 per cell): one rand() each.
    pub fn new(w: usize, h: usize) -> TileGrid {
        let cells = (0..w * h)
            .map(|_| {
                let r = crt::rand() as u8;
                [0xFF, r & 0x0F]
            })
            .collect();
        TileGrid { w, h, cells }
    }

    pub fn idx(&self, x: usize, y: usize) -> usize {
        y * self.w + x
    }

    /// Fill (0x6502C0), row-major, one rand() per cell into the nibble.
    pub fn fill(&mut self, tile: u8) {
        let b7 = if tile > 0x80 { 0x80 } else { 0 };
        for i in 0..self.cells.len() {
            let c = &mut self.cells[i];
            c[0] = tile;
            c[1] &= 0xBF;
            c[1] &= 0xDF;
            let r = crt::rand() as u8;
            c[1] ^= (c[1] ^ r) & 0x0F;
            c[1] = (c[1] & 0x7F) | b7;
        }
    }

    /// Stamp (0x650720): write a site's lattice points.
    pub fn stamp(&mut self, pts: &[(f32, f32)], tile: i32, task: usize, names: &mut [Option<usize>]) {
        let special = (0x7B..=0x7F).contains(&tile) || tile == 0x77 || tile == 0x78;
        for &(x, y) in pts {
            let id = if special { band(tile, perlin01(x as f64 * 0.015625 * 8.0, y as f64 * 0.015625 * 8.0, 0.0)) } else { tile as u8 };
            let i = (y.trunc() as i32 as usize) * self.w + (x.trunc() as i32 as usize);
            let b = (id >= 0x80) as u8;
            let c = &mut self.cells[i];
            c[1] = (c[1] & 0x7F) | (b << 7);
            c[1] = (c[1] & 0xBF) | (b << 6);
            c[0] = id;
            names[i] = Some(task);
        }
    }

    /// Serialize (0x487040): base64("VRSN\0" + u32le 1 + raw cells).
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(9 + 2 * self.cells.len());
        buf.extend_from_slice(b"VRSN\0");
        buf.extend_from_slice(&1u32.to_le_bytes());
        for c in &self.cells {
            buf.extend_from_slice(c);
        }
        base64(&buf)
    }
}

fn base64(data: &[u8]) -> Vec<u8> {
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

/// Ids in water/shore sets (0x45D610 / 0x42CA90).
pub fn is_shore(t: u8) -> bool {
    matches!(t, 0x3A | 0x3C | 0x3D | 0x3F | 0x40)
}

pub fn is_water(t: u8) -> bool {
    matches!(t, 0x37 | 0x38 | 0x39 | 0x3B | 0x3E | 0x30 | 0x34 | 0x40) || is_shore(t)
}

/// Noise bands for the special ids (callbacks 0x6500F0..0x6502A0).
pub fn band(tile: i32, n: f64) -> u8 {
    match tile {
        0x7B => if n < 0.4 { 4 } else { 0x1F },
        0x7C => if n < 0.75 { 1 } else if n < 0.85 { 0x0D } else { 0x9A },
        0x7E => if n < 0.25 { 0x9C } else if n < 0.4 { 0x0D } else if n < 0.7 { 0x10 } else { 0x9B },
        0x7F => {
            if n < 0.25 { 0x9A } else if n < 0.35 { 0x11 } else if n < 0.4 { 4 } else if n < 0.45 { 0x0E }
            else if n < 0.55 { 4 } else if n < 0.65 { 0x10 } else { 0x97 }
        }
        0x77 => if n < 0.5 { 0x29 } else { 4 },
        0x78 => if n < 0.5 { 0x21 } else { 4 },
        // 0x7D and the jump table's default arm.
        _ => {
            if n < 0.25 { 1 } else if n < 0.26 { 2 } else if n < 0.35 { 3 } else if n < 0.4 { 4 }
            else if n < 0.5 { 6 } else if n < 0.75 { 7 } else { 8 }
        }
    }
}

/// Ken Perlin's permutation table (also the road noise table at 0x6FE378).
pub const PERLIN_PERM: [u8; 256] = P0;

const P0: [u8; 256] = [
    151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69, 142, 8, 99, 37, 240, 21, 10, 23,
    190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219, 203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20,
    125, 136, 171, 168, 68, 175, 74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230, 220,
    105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80, 73, 209, 76, 132, 187, 208, 89, 18, 169, 200, 196,
    135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173, 186, 3, 64, 52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255,
    82, 85, 212, 207, 206, 59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163, 70, 221,
    153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232, 178, 185, 112, 104, 218, 246, 97, 228,
    251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162, 241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106,
    157, 184, 84, 204, 176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141, 128, 195, 78,
    66, 215, 61, 156, 180,
];

fn pp(i: usize) -> usize {
    P0[i & 255] as usize
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

fn grad(h: usize, x: f64, y: f64, z: f64) -> f64 {
    let h = h & 15;
    let u = if h < 8 { x } else { y };
    let v = if h < 4 { y } else if h == 12 || h == 14 { x } else { z };
    (if h & 1 != 0 { -u } else { u }) + (if h & 2 != 0 { -v } else { v })
}

/// Improved Noise (0x55D910), remapped to [0,1].
pub fn perlin01(x: f64, y: f64, z: f64) -> f64 {
    let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
    let (xi, yi, zi) = ((fx as i64 & 255) as usize, (fy as i64 & 255) as usize, (fz as i64 & 255) as usize);
    let (x, y, z) = (x - fx, y - fy, z - fz);
    let (u, v, w) = (fade(x), fade(y), fade(z));
    let a = pp(xi) + yi;
    let aa = pp(a) + zi;
    let ab = pp(a + 1) + zi;
    let b = pp(xi + 1) + yi;
    let ba = pp(b) + zi;
    let bb = pp(b + 1) + zi;
    let n = lerp(
        w,
        lerp(v, lerp(u, grad(pp(aa), x, y, z), grad(pp(ba), x - 1.0, y, z)), lerp(u, grad(pp(ab), x, y - 1.0, z), grad(pp(bb), x - 1.0, y - 1.0, z))),
        lerp(
            v,
            lerp(u, grad(pp(aa + 1), x, y, z - 1.0), grad(pp(ba + 1), x - 1.0, y, z - 1.0)),
            lerp(u, grad(pp(ab + 1), x, y - 1.0, z - 1.0), grad(pp(bb + 1), x - 1.0, y - 1.0, z - 1.0)),
        ),
    );
    (n + 1.0) * 0.5
}

/// Gaussian kernel (0x4A4A80): never normalised; f32 stores as marked.
pub fn gaussian_kernel(size: usize, sigma: f32) -> Vec<f32> {
    let f = |v: f64| v as f32;
    let pi_f = 3.14159274f32 as f64;
    let tss = f((sigma as f64 + sigma as f64) * sigma as f64);
    let s = f(f(pi_f * tss as f64) as f64).sqrt() as f32;
    let s = f(s as f64);
    let norm = f(1.0 / s as f64);
    let mut x = -((size as i32 >> 1) as f32);
    let mut k = Vec::with_capacity(size);
    for _ in 0..size {
        let a = f(-(x as f64 * x as f64) / tss as f64);
        let e = f(crate::crt::exp(a as f64));
        k.push(f(e as f64 * norm as f64));
        x = f(x as f64 + 1.0);
    }
    k
}

/// Separable blur pass (0x4A4B30): zero padding, no renormalisation.
pub fn blur(src: &[f32], w: usize, h: usize, k: &[f32], kx: usize, ky: usize) -> Vec<f32> {
    let (rx, ry) = ((kx / 2) as i64, (ky / 2) as i64);
    let mut dst = vec![0f32; w * h];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let mut sum = 0f32;
            for dy in 0..ky as i64 {
                let yy = y - ry + dy;
                for dx in 0..kx as i64 {
                    let xx = x - rx + dx;
                    if xx >= 0 && xx < w as i64 && yy >= 0 && yy < h as i64 {
                        sum = (sum as f64 + src[(yy * w as i64 + xx) as usize] as f64 * k[(dy * kx as i64 + dx) as usize] as f64) as f32;
                    }
                }
            }
            dst[(y * w as i64 + x) as usize] = sum;
        }
    }
    dst
}
