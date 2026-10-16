//! Site placement: Boost.Graph random_graph_layout (minstd_rand seeded with
//! 1 on every build) followed by kamada_kawai_spring_layout with Klei's
//! done functor, operation order kept exactly (each detail was confirmed
//! bit-exact against the game).

/// `qword 0x006E4670`: 1/2147483646 rounded (multiply, never divide).
const MINSTD_SCALE: f64 = f64::from_bits(0x3E00_0000_0040_0000);
const EPS: f64 = f64::from_bits(0x3CB0_0000_0000_0000); // 2^-52, qword 0x006E4270
const TOL: f64 = 0.001; // qword 0x006E40F8
const MAX_ITER: i32 = 100;

/// `uniform_01<minstd_rand>` over a fresh `minstd_rand(1)`.
pub struct Minstd(u32);

impl Minstd {
    pub fn new() -> Minstd {
        Minstd(1)
    }

    pub fn uniform01(&mut self) -> f64 {
        loop {
            self.0 = ((self.0 as u64 * 48271) % 2147483647) as u32;
            let v = (self.0 as i32 - 1) as f64 * MINSTD_SCALE;
            if !(1.0 <= v) {
                return v;
            }
        }
    }
}

/// Build (0x00643a30): one random point per vertex in [0,w]x[0,h], x then y.
pub fn random_layout(n: usize, w: f64, h: f64) -> Vec<(f64, f64)> {
    let mut gen = Minstd::new();
    let (left, right) = (0f64.min(w), 0f64.max(w));
    let (top, bottom) = (0f64.min(h), 0f64.max(h));
    (0..n)
        .map(|_| {
            let x = gen.uniform01() * (right - left) + left;
            let y = gen.uniform01() * (bottom - top) + top;
            (x, y)
        })
        .collect()
}

/// boost::math::hypot as compiled in the game (0x0062f810).
fn bhypot(x: f64, y: f64) -> f64 {
    let (x, y) = (x.abs(), y.abs());
    let (a, b) = if y > x { (y, x) } else { (x, y) };
    if !(a * EPS < b) {
        return a;
    }
    let r = b / a;
    a * (r * r + 1.0).sqrt()
}

/// convex_topology::norm (0x00633b20): bhypot(bhypot(0, dx), dy), where
/// the inner call is exactly |dx|.
fn norm(dx: f64, dy: f64) -> f64 {
    bhypot(dx.abs(), dy)
}

/// Hop counts between all vertex pairs (Johnson over unit weights), row
/// major; `f64::MAX` when unreachable.
fn all_pairs_hops(n: usize, edges: &[(usize, usize)]) -> Vec<f64> {
    let mut adj = vec![Vec::new(); n];
    for &(u, v) in edges {
        adj[u].push(v);
        adj[v].push(u);
    }
    let mut out = vec![f64::MAX; n * n];
    let mut q = Vec::with_capacity(n);
    for s in 0..n {
        let d = &mut out[s * n..(s + 1) * n];
        d[s] = 0.0;
        q.clear();
        q.push(s);
        let mut qi = 0;
        while qi < q.len() {
            let u = q[qi];
            qi += 1;
            for &v in &adj[u] {
                if d[v] == f64::MAX {
                    d[v] = d[u] + 1.0;
                    q.push(v);
                }
            }
        }
    }
    out
}

/// The spring lengths and constants of a graph (n x n, row major), or
/// None when it is disconnected.
struct Springs {
    n: usize,
    edges: Vec<(usize, usize)>,
    dist: Vec<f64>,
    ks: Vec<f64>,
}

impl Springs {
    fn new(n: usize, edges: &[(usize, usize)]) -> Option<Springs> {
        let mut dist = all_pairs_hops(n, edges);
        let (k_spring, l) = (1.0f64, 10.0f64);
        let mut ks = vec![0.0f64; n * n];
        for i in 0..n {
            for j in i + 1..n {
                let dij = dist[i * n + j];
                if dij == f64::MAX {
                    return None;
                }
                dist[i * n + j] = dij * l;
                dist[j * n + i] = dij * l;
                let kk = k_spring / (dij * dij);
                ks[i * n + j] = kk;
                ks[j * n + i] = kk;
            }
        }
        Some(Springs { n, edges: edges.to_vec(), dist, ks })
    }
}

thread_local! {
    /// GenerateVoronoiMap lays out the same graph a hundred times in a row.
    static SPRINGS: std::cell::RefCell<Option<std::rc::Rc<Springs>>> = const { std::cell::RefCell::new(None) };
}

