//! Don't Starve and Don't Starve Together world generation outside the game.
//!
//! The game's own worldgen Lua (read from the user's install, never
//! bundled) runs on a vendored Lua 5.1.4 built to match the game's 32-bit
//! MSVC build, with the native `WorldSim` provided by Rust.

pub mod api;
pub mod crt;
pub mod dst_env;
pub mod env;
pub mod filter;
pub mod glibc_fmt;
pub mod ktex;
pub mod lua;
pub mod lua_patch;
pub mod msvc_fmt;
pub mod pack;
pub mod pcg;
pub mod platform;
pub mod summary;
pub mod value;
pub mod vfs;
pub mod worldsim;
pub mod zipfile;
