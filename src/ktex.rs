//! Klei's KTEX textures (minimap icon atlases) decoded to RGBA, the XML
//! atlases that name regions of them, and a minimal PNG writer. Used by
//! `ds-worldgen pack` to cut the games' minimap icons out of the user's
//! installs for the map view of standalone builds.

use std::collections::BTreeMap;

pub struct Image {
    pub width: usize,
    pub height: usize,
    /// RGBA, rows top to bottom.
    pub rgba: Vec<u8>,
}

/// Decodes the largest mip level of a KTEX file (DXT1, DXT3, DXT5, RGBA or RGB).
pub fn decode(tex: &[u8]) -> Result<Image, String> {
    if tex.len() < 18 || &tex[..4] != b"KTEX" {
        return Err("not a KTEX texture".into());
    }
    let header = u32::from_le_bytes(tex[4..8].try_into().unwrap());
    // Current layout: platform 4 bits, pixel format 5, texture type 4,
    // mip count 5, flags 2, then 12 bits all set. Older files pack them
    // into 3, 3, 3, 4 and 1 bits.
    let (format, mips) = if header >> 20 == 0xfff {
        ((header >> 4) & 0x1f, (header >> 13) & 0x1f)
    } else {
        ((header >> 3) & 0x7, (header >> 9) & 0xf)
    };
    let u16_at = |i: usize| u16::from_le_bytes([tex[i], tex[i + 1]]) as usize;
    let (width, height) = (u16_at(8), u16_at(10));
    let data_start = 8 + 10 * mips.max(1) as usize;
    let size = u32::from_le_bytes(tex[14..18].try_into().unwrap()) as usize;
    let data = tex.get(data_start..data_start + size).ok_or("truncated KTEX texture")?;
    let mut rgba = vec![0u8; width * height * 4];
    match format {
        0..=2 => {
            let block_bytes = if format == 0 { 8 } else { 16 };
            let (bw, bh) = (width.div_ceil(4), height.div_ceil(4));
            for by in 0..bh {
                for bx in 0..bw {
                    let off = (by * bw + bx) * block_bytes;
                    let block = data.get(off..off + block_bytes).ok_or("truncated DXT data")?;
                    let px = decode_block(format, block);
                    for y in 0..4 {
                        for x in 0..4 {
                            let (ix, iy) = (bx * 4 + x, by * 4 + y);
                            if ix < width && iy < height {
                                let o = (iy * width + ix) * 4;
                                rgba[o..o + 4].copy_from_slice(&px[y * 4 + x]);
                            }
                        }
                    }
                }
            }
        }
        4 => rgba.copy_from_slice(data.get(..width * height * 4).ok_or("truncated RGBA data")?),
        5 => {
            for (i, p) in data.chunks_exact(3).take(width * height).enumerate() {
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        f => return Err(format!("KTEX pixel format {f} is not supported")),
    }
    Ok(Image { width, height, rgba })
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 0x1f) as u32;
    let g = ((c >> 5) & 0x3f) as u32;
    let b = (c & 0x1f) as u32;
    [((r * 255 + 15) / 31) as u8, ((g * 255 + 31) / 63) as u8, ((b * 255 + 15) / 31) as u8]
}

/// One 4x4 block: format 0 = DXT1, 1 = DXT3, 2 = DXT5.
fn decode_block(format: u32, b: &[u8]) -> [[u8; 4]; 16] {
    let colour = if format == 0 { b } else { &b[8..16] };
    let c0 = u16::from_le_bytes([colour[0], colour[1]]);
    let c1 = u16::from_le_bytes([colour[2], colour[3]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |a: u8, b: u8, wa: u32, wb: u32| ((a as u32 * wa + b as u32 * wb) / (wa + wb)) as u8;
    let mut palette = [[0u8; 4]; 4];
    palette[0] = [p0[0], p0[1], p0[2], 255];
    palette[1] = [p1[0], p1[1], p1[2], 255];
    if c0 > c1 || format != 0 {
        palette[2] = [mix(p0[0], p1[0], 2, 1), mix(p0[1], p1[1], 2, 1), mix(p0[2], p1[2], 2, 1), 255];
        palette[3] = [mix(p0[0], p1[0], 1, 2), mix(p0[1], p1[1], 1, 2), mix(p0[2], p1[2], 1, 2), 255];
    } else {
        palette[2] = [mix(p0[0], p1[0], 1, 1), mix(p0[1], p1[1], 1, 1), mix(p0[2], p1[2], 1, 1), 255];
        palette[3] = [0, 0, 0, 0];
    }
    let indices = u32::from_le_bytes(colour[4..8].try_into().unwrap());
    let mut out = [[0u8; 4]; 16];
    for (i, px) in out.iter_mut().enumerate() {
        *px = palette[((indices >> (2 * i)) & 3) as usize];
    }
    match format {
        1 => {
            let alpha = u64::from_le_bytes(b[0..8].try_into().unwrap());
            for (i, px) in out.iter_mut().enumerate() {
                px[3] = (((alpha >> (4 * i)) & 0xf) * 17) as u8;
            }
        }
        2 => {
            let (a0, a1) = (b[0] as u32, b[1] as u32);
            let mut levels = [0u32; 8];
            levels[0] = a0;
            levels[1] = a1;
            if a0 > a1 {
                for k in 1..7 {
                    levels[k + 1] = ((7 - k as u32) * a0 + k as u32 * a1) / 7;
                }
            } else {
                for k in 1..5 {
                    levels[k + 1] = ((5 - k as u32) * a0 + k as u32 * a1) / 5;
                }
                levels[6] = 0;
                levels[7] = 255;
            }
            let bits = u64::from_le_bytes([b[2], b[3], b[4], b[5], b[6], b[7], 0, 0]);
            for (i, px) in out.iter_mut().enumerate() {
                px[3] = levels[((bits >> (3 * i)) & 7) as usize] as u8;
            }
        }
        _ => {}
    }
    out
}

/// `<Element name="x.png" u1= u2= v1= v2= />` entries of an atlas XML.
pub fn atlas_elements(xml: &str) -> BTreeMap<String, [f64; 4]> {
    let mut out = BTreeMap::new();
    for el in xml.split("<Element").skip(1) {
        let attr = |name: &str| -> Option<String> {
            let start = el.find(&format!("{name}=\""))? + name.len() + 2;
            Some(el[start..start + el[start..].find('"')?].to_string())
        };
        let num = |name: &str| attr(name).and_then(|v| v.parse::<f64>().ok());
        if let (Some(n), Some(u1), Some(u2), Some(v1), Some(v2)) = (attr("name"), num("u1"), num("u2"), num("v1"), num("v2")) {
            out.insert(n, [u1, u2, v1, v2]);
        }
    }
    out
}

/// Cuts an atlas region out (u/v from the atlas XML; v counts from the
/// bottom, as the texture rows are stored bottom-up).
pub fn crop(img: &Image, uv: [f64; 4]) -> Image {
    let [u1, u2, v1, v2] = uv;
    let x0 = (u1 * img.width as f64).round() as usize;
    let x1 = ((u2 * img.width as f64).round() as usize).min(img.width);
    let y0 = (v1 * img.height as f64).round() as usize;
    let y1 = ((v2 * img.height as f64).round() as usize).min(img.height);
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    let mut rgba = Vec::with_capacity(w * h * 4);
    // Stored rows run bottom-up: output row 0 is the region's top (y1 - 1).
    for row in (y0..y1).rev() {
        let start = (row * img.width + x0) * 4;
        rgba.extend_from_slice(&img.rgba[start..start + w * 4]);
    }
    Image { width: w, height: h, rgba }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// RGBA image as a PNG file.
pub fn png(img: &Image) -> Vec<u8> {
    let mut raw = Vec::with_capacity((img.width * 4 + 1) * img.height);
    for row in img.rgba.chunks_exact(img.width * 4) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0xda];
    zlib.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(&raw, 9));
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(img.width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(img.height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &zlib);
    chunk(b"IEND", &[]);
    out
}

/// The image without its fully transparent margins.
pub fn trim(img: &Image) -> Image {
    let (w, h) = (img.width, img.height);
    let opaque = |x: usize, y: usize| img.rgba[(y * w + x) * 4 + 3] > 8;
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if opaque(x, y) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return Image { width: w, height: h, rgba: img.rgba.clone() };
    }
    let mut rgba = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for y in y0..y1 {
        rgba.extend_from_slice(&img.rgba[(y * w + x0) * 4..(y * w + x1) * 4]);
    }
    Image { width: x1 - x0, height: y1 - y0, rgba }
}

/// Half size, averaging 2x2 blocks.
pub fn half(img: &Image) -> Image {
    let (w, h) = (img.width / 2, img.height / 2);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let at = |dx: usize, dy: usize| img.rgba[((2 * y + dy) * img.width + 2 * x + dx) * 4 + c] as u32;
                rgba.push(((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8);
            }
        }
    }
    Image { width: w, height: h, rgba }
}
