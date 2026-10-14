//! GenerateBlendedMap (0x14012bc80) helpers: the float Gaussian kernel
//! (0x140438820) and the separable zero-padded convolution (0x140438910).
//! `expf` and `powf` are computed in double precision and rounded to
//! float, which gives the game's results on every world checked.

pub fn expf(x: f32) -> f32 {
    crate::crt::exp(x as f64) as f32
}

pub fn powf(x: f32, y: f32) -> f32 {
    crate::crt::pow(x as f64, y as f64) as f32
}

/// Kernel 0x140438820: not normalised; PI_f = 3.1415927f.
pub fn kernel(k: usize, sigma: f32) -> Vec<f32> {
    let s2 = (sigma * 2.0) * sigma;
    let norm = 1.0f32 / (std::f32::consts::PI * s2).sqrt();
    let mut x = -((k >> 1) as f32);
    let mut out = Vec::with_capacity(k);
    for _ in 0..k {
        out.push(expf(-(x * x / s2)) * norm);
        x += 1.0;
    }
    out
}

/// Conv 0x140438910: zero padding, sequential float accumulation.
pub fn conv(src: &[f32], w: usize, h: usize, kern: &[f32], kw: usize, kh: usize) -> Vec<f32> {
    let (rx, ry) = ((kw / 2) as i64, (kh / 2) as i64);
    let mut dst = vec![0f32; w * h];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let mut s = 0f32;
            for j in 0..kh as i64 {
                let sy = y - ry + j;
                for i in 0..kw as i64 {
                    let sx = x - rx + i;
                    if sx >= 0 && sx < w as i64 && sy >= 0 && sy < h as i64 {
                        s += src[(sy * w as i64 + sx) as usize] * kern[(j * kw as i64 + i) as usize];
                    }
                }
            }
            dst[(y * w as i64 + x) as usize] = s;
        }
    }
    dst
}

/// Blur 0x140438c40: horizontal then vertical.
pub fn blur(buf: &[f32], w: usize, h: usize, k: i32, sigma: f32) -> Vec<f32> {
    let k = (k | 1).clamp(1, 255) as usize;
    let kern = kernel(k, sigma);
    let tmp = conv(buf, w, h, &kern, k, 1);
    conv(&tmp, w, h, &kern, 1, k)
}
