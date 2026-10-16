//! A thin, single-threaded wrapper over a Lua 5.1.4 state.
//!
//! Rust functions are registered as boxed closures. Each closure receives a
//! `Ctx` for reading arguments and pushing results, and returns
//! `Ok(nresults)` or `Err(message)`. The C trampoline raises errors, so no
//! longjmp ever crosses Rust frames. Panics are caught and become errors.

pub mod ffi;

use std::ffi::{c_int, c_void, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use ffi::*;

pub use crate::value::Value;

pub type Callback = Box<dyn FnMut(&mut Ctx) -> Result<i32, String>>;

pub struct Lua {
    state: *mut lua_State,
    // Keeps registered closures alive for the life of the state.
    callbacks: Vec<*mut Callback>,
    fast_data: Vec<Box<dyn std::any::Any>>,
}

pub struct Ctx {
    pub l: *mut lua_State,
}

impl Ctx {
    pub fn top(&self) -> i32 {
        unsafe { lua_gettop(self.l) }
    }

    pub fn arg(&self, idx: i32) -> Result<Value, String> {
        unsafe { Value::from_stack(self.l, idx) }
    }

    pub fn args(&self) -> Result<Vec<Value>, String> {
        self.args_from(1)
    }

    pub fn args_from(&self, first: i32) -> Result<Vec<Value>, String> {
        (first..=self.top()).map(|i| self.arg(i)).collect()
    }

    pub fn arg_str(&self, idx: i32) -> Option<Vec<u8>> {
        unsafe {
            if lua_type(self.l, idx) != LUA_TSTRING && lua_type(self.l, idx) != LUA_TNUMBER {
                return None;
            }
            let mut len = 0usize;
            let p = lua_tolstring(self.l, idx, &mut len);
            Some(std::slice::from_raw_parts(p as *const u8, len).to_vec())
        }
    }

    pub fn arg_num(&self, idx: i32) -> Option<f64> {
        unsafe {
            if lua_type(self.l, idx) == LUA_TNUMBER {
                Some(lua_tonumber(self.l, idx))
            } else {
                None
            }
        }
    }

    pub fn push(&mut self, v: &Value) {
        unsafe { v.push(self.l) }
    }

    pub fn push_all(&mut self, vs: &[Value]) -> i32 {
        for v in vs {
            self.push(v);
        }
        vs.len() as i32
    }

    /// Loads a chunk and leaves the function on the stack, or pushes nil and
    /// returns the error message.
    pub fn load_chunk(&mut self, source: &[u8], chunkname: &str) -> Result<(), String> {
        let name = CString::new(chunkname).map_err(|e| e.to_string())?;
        unsafe {
            let status = load_cached(self.l, source, &name);
            if status != 0 {
                let msg = Value::from_stack(self.l, -1)?.to_display();
                lua_settop(self.l, -2);
                return Err(msg);
            }
        }
        Ok(())
    }
}

thread_local! {
    /// The Lua state of the Rust callback currently running (innermost).
    static CURRENT: std::cell::Cell<*mut lua_State> = const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Calls a Lua function held as `Value::Function(registry ref)` from inside
/// a native (Rust) call — e.g. DST's SetResolveNoiseFunction callbacks,
/// which the engine calls through pcall. Returns its results, or the Lua
/// error message.
pub fn call_function(func: &Value, args: &[Value]) -> Result<Vec<Value>, String> {
    let Value::Function(r) = func else { return Err("call_function: not a function".into()) };
    let l = CURRENT.with(|c| c.get());
    if l.is_null() {
        return Err("call_function: no Lua state is active".into());
    }
    unsafe {
        let base = lua_gettop(l);
        lua_rawgeti(l, LUA_REGISTRYINDEX, *r);
        for a in args {
            a.push(l);
        }
        let status = lua_pcall(l, args.len() as c_int, LUA_MULTRET, 0);
        if status != 0 {
            let msg = Value::from_stack(l, -1).map(|v| v.to_display()).unwrap_or_default();
            lua_settop(l, base);
            return Err(msg);
        }
        let top = lua_gettop(l);
        let mut out = Vec::new();
        for i in base + 1..=top {
            out.push(Value::from_stack(l, i)?);
        }
        lua_settop(l, base);
        Ok(out)
    }
}

unsafe extern "C" fn dispatch(l: *mut lua_State, ud: *mut c_void) -> c_int {
    let cb = &mut *(ud as *mut Callback);
    let mut ctx = Ctx { l };
    let previous = CURRENT.with(|c| c.replace(l));
    let outcome = catch_unwind(AssertUnwindSafe(|| cb(&mut ctx)));
    CURRENT.with(|c| c.set(previous));
    let message = match outcome {
        Ok(Ok(n)) => return n,
        Ok(Err(msg)) => msg,
        Err(panic) => {
            let detail = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic".into());
            format!("rust panic: {detail}")
        }
    };
    lua_pushlstring(l, message.as_ptr() as *const _, message.len());
    -1
}

impl Lua {
    pub fn new() -> Lua {
        // Lua allocates through Rust's global allocator, so a binary can
        // pick a faster one (the CLI uses mimalloc).
        let state = unsafe {
            let s = lua_newstate(rust_alloc, std::ptr::null_mut());
            if !s.is_null() {
                lua_atpanic(s, lua_panic);
            }
            s
        };
        assert!(!state.is_null(), "lua_newstate failed");
        unsafe { luaL_openlibs(state) };
        Lua { state, callbacks: Vec::new(), fast_data: Vec::new() }
    }

    pub fn raw(&self) -> *mut lua_State {
        self.state
    }

    fn push_callback(&mut self, f: Callback) {
        let boxed: *mut Callback = Box::into_raw(Box::new(f));
        self.callbacks.push(boxed);
        unsafe { ds_push_rust_function(self.state, dispatch, boxed as *mut c_void) };
    }

    /// Pushes a function with a fast path: `fast` gets the numeric
    /// arguments from stack index `first` (see ds_push_fast_function in
    /// shim.c) and `slow` handles every other call. `data` is what
    /// `fast` receives, and lives as long as the state.
    fn push_fast<T: 'static>(&mut self, fast: FastFn, data: Box<T>, first: i32, slow: Callback) {
        let ud = &*data as *const T as *mut c_void;
        self.fast_data.push(data);
        let boxed: *mut Callback = Box::into_raw(Box::new(slow));
        self.callbacks.push(boxed);
        unsafe { ds_push_fast_function(self.state, fast, ud, first, dispatch, boxed as *mut c_void) };
    }

    pub fn set_global_fast_fn<T: 'static>(&mut self, name: &str, fast: FastFn, data: Box<T>, first: i32, slow: Callback) {
        let n = CString::new(name).unwrap();
        self.push_fast(fast, data, first, slow);
        unsafe { lua_setfield(self.state, LUA_GLOBALSINDEX, n.as_ptr()) };
    }

    /// `table_name.field = f` (as set_table_fn) with a fast path.
    pub fn set_table_fast_fn<T: 'static>(&mut self, table_name: &str, field: &str, fast: FastFn, data: Box<T>, first: i32, slow: Callback) {
        let t = CString::new(table_name).unwrap();
        let k = CString::new(field).unwrap();
        unsafe {
            lua_getfield(self.state, LUA_GLOBALSINDEX, t.as_ptr());
            if lua_type(self.state, -1) != LUA_TTABLE {
                lua_settop(self.state, -2);
                lua_createtable(self.state, 0, 0);
                lua_pushvalue(self.state, -1);
                lua_setfield(self.state, LUA_GLOBALSINDEX, t.as_ptr());
            }
        }
        self.push_fast(fast, data, first, slow);
        unsafe {
            lua_setfield(self.state, -2, k.as_ptr());
            lua_settop(self.state, -2);
        }
    }

    pub fn set_global(&mut self, name: &str, v: &Value) {
        let n = CString::new(name).unwrap();
        unsafe {
            v.push(self.state);
            lua_setfield(self.state, LUA_GLOBALSINDEX, n.as_ptr());
        }
    }

    pub fn set_global_fn(&mut self, name: &str, f: Callback) {
        let n = CString::new(name).unwrap();
        self.push_callback(f);
        unsafe { lua_setfield(self.state, LUA_GLOBALSINDEX, n.as_ptr()) };
    }

    /// Sets `table_name.field = f` on a global table (created if missing).
    pub fn set_table_fn(&mut self, table_name: &str, field: &str, f: Callback) {
        let t = CString::new(table_name).unwrap();
        let k = CString::new(field).unwrap();
        unsafe {
            lua_getfield(self.state, LUA_GLOBALSINDEX, t.as_ptr());
            if lua_type(self.state, -1) != LUA_TTABLE {
                lua_settop(self.state, -2);
                lua_createtable(self.state, 0, 0);
                lua_pushvalue(self.state, -1);
                lua_setfield(self.state, LUA_GLOBALSINDEX, t.as_ptr());
            }
        }
        self.push_callback(f);
        unsafe {
            lua_setfield(self.state, -2, k.as_ptr());
            lua_settop(self.state, -2);
        }
    }

    /// Loads and runs a chunk; returns its results.
    pub fn exec(&mut self, source: &[u8], chunkname: &str) -> Result<Vec<Value>, String> {
        let base = unsafe { lua_gettop(self.state) };
        let mut ctx = Ctx { l: self.state };
        ctx.load_chunk(source, chunkname)?;
        unsafe {
            let status = ds_pcall_traceback(self.state, 0, LUA_MULTRET);
            if status != 0 {
                let msg = Value::from_stack(self.state, -1)?.to_display();
                lua_settop(self.state, base);
                return Err(msg);
            }
            let top = lua_gettop(self.state);
            let mut out = Vec::new();
            for i in base + 1..=top {
                out.push(Value::from_stack(self.state, i)?);
            }
            lua_settop(self.state, base);
            Ok(out)
        }
    }
}

