//! The game's one random stream: MSVCR90 `rand()`/`srand()`.
//!
//! Lua's `math.random` (via the lmathlib redirect in build.rs) and native
//! WorldSim code both draw from it, interleaved, exactly as on the game's
//! worldgen thread. Verified bit-exact against the game: Lua 5.1.4
//! maps it as `(rand() % RAND_MAX) / RAND_MAX`.

use std::cell::Cell;
use std::ffi::{c_int, c_uint};

pub const RAND_MAX: u32 = 0x7fff;

thread_local! {
    static STATE: Cell<u32> = const { Cell::new(1) };
}

pub fn srand(seed: u32) {
    STATE.with(|s| s.set(seed));
}

pub fn rand() -> u32 {
    STATE.with(|s| {
        let next = s.get().wrapping_mul(214013).wrapping_add(2531011);
        s.set(next);
        (next >> 16) & RAND_MAX
    })
}

/// Lua 5.1.4 `math.random()` with no arguments.
pub fn lua_random() -> f64 {
    (rand() % RAND_MAX) as f64 / RAND_MAX as f64
}

#[no_mangle]
pub extern "C" fn ds_crt_rand() -> c_int {
    rand() as c_int
}

#[no_mangle]
pub extern "C" fn ds_crt_srand(seed: c_uint) {
    srand(seed)
}

/// The vendored musl `exp` and `pow` (vendor/musl-math), for native code
/// whose results must match the game on every platform.
pub fn exp(x: f64) -> f64 {
    unsafe { dsm_exp(x) }
}

pub fn pow(x: f64, y: f64) -> f64 {
    unsafe { dsm_pow(x, y) }
}

extern "C" {
    fn dsm_exp(x: f64) -> f64;
    fn dsm_pow(x: f64, y: f64) -> f64;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Values captured in game: math.randomseed(1791915441) then 16 x
    // math.random().
    #[test]
    fn matches_game_probe() {
        let expected = [
            0.93804742576372568, 0.89422284615619374, 0.39606921597949157, 0.76509903256324963,
            0.57670827356791898, 0.25827814569536423, 0.7465742973113193, 0.5357219153416547,
            0.34717856379894407, 0.053376873073519092, 0.9812921536912137, 0.15320291756950591,
            0.98507644886623735, 0.30350657673879206, 0.83538315988647116, 0.49055452131717886,
        ];
        srand(1791915441);
        for e in expected {
            assert_eq!(lua_random().to_bits(), f64::to_bits(e));
        }
    }
}