/// bhypot without branches (the same result for every input), so the
/// row loops below vectorise.
#[inline(always)]
fn norm_sel(dx: f64, dy: f64) -> f64 {
    let (x, y) = (dx.abs(), dy.abs());
    let gt = y > x;
    let a = if gt { y } else { x };
    let b = if gt { x } else { y };
    let r = b / a;
    let h = a * (r * r + 1.0).sqrt();
    if a * EPS < b { h } else { a }
}

/// cpd(m, i) (the spring force on m from i) for i in `range`, into ox/oy
/// (indexed like i). The caller handles i == m, which is (0, 0).
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn cpd_row_impl(xs: &[f64], ys: &[f64], dist: &[f64], ks: &[f64], m: usize, range: std::ops::Range<usize>, ox: &mut [f64], oy: &mut [f64]) {
    let (px, py) = (xs[m], ys[m]);
    let (xs, ys, dist, ks) = (&xs[range.clone()], &ys[range.clone()], &dist[range.clone()], &ks[range.clone()]);
    let (ox, oy) = (&mut ox[range.clone()], &mut oy[range]);
    let n = xs.len();
    let (ys, dist, ks, ox, oy) = (&ys[..n], &dist[..n], &ks[..n], &mut ox[..n], &mut oy[..n]);
    for i in 0..n {
        let dx = px - xs[i];
        let dy = py - ys[i];
        let r = dist[i] / norm_sel(dx, dy);
        let k = ks[i];
        ox[i] = k * (dx - dx * r);
        oy[i] = k * (dy - dy * r);
    }
}

/// One pass over p's row at p's current position: cpd(p, i) into rx/ry
/// (with (0, 0) at p) and the Newton step's Hessian terms into c (the
/// entry at p is not used).
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn p_row_impl(xs: &[f64], ys: &[f64], dist: &[f64], ks: &[f64], p: usize, rx: &mut [f64], ry: &mut [f64], c: &mut [Vec<f64>; 4]) {
    let (px, py) = (xs[p], ys[p]);
    let [c00, c01, c10, c11] = c;
    // Equal lengths let the compiler drop bounds checks and vectorise.
    let n = xs.len();
    let (ys, dist, ks, rx, ry) = (&ys[..n], &dist[..n], &ks[..n], &mut rx[..n], &mut ry[..n]);
    let (c00, c01, c10, c11) = (&mut c00[..n], &mut c01[..n], &mut c10[..n], &mut c11[..n]);
    for i in 0..n {
        let d0 = px - xs[i];
        let d1 = py - ys[i];
        let dd = norm_sel(d0, d1);
        let (k, li) = (ks[i], dist[i]);
        let r = li / dd;
        rx[i] = k * (d0 - d0 * r);
        ry[i] = k * (d1 - d1 * r);
        let dd2 = dd * dd;
        let inv3 = 1.0 / (dd * dd2);
        c00[i] = (((d0 * d0 - dd2) * li) * inv3 + 1.0) * k;
        c01[i] = (((li * k) * d0) * d1) * inv3;
        c10[i] = (((li * k) * d1) * d0) * inv3;
        c11[i] = (((d1 * d1 - dd2) * li) * inv3 + 1.0) * k;
    }
    rx[p] = 0.0;
    ry[p] = 0.0;
}