impl Drop for Lua {
    fn drop(&mut self) {
        unsafe { lua_close(self.state) };
        for cb in self.callbacks.drain(..) {
            unsafe { drop(Box::from_raw(cb)) };
        }
    }
}

/// lua_Alloc on Rust's global allocator (16-byte aligned blocks).
unsafe extern "C" fn rust_alloc(_ud: *mut c_void, ptr: *mut c_void, osize: usize, nsize: usize) -> *mut c_void {
    use std::alloc::{alloc, dealloc, realloc, Layout};
    const ALIGN: usize = 16;
    if nsize == 0 {
        if !ptr.is_null() {
            dealloc(ptr as *mut u8, Layout::from_size_align_unchecked(osize, ALIGN));
        }
        return std::ptr::null_mut();
    }
    if ptr.is_null() {
        return alloc(Layout::from_size_align_unchecked(nsize, ALIGN)) as *mut c_void;
    }
    realloc(ptr as *mut u8, Layout::from_size_align_unchecked(osize, ALIGN), nsize) as *mut c_void
}

/// An error outside any protected call (luaL_newstate's handler prints it
/// and exits).
unsafe extern "C" fn lua_panic(l: *mut lua_State) -> c_int {
    let msg = Value::from_stack(l, -1).map(|v| v.to_display()).unwrap_or_default();
    panic!("unprotected Lua error: {msg}");
}

