//! Raw bindings to the vendored Lua 5.1.4 C API (only what is used).
#![allow(non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_int, c_void};

#[repr(C)]
pub struct lua_State {
    _private: [u8; 0],
}

pub type lua_Number = f64;
pub type lua_Integer = isize;
pub type RustFn = unsafe extern "C" fn(*mut lua_State, *mut c_void) -> c_int;
pub type FastFn = unsafe extern "C" fn(ud: *mut c_void, args: *const f64, n: c_int, out: *mut f64) -> c_int;

pub const LUA_REGISTRYINDEX: c_int = -10000;
pub const LUA_GLOBALSINDEX: c_int = -10002;
pub const LUA_MULTRET: c_int = -1;

pub const LUA_TNONE: c_int = -1;
pub const LUA_TNIL: c_int = 0;
pub const LUA_TBOOLEAN: c_int = 1;
pub const LUA_TLIGHTUSERDATA: c_int = 2;
pub const LUA_TNUMBER: c_int = 3;
pub const LUA_TSTRING: c_int = 4;
pub const LUA_TTABLE: c_int = 5;
pub const LUA_TFUNCTION: c_int = 6;
pub const LUA_TUSERDATA: c_int = 7;

extern "C" {
    pub fn luaL_newstate() -> *mut lua_State;
    pub fn lua_close(l: *mut lua_State);
    pub fn luaL_openlibs(l: *mut lua_State);
    pub fn lua_dump(l: *mut lua_State, writer: unsafe extern "C" fn(*mut lua_State, *const c_void, usize, *mut c_void) -> c_int, data: *mut c_void) -> c_int;
    pub fn lua_newstate(f: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, usize) -> *mut c_void, ud: *mut c_void) -> *mut lua_State;
    pub fn lua_atpanic(l: *mut lua_State, f: unsafe extern "C" fn(*mut lua_State) -> c_int) -> *mut c_void;
    pub fn luaL_loadbuffer(l: *mut lua_State, buf: *const c_char, sz: usize, name: *const c_char) -> c_int;

    pub fn lua_gettop(l: *mut lua_State) -> c_int;
    pub fn lua_settop(l: *mut lua_State, idx: c_int);
    pub fn lua_pushvalue(l: *mut lua_State, idx: c_int);
    pub fn lua_insert(l: *mut lua_State, idx: c_int);
    pub fn lua_remove(l: *mut lua_State, idx: c_int);

    pub fn lua_type(l: *mut lua_State, idx: c_int) -> c_int;
    pub fn lua_typename(l: *mut lua_State, tp: c_int) -> *const c_char;
    pub fn lua_tonumber(l: *mut lua_State, idx: c_int) -> lua_Number;
    pub fn lua_toboolean(l: *mut lua_State, idx: c_int) -> c_int;
    pub fn lua_tolstring(l: *mut lua_State, idx: c_int, len: *mut usize) -> *const c_char;
    pub fn lua_objlen(l: *mut lua_State, idx: c_int) -> usize;

    pub fn lua_pushnil(l: *mut lua_State);
    pub fn lua_pushnumber(l: *mut lua_State, n: lua_Number);
    pub fn lua_pushlstring(l: *mut lua_State, s: *const c_char, len: usize);
    pub fn lua_pushboolean(l: *mut lua_State, b: c_int);

    pub fn lua_rawgeti(l: *mut lua_State, idx: c_int, n: c_int);
    pub fn lua_rawseti(l: *mut lua_State, idx: c_int, n: c_int);
    pub fn lua_getfield(l: *mut lua_State, idx: c_int, k: *const c_char);
    pub fn lua_setfield(l: *mut lua_State, idx: c_int, k: *const c_char);
    pub fn lua_rawget(l: *mut lua_State, idx: c_int);
    pub fn lua_rawset(l: *mut lua_State, idx: c_int);
    pub fn lua_createtable(l: *mut lua_State, narr: c_int, nrec: c_int);
    pub fn lua_next(l: *mut lua_State, idx: c_int) -> c_int;

    pub fn lua_call(l: *mut lua_State, nargs: c_int, nresults: c_int);
    pub fn lua_pcall(l: *mut lua_State, nargs: c_int, nresults: c_int, errfunc: c_int) -> c_int;
    pub fn luaL_ref(l: *mut lua_State, t: c_int) -> c_int;

    // src/c/shim.c
    pub fn ds_push_rust_function(l: *mut lua_State, f: RustFn, ud: *mut c_void);
    pub fn ds_register_helpers(l: *mut lua_State);
    pub fn ds_push_fast_function(l: *mut lua_State, fast: FastFn, fast_ud: *mut c_void, first: c_int, f: RustFn, ud: *mut c_void);
    pub fn ds_pcall_traceback(l: *mut lua_State, nargs: c_int, nresults: c_int) -> c_int;
    pub fn ds_set_number2int_round(on: c_int);
    // src/c/crt.c
}
