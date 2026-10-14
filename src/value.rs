//! Values crossing the Lua/native boundary.
//!
//! Every native WorldSim result the game produces is a scalar, a string or
//! an array, so arrays are first-class. Other tables are read in
//! `next()` order as `Map`, for diagnostics only.

use std::ffi::c_int;

use crate::lua::ffi::*;

#[derive(Clone, Debug)]
pub enum Value {
    Nil,
    Bool(bool),
    Num(f64),
    Str(Vec<u8>),
    Arr(Vec<Value>),
    Map(Vec<(Value, Value)>),
    /// A table built like the game's `lua_createtable(L, n, 0)` filled with
    /// keys 0..n-1 (GenerateBlendedMap). Never produced by from_stack.
    ZeroBased(Vec<f64>),
    /// A Lua function held in the registry (DST native callbacks such as
    /// SetResolveNoiseFunction). Traces record it as "<function>".
    Function(c_int),
}

impl PartialEq for Value {
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            // Bit-exact: parity means identical doubles, including -0.
            (Value::Num(a), Value::Num(b)) => a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()),
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Arr(a), Value::Arr(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Function(_), Value::Function(_)) => true,
            (Value::Function(_), Value::Str(s)) | (Value::Str(s), Value::Function(_)) => s.as_slice() == b"<function>",
            (Value::ZeroBased(a), Value::ZeroBased(b)) => a.iter().map(|x| x.to_bits()).eq(b.iter().map(|x| x.to_bits())),
            (Value::ZeroBased(a), Value::Map(m)) | (Value::Map(m), Value::ZeroBased(a)) => {
                a.len() == m.len()
                    && m.iter().all(|(k, val)| {
                        let idx = match k {
                            Value::Num(n) => Some(*n as usize),
                            Value::Str(s) => std::str::from_utf8(s).ok().and_then(|t| t.parse::<usize>().ok()),
                            _ => None,
                        };
                        idx.and_then(|i| a.get(i)).zip(val.as_num()).is_some_and(|(x, y)| x.to_bits() == y.to_bits())
                    })
            }
            _ => false,
        }
    }
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(s.as_bytes().to_vec())
    }

    pub fn as_num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn to_display(&self) -> String {
        match self {
            Value::Str(s) => String::from_utf8_lossy(s).into_owned(),
            other => format!("{other:?}"),
        }
    }

    /// Reads the value at `idx`. Functions and userdata cannot cross.
    pub unsafe fn from_stack(l: *mut lua_State, idx: c_int) -> Result<Value, String> {
        let idx = if idx < 0 && idx > LUA_REGISTRYINDEX { lua_gettop(l) + idx + 1 } else { idx };
        match lua_type(l, idx) {
            LUA_TNIL | LUA_TNONE => Ok(Value::Nil),
            LUA_TBOOLEAN => Ok(Value::Bool(lua_toboolean(l, idx) != 0)),
            LUA_TNUMBER => Ok(Value::Num(lua_tonumber(l, idx))),
            LUA_TSTRING => {
                let mut len = 0usize;
                let p = lua_tolstring(l, idx, &mut len);
                Ok(Value::Str(std::slice::from_raw_parts(p as *const u8, len).to_vec()))
            }
            LUA_TTABLE => {
                let n = lua_objlen(l, idx);
                let mut count = 0usize;
                let mut entries = Vec::new();
                lua_pushnil(l);
                while lua_next(l, idx) != 0 {
                    count += 1;
                    let k = Value::from_stack(l, -2)?;
                    let v = Value::from_stack(l, -1)?;
                    entries.push((k, v));
                    lua_settop(l, -2);
                }
                if count == n {
                    let mut arr = Vec::with_capacity(n);
                    for i in 1..=n {
                        lua_rawgeti(l, idx, i as c_int);
                        arr.push(Value::from_stack(l, -1)?);
                        lua_settop(l, -2);
                    }
                    Ok(Value::Arr(arr))
                } else {
                    Ok(Value::Map(entries))
                }
            }
            LUA_TFUNCTION => {
                lua_pushvalue(l, idx);
                Ok(Value::Function(luaL_ref(l, LUA_REGISTRYINDEX)))
            }
            t => {
                let name = std::ffi::CStr::from_ptr(lua_typename(l, t)).to_string_lossy().into_owned();
                Err(format!("cannot pass a {name} across the native boundary"))
            }
        }
    }

    pub unsafe fn push(&self, l: *mut lua_State) {
        match self {
            Value::Nil => lua_pushnil(l),
            Value::Bool(b) => lua_pushboolean(l, *b as c_int),
            Value::Num(n) => lua_pushnumber(l, *n),
            Value::Str(s) => lua_pushlstring(l, s.as_ptr() as *const _, s.len()),
            Value::Arr(items) => {
                lua_createtable(l, items.len() as c_int, 0);
                for (i, v) in items.iter().enumerate() {
                    v.push(l);
                    lua_rawseti(l, -2, (i + 1) as c_int);
                }
            }
            Value::Function(r) => lua_rawgeti(l, LUA_REGISTRYINDEX, *r),
            Value::ZeroBased(items) => {
                lua_createtable(l, items.len() as c_int, 0);
                for (i, x) in items.iter().enumerate() {
                    lua_pushnumber(l, i as f64);
                    lua_pushnumber(l, *x);
                    lua_rawset(l, -3);
                }
            }
            Value::Map(entries) => {
                lua_createtable(l, 0, entries.len() as c_int);
                for (k, v) in entries {
                    k.push(l);
                    v.push(l);
                    lua_rawset(l, -3);
                }
            }
        }
    }
}