/// The row passes are divide- and sqrt-bound; on x86-64 they also exist
/// compiled for AVX (four lanes instead of two), chosen at run time. IEEE
/// division and square root are correctly rounded at any width, and no
/// operation is fused, so both versions give the same bits.
#[cfg(target_arch = "x86_64")]
fn have_avx() -> bool {
    use std::sync::OnceLock;
    static AVX: OnceLock<bool> = OnceLock::new();
    *AVX.get_or_init(|| std::arch::is_x86_feature_detected!("avx"))
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
#[allow(clippy::too_many_arguments)]
unsafe fn p_row_avx(xs: &[f64], ys: &[f64], dist: &[f64], ks: &[f64], p: usize, rx: &mut [f64], ry: &mut [f64], c: &mut [Vec<f64>; 4]) {
    p_row_impl(xs, ys, dist, ks, p, rx, ry, c)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
#[allow(clippy::too_many_arguments)]
unsafe fn cpd_row_avx(xs: &[f64], ys: &[f64], dist: &[f64], ks: &[f64], m: usize, range: std::ops::Range<usize>, ox: &mut [f64], oy: &mut [f64]) {
    cpd_row_impl(xs, ys, dist, ks, m, range, ox, oy)
}

#[allow(clippy::too_many_arguments)]
fn p_row(xs: &[f64], ys: &[f64], dist: &[f64], ks: &[f64], p: usize, rx: &mut [f64], ry: &mut [f64], c: &mut [Vec<f64>; 4]) {
    #[cfg(target_arch = "x86_64")]
    if have_avx() {
        return unsafe { p_row_avx(xs, ys, dist, ks, p, rx, ry, c) };
    }
    p_row_impl(xs, ys, dist, ks, p, rx, ry, c)
}

#[allow(clippy::too_many_arguments)]
fn cpd_row(xs: &[f64], ys: &[f64], dist: &[f64], ks: &[f64], m: usize, range: std::ops::Range<usize>, ox: &mut [f64], oy: &mut [f64]) {
    #[cfg(target_arch = "x86_64")]
    if have_avx() {
        return unsafe { cpd_row_avx(xs, ys, dist, ks, m, range, ox, oy) };
    }
    cpd_row_impl(xs, ys, dist, ks, m, range, ox, oy)
}

/// The pair forces T[m*n + i] = cpd(m, i) for the positions `xs`/`ys`,
/// kept between calls: GenerateVoronoiMap's hundred passes each move only
/// a few vertices, so only their rows and columns are recomputed.
struct Forces {
    springs: std::rc::Rc<Springs>,
    xs: Vec<f64>,
    ys: Vec<f64>,
    tx: Vec<f64>,
    ty: Vec<f64>,
}

thread_local! {
    static FORCES: std::cell::RefCell<Option<Forces>> = const { std::cell::RefCell::new(None) };
}

impl Forces {
    fn update(&mut self, xs: &[f64], ys: &[f64]) {
        let n = xs.len();
        let s = self.springs.clone();
        let changed: Vec<usize> = (0..n).filter(|&i| self.xs[i].to_bits() != xs[i].to_bits() || self.ys[i].to_bits() != ys[i].to_bits()).collect();
        if changed.is_empty() {
            return;
        }
        let rows: Vec<usize> = if changed.len() * 2 > n { (0..n).collect() } else { changed };
        let all = rows.len() == n;
        for &m in &rows {
            // All rows: each pair once (m < i); some rows: the full row.
            let range = if all { m + 1..n } else { 0..n };
            let (tx, ty) = (&mut self.tx[m * n..(m + 1) * n], &mut self.ty[m * n..(m + 1) * n]);
            cpd_row(xs, ys, &s.dist[m * n..(m + 1) * n], &s.ks[m * n..(m + 1) * n], m, range.clone(), tx, ty);
            tx[m] = 0.0;
            ty[m] = 0.0;
            for i in range {
                if i != m {
                    self.tx[i * n + m] = -self.tx[m * n + i];
                    self.ty[i * n + m] = -self.ty[m * n + i];
                }
            }
        }
        self.xs.copy_from_slice(xs);
        self.ys.copy_from_slice(ys);
    }
}

/// kamada_kawai_spring_layout (0x0064a5b0 → run() 0x00648f70), edge
/// length 10, spring constant 1. Returns false (positions untouched) for a
/// disconnected graph, as Boost does.
///
/// Every value is computed with the operations, and summed in the order,
/// of Boost's loops. Exact identities save work: cpd(i, m) is -cpd(m, i)
/// (IEEE negation and subtraction are symmetric), norm's inner
/// bhypot(0, dx) is |dx|, a pair force depends only on its two positions
/// (so it is reused while neither moves), and a Newton step's Hessian at
/// p's new position comes from the same pass as p's new forces.
pub fn kamada_kawai(pos: &mut [(f64, f64)], edges: &[(usize, usize)]) -> bool {
    let n = pos.len();
    let cached = SPRINGS.with(|c| c.borrow().as_ref().filter(|s| s.n == n && s.edges == edges).cloned());
    let springs = match cached {
        Some(s) => s,
        None => {
            let Some(s) = Springs::new(n, edges) else { return false };
            let s = std::rc::Rc::new(s);
            SPRINGS.with(|c| *c.borrow_mut() = Some(s.clone()));
            s
        }
    };
    let (dist, ks) = (&springs.dist[..], &springs.ks[..]);
    let row = |m: usize| m * n..(m + 1) * n;
    let mut xs: Vec<f64> = pos.iter().map(|p| p.0).collect();
    let mut ys: Vec<f64> = pos.iter().map(|p| p.1).collect();

    // pd[u] = cpds(pos, u): row u of the pair forces summed in index order.
    let mut forces = FORCES.with(|f| f.borrow_mut().take()).filter(|f| std::rc::Rc::ptr_eq(&f.springs, &springs)).unwrap_or_else(|| Forces {
        springs: springs.clone(),
        xs: vec![f64::NAN; n],
        ys: vec![f64::NAN; n],
        tx: vec![0.0; n * n],
        ty: vec![0.0; n * n],
    });
    forces.update(&xs, &ys);
    let mut pd: Vec<(f64, f64)> = (0..n)
        .map(|u| {
            let (mut x, mut y) = (0.0f64, 0.0f64);
            for i in row(u) {
                x = x + forces.tx[i];
                y = y + forces.ty[i];
            }
            (x, y)
        })
        .collect();
    FORCES.with(|f| *f.borrow_mut() = Some(forces));

    let (mut p, mut delta_p) = (0usize, 0.0f64);
    for u in 0..n {
        let d = norm(pd[u].0, pd[u].1);
        if d > delta_p {
            p = u;
            delta_p = d;
        }
    }
    let (mut rx, mut ry) = (vec![0.0f64; n], vec![0.0f64; n]);
    let mut c = [vec![0.0f64; n], vec![0.0f64; n], vec![0.0f64; n], vec![0.0f64; n]];
    let (mut ppx, mut ppy) = (vec![0.0f64; n], vec![0.0f64; n]);
    // Klei done functor: {limit=1, inner=0, outer=0, max=100, last=0.0}.
    let (mut outer, mut last) = (0i32, 0.0f64);
    loop {
        outer += 1;
        let mut inner = 0i32;
        if outer > MAX_ITER {
            break;
        }
        let mut diff = last - delta_p;
        if 0.0 > diff {
            diff = -diff;
        }
        last = delta_p;
        if TOL > diff {
            break;
        }
        // pp[i] = cpd(pos, i, p) = -cpd(p, i); cpd(p, p) = (0, 0).
        p_row(&xs, &ys, &dist[row(p)], &ks[row(p)], p, &mut rx, &mut ry, &mut c);
        for i in 0..n {
            ppx[i] = -rx[i];
            ppy[i] = -ry[i];
        }
        ppx[p] = 0.0;
        ppy[p] = 0.0;
        loop {
            let (mut m00, mut m01, mut m10, mut m11) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for i in 0..n {
                if i == p {
                    continue;
                }
                m00 = c[0][i] + m00;
                m01 = c[1][i] + m01;
                m10 = c[2][i] + m10;
                m11 = c[3][i] + m11;
            }
            let (a, b) = pd[p];
            let denom = m00 * m11 - m01 * m10;
            let xnum = m11 * a - m01 * b;
            let ynum = m00 * b - m10 * a;
            let (sx, sy) = (-(xnum / denom), -(ynum / denom));
            xs[p] = sx + xs[p];
            ys[p] = sy + ys[p];
            p_row(&xs, &ys, &dist[row(p)], &ks[row(p)], p, &mut rx, &mut ry, &mut c);
            let (mut x, mut y) = (0.0f64, 0.0f64);
            for i in 0..n {
                x = x + rx[i];
                y = y + ry[i];
            }
            pd[p] = (x, y);
            delta_p = norm(pd[p].0, pd[p].1);
            inner += 1;
            if inner > MAX_ITER {
                break;
            }
            if TOL > delta_p {
                break;
            }
        }
        // op = cpd(pos, u, old_p) = -cpd(old_p, u), with rx/ry the row of p
        // at its final position.
        let old_p = p;
        for u in 0..n {
            let od = (ppx[u], ppy[u]);
            let op = if u == old_p { (0.0, 0.0) } else { (-rx[u], -ry[u]) };
            let (t0, t1) = (op.0 - od.0, op.1 - od.1);
            pd[u] = (t0 + pd[u].0, t1 + pd[u].1);
            let d = norm(pd[u].0, pd[u].1);
            if d > delta_p {
                p = u;
                delta_p = d;
            }
        }
    }
    for (i, q) in pos.iter_mut().enumerate() {
        *q = (xs[i], ys[i]);
    }
    true
}
