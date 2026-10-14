/* Glue between Lua 5.1.4 and Rust.
 *
 * Rust callbacks never call lua_error themselves: lua_error longjmps, and
 * jumping over Rust frames is undefined. A callback returns a negative
 * count with the error message pushed, and the trampoline raises it here,
 * in C.
 */
#include "lua.h"
#include "lauxlib.h"

/* float->int conversion mode for the DS_PARITY block in luaconf.h. */
int ds_number2int_round = 1;

void ds_set_number2int_round(int on) { ds_number2int_round = on; }

typedef int (*ds_rust_fn)(lua_State *L, void *ud);

static int ds_trampoline(lua_State *L) {
  ds_rust_fn f = (ds_rust_fn)lua_touserdata(L, lua_upvalueindex(1));
  void *ud = lua_touserdata(L, lua_upvalueindex(2));
  int r = f(L, ud);
  if (r < 0) return lua_error(L);
  return r;
}

void ds_push_rust_function(lua_State *L, ds_rust_fn f, void *ud) {
  lua_pushlightuserdata(L, (void *)f);
  lua_pushlightuserdata(L, ud);
  lua_pushcclosure(L, ds_trampoline, 2);
}

/* Runs `nargs` arguments already on the stack through a protected call of
 * the function below them; on error leaves a traceback string on top. */
static int ds_traceback(lua_State *L) {
  lua_getfield(L, LUA_GLOBALSINDEX, "debug");
  if (!lua_istable(L, -1)) { lua_pop(L, 1); return 1; }
  lua_getfield(L, -1, "traceback");
  if (!lua_isfunction(L, -1)) { lua_pop(L, 2); return 1; }
  lua_pushvalue(L, 1);
  lua_pushinteger(L, 2);
  lua_call(L, 2, 1);
  return 1;
}

int ds_pcall_traceback(lua_State *L, int nargs, int nresults) {
  int base = lua_gettop(L) - nargs;
  int status;
  lua_pushcfunction(L, ds_traceback);
  lua_insert(L, base);
  status = lua_pcall(L, nargs, nresults, base);
  lua_remove(L, base);
  return status;
}
