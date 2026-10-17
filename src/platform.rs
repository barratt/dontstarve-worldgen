//! Which of DST's builds to reproduce: the Windows game (and Windows
//! dedicated server), or the Linux dedicated server.
//!
//! Both builds draw from the same PCG32 stream, but Klei's `pcg32_rand`
//! reduces each output modulo the C runtime's `RAND_MAX` (32767 for MSVC,
//! 0x7FFFFFFF for glibc), and `std::random_shuffle` comes from a different
//! C++ library (VC9 or libstdc++). Every shuffle the native world generator
//! makes (ReserveSpace, the GetPointsForSite family, mazes) therefore comes
//! out differently, and so does the world from the first set piece placed.
//! The builds also print numbers through their own `printf`.
//!
//! The setting is per thread, like the random stream: a generation sets it
//! for its run and restores Windows afterwards.

use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Platform {
    #[default]
    Windows,
    Linux,
}

impl Platform {
    pub fn parse(s: &str) -> Result<Platform, String> {
        match s.to_ascii_lowercase().as_str() {
            "windows" | "win" => Ok(Platform::Windows),
            "linux" => Ok(Platform::Linux),
            _ => Err(format!("unknown platform {s} (windows, linux)")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Linux => "linux",
        }
    }

    /// The C runtime's `RAND_MAX`, the modulus of `pcg32_rand`.
    pub fn rand_max(self) -> u32 {
        match self {
            Platform::Windows => 0x7FFF,
            Platform::Linux => 0x7FFF_FFFF,
        }
    }

    /// The engine's PLATFORM global.
    pub fn engine_name(self) -> &'static str {
        match self {
            Platform::Windows => "WIN32",
            Platform::Linux => "LINUX",
        }
    }

    /// The build's APP_BUILD_DATE and APP_BUILD_TIME (DST 756039), which the
    /// savedata's meta records.
    pub fn build_stamp(self) -> (&'static str, &'static str) {
        match self {
            Platform::Windows => ("441", "01:32:27"),
            Platform::Linux => ("437", "01:32:42"),
        }
    }
}

thread_local! {
    static CURRENT: Cell<Platform> = const { Cell::new(Platform::Windows) };
}

pub fn set(p: Platform) {
    CURRENT.with(|c| c.set(p));
}

pub fn current() -> Platform {
    CURRENT.with(|c| c.get())
}