thread_local! {
    /// Bytecode of every chunk compiled on this thread, keyed by chunk name
    /// and source hash. Reloading dumped bytecode is several times faster
    /// than parsing, and the next seed loads the same ~1100 files.
    static CHUNK_CACHE: std::cell::RefCell<std::collections::HashMap<(Box<[u8]>, u64), Box<[u8]>>> = Default::default();
}

pub fn source_hash(s: &[u8]) -> u64 {
    // FNV-1a over 8-byte words; collisions are additionally guarded by the chunk name.
    let mut h: u64 = 0xcbf29ce484222325 ^ s.len() as u64;
    let mut chunks = s.chunks_exact(8);
    for c in &mut chunks {
        h = (h ^ u64::from_le_bytes(c.try_into().unwrap())).wrapping_mul(0x100000001b3);
    }
    for &b in chunks.remainder() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}

unsafe extern "C" fn dump_writer(_l: *mut lua_State, p: *const c_void, sz: usize, ud: *mut c_void) -> c_int {
    (*(ud as *mut Vec<u8>)).extend_from_slice(std::slice::from_raw_parts(p as *const u8, sz));
    0
}

unsafe fn load_cached(l: *mut lua_State, source: &[u8], name: &CString) -> c_int {
    if std::env::var_os("DSW_NO_CHUNK_CACHE").is_some() || source.first() == Some(&0x1b) {
        return luaL_loadbuffer(l, source.as_ptr() as *const _, source.len(), name.as_ptr());
    }
    let key = (name.as_bytes().into(), source_hash(source));
    let hit = CHUNK_CACHE.with(|c| c.borrow().get(&key).map(|b| (b.as_ptr(), b.len())));
    if let Some((p, n)) = hit {
        // Entries are never removed, so the buffer outlives the call.
        return luaL_loadbuffer(l, p as *const _, n, name.as_ptr());
    }
    let status = luaL_loadbuffer(l, source.as_ptr() as *const _, source.len(), name.as_ptr());
    if status == 0 {
        let mut out = Vec::new();
        if lua_dump(l, dump_writer, &mut out as *mut Vec<u8> as *mut c_void) == 0 {
            CHUNK_CACHE.with(|c| c.borrow_mut().insert(key, out.into_boxed_slice()));
        }
    }
    status
}
