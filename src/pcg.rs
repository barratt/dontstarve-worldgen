//! DST's random stream: PCG32 (thread-local, shared by Lua and native
//! worldgen code on the WorldSim thread) and Klei's math.random.
//! Confirmed in game: 16/16 draws after randomseed(123456) + one discard.

use std::cell::Cell;

const MULT: u64 = 0x5851_f42d_4c95_7f2d;
const INC: u64 = 0xb47c_7397_2972_b7b7; // pcg32_srandom_r initseq 0xda3e39cb94b95bdb

thread_local! {
    static STATE: Cell<u64> = const { Cell::new(0x853c_49e6_748f_ea9b) };
    static STREAM: Cell<u64> = const { Cell::new(0xda3e_39cb_94b9_5bdb) };
}

/// pcg32_srand (0x1403d4370).
pub fn srand(seed: u32) {
    STREAM.with(|s| s.set(INC));
    STATE.with(|s| s.set((seed as u64).wrapping_add(INC).wrapping_mul(MULT).wrapping_add(INC)));
}

/// One raw PCG32 step (0x1403d41b0).
pub fn raw() -> u32 {
    let inc = STREAM.with(|s| s.get());
    STATE.with(|s| {
        let old = s.get();
        s.set(old.wrapping_mul(MULT).wrapping_add(inc));
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        xorshifted.rotate_right((old >> 59) as u32)
    })
}

/// pcg32_rand (0x1403d4210): rejects raw outputs below 4, returns
/// out % RAND_MAX (32767 on Windows, 0x7FFFFFFF on Linux).
pub fn rand() -> u32 {
    let rand_max = crate::platform::current().rand_max();
    loop {
        let out = raw();
        if out >= 4 {
            return out % rand_max;
        }
    }
}

/// boundedrand(n) (0x1403d4340), pcg32_boundedrand_r.
pub fn boundedrand(bound: u32) -> u32 {
    let threshold = bound.wrapping_neg() % bound;
    loop {
        let r = raw();
        if r >= threshold {
            return r % bound;
        }
    }
}

/// Klei math.random's draw (0x140497780): one raw step mapped to [0, 1].
pub fn lua_draw() -> f64 {
    raw() as f64 / 4294967295.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // In game: randomseed(123456),
    // one discarded math.random(), then 16 draws.
    #[test]
    fn matches_game_probe() {
        srand(123456);
        lua_draw();
        let expect = [0.6693573793557839, 0.057633673552804084, 0.10749862485274175, 0.5135727328047093];
        for e in expect {
            assert_eq!(lua_draw().to_bits(), f64::to_bits(e));
        }
    }
}
