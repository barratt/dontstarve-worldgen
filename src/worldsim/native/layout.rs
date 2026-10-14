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

/// convex_topology::norm (0x00633b20).
fn norm(dx: f64, dy: f64) -> f64 {
    bhypot(bhypot(0.0, dx), dy)
}

/// Hop counts between all vertex pairs (Johnson over unit weights);
/// `f64::MAX` when unreachable.
fn all_pairs_hops(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<f64>> {
    let mut adj = vec![Vec::new(); n];
    for &(u, v) in edges {
        adj[u].push(v);
        adj[v].push(u);
    }
    (0..n)
        .map(|s| {
            let mut d = vec![f64::MAX; n];
            d[s] = 0.0;
            let mut q = vec![s];
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
            d
        })
        .collect()
}

/// kamada_kawai_spring_layout (0x0064a5b0 → run() 0x00648f70), edge
/// length 10, spring constant 1. Returns false (positions untouched) for a
/// disconnected graph, as Boost does.
pub fn kamada_kawai(pos: &mut [(f64, f64)], edges: &[(usize, usize)]) -> bool {
    let n = pos.len();
    let mut dist = all_pairs_hops(n, edges);
    let (k_spring, l) = (1.0f64, 10.0f64);
    let mut ks = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in i + 1..n {
            let dij = dist[i][j];
            if dij == f64::MAX {
                return false;
            }
            dist[i][j] = dij * l;
            dist[j][i] = dij * l;
            let kk = k_spring / (dij * dij);
            ks[i][j] = kk;
            ks[j][i] = kk;
        }
    }
    let cpd = |pos: &[(f64, f64)], m: usize, i: usize| -> (f64, f64) {
        if i == m {
            return (0.0, 0.0);
        }
        let dx = pos[m].0 - pos[i].0;
        let dy = pos[m].1 - pos[i].1;
        let r = dist[m][i] / norm(dx, dy);
        let k = ks[m][i];
        (k * (dx - dx * r), k * (dy - dy * r))
    };
    let cpds = |pos: &[(f64, f64)], m: usize| -> (f64, f64) {
        let (mut x, mut y) = (0.0f64, 0.0f64);
        for i in 0..n {
            let t = cpd(pos, m, i);
            x = x + t.0;
            y = y + t.1;
        }
        (x, y)
    };
    let mut pd: Vec<(f64, f64)> = vec![(0.0, 0.0); n];
    let (mut p, mut delta_p) = (0usize, 0.0f64);
    for u in 0..n {
        pd[u] = cpds(pos, u);
        let d = norm(pd[u].0, pd[u].1);
        if d > delta_p {
            p = u;
            delta_p = d;
        }
    }
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
        let pp: Vec<(f64, f64)> = (0..n).map(|i| cpd(pos, i, p)).collect();
        loop {
            let (mut m00, mut m01, mut m10, mut m11) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for i in 0..n {
                if i == p {
                    continue;
                }
                let d0 = pos[p].0 - pos[i].0;
                let d1 = pos[p].1 - pos[i].1;
                let dd = norm(d0, d1);
                let dd2 = dd * dd;
                let inv3 = 1.0 / (dd * dd2);
                let (k, li) = (ks[p][i], dist[p][i]);
                m00 = (((d0 * d0 - dd2) * li) * inv3 + 1.0) * k + m00;
                m01 = (((li * k) * d0) * d1) * inv3 + m01;
                m10 = (((li * k) * d1) * d0) * inv3 + m10;
                m11 = (((d1 * d1 - dd2) * li) * inv3 + 1.0) * k + m11;
            }
            let (a, b) = pd[p];
            let denom = m00 * m11 - m01 * m10;
            let xnum = m11 * a - m01 * b;
            let ynum = m00 * b - m10 * a;
            let (sx, sy) = (-(xnum / denom), -(ynum / denom));
            pos[p] = (sx + pos[p].0, sy + pos[p].1);
            pd[p] = cpds(pos, p);
            delta_p = norm(pd[p].0, pd[p].1);
            inner += 1;
            if inner > MAX_ITER {
                break;
            }
            if TOL > delta_p {
                break;
            }
        }
        let old_p = p;
        for u in 0..n {
            let od = pp[u];
            let op = cpd(pos, u, old_p);
            let (t0, t1) = (op.0 - od.0, op.1 - od.1);
            pd[u] = (t0 + pd[u].0, t1 + pd[u].1);
            let d = norm(pd[u].0, pd[u].1);
            if d > delta_p {
                p = u;
                delta_p = d;
            }
        }
    }
    true
}
