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

/* Per-tile WorldSim methods are called tens of millions of times per world.
 * When every argument (from stack index `first`, at most 32) is a number,
 * the fast function handles the call without building Rust values; it
 * returns 1 (number result), 2 (boolean result), 3 (no results), 4 (two
 * numbers) or 0 to fall back to the general callback. */
typedef int (*ds_fast_fn)(void *ud, const double *args, int n, double *out);

static int ds_fast_trampoline(lua_State *L) {
  int first = (int)(size_t)lua_touserdata(L, lua_upvalueindex(3));
  int n = lua_gettop(L) - first + 1;
  if (n >= 0 && n <= 32) {
    double a[32], out[2] = {0, 0};
    int i, k;
    for (i = 0; i < n; i++) {
      if (lua_type(L, first + i) != LUA_TNUMBER) goto slow;
      a[i] = lua_tonumber(L, first + i);
    }
    k = ((ds_fast_fn)lua_touserdata(L, lua_upvalueindex(1)))(lua_touserdata(L, lua_upvalueindex(2)), a, n, out);
    if (k == 1) { lua_pushnumber(L, out[0]); return 1; }
    if (k == 2) { lua_pushboolean(L, out[0] != 0); return 1; }
    if (k == 3) return 0;
    if (k == 4) { lua_pushnumber(L, out[0]); lua_pushnumber(L, out[1]); return 2; }
  }
slow:
  {
    ds_rust_fn f = (ds_rust_fn)lua_touserdata(L, lua_upvalueindex(4));
    int r = f(L, lua_touserdata(L, lua_upvalueindex(5)));
    if (r < 0) return lua_error(L);
    return r;
  }
}

void ds_push_fast_function(lua_State *L, ds_fast_fn fast, void *fast_ud, int first, ds_rust_fn f, void *ud) {
  lua_pushlightuserdata(L, (void *)fast);
  lua_pushlightuserdata(L, fast_ud);
  lua_pushlightuserdata(L, (void *)(size_t)first);
  lua_pushlightuserdata(L, (void *)f);
  lua_pushlightuserdata(L, ud);
  lua_pushcclosure(L, ds_fast_trampoline, 5);
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

/* Hamlet helpers for lua_patch.rs, on the Lua API so they read and write
 * the script's tables exactly as its own loops do. Each returns nothing
 * (having changed nothing) when a value is not as expected, and the
 * script then runs its original loop. */
#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#pragma STDC FP_CONTRACT OFF

/* t[k] (with metamethods, as `t.k` in Lua) for the key string at stack
 * index k, pushed once by the caller so it is not re-interned per entry. */
static int ds_num_field(lua_State *L, int t, int k, double *out) {
  int ok;
  if (t < 0) t = lua_gettop(L) + t + 1;
  lua_pushvalue(L, k);
  lua_gettable(L, t);
  ok = lua_type(L, -1) == LUA_TNUMBER;
  if (ok) *out = lua_tonumber(L, -1);
  lua_pop(L, 1);
  return ok;
}

/* city_builder.lua clearground: DswClearGround(entities, REQUIRED_PREFABS,
 * pt.x, pt.z, WIDTH, HEIGHT, TILE_SCALE, radius) removes, from every list of
 * entities whose prefab is not required, the entries within radius of pt.
 * The script's back-to-front table.remove leaves the kept entries in order
 * at 1..k and nil above, which is what the compaction here writes. */
static int ds_clearground(lua_State *L) {
  double ptx, ptz, w, h, ts, radius;
  int i, n, k, j, lists = 0, nkeep = 0, cap = 0, at;
  unsigned char *keep = NULL; /* per entry, in traversal order: 1 keep, 0 remove */
  int *removing = NULL;       /* per non-reserved list: entries removed */
  int rcap = 0;
  if (lua_type(L, 1) != LUA_TTABLE || lua_type(L, 2) != LUA_TTABLE) return 0;
  for (i = 3; i <= 8; i++) if (lua_type(L, i) != LUA_TNUMBER) return 0;
  ptx = lua_tonumber(L, 3); ptz = lua_tonumber(L, 4);
  w = lua_tonumber(L, 5); h = lua_tonumber(L, 6);
  ts = lua_tonumber(L, 7); radius = lua_tonumber(L, 8);
  lua_settop(L, 8);
  lua_pushliteral(L, "x"); /* 9 */
  lua_pushliteral(L, "z"); /* 10 */
  /* Pass 0 reads every entry and records the decisions (nothing changes
   * if a value is not as expected); pass 1 applies them. The traversal of
   * entities is the same in both passes (only list contents change). */
#define DS_CG_FAIL { free(keep); free(removing); return 0; }
  lua_pushnil(L);
  while (lua_next(L, 1)) {
    int reserved = 0, list = lua_gettop(L), removed = 0;
    for (j = 1;; j++) {
      lua_rawgeti(L, 2, j);
      if (lua_isnil(L, -1)) { lua_pop(L, 1); break; }
      if (lua_equal(L, list - 1, -1)) reserved = 1;
      lua_pop(L, 1);
      if (reserved) break;
    }
    if (!reserved) {
      if (lua_type(L, list) != LUA_TTABLE) DS_CG_FAIL
      n = (int)lua_objlen(L, list);
      if (nkeep + n > cap) {
        unsigned char *p;
        cap = (nkeep + n) * 2 + 64;
        p = (unsigned char *)realloc(keep, cap);
        if (!p) DS_CG_FAIL
        keep = p;
      }
      for (i = 1; i <= n; i++) {
        double x, z, xdist, zdist;
        lua_rawgeti(L, list, i);
        if (lua_type(L, -1) != LUA_TTABLE || !ds_num_field(L, -1, 9, &x) || !ds_num_field(L, -1, 10, &z)) DS_CG_FAIL
        xdist = fabs(((x / ts) + w / 2.0) - ptx) + 0.2;
        zdist = fabs(((z / ts) + h / 2.0) - ptz) + 0.2;
        keep[nkeep] = !((xdist * xdist) + (zdist * zdist) <= radius * radius);
        removed += !keep[nkeep];
        nkeep++;
        lua_pop(L, 1);
      }
      if (lists >= rcap) {
        int *p;
        rcap = rcap * 2 + 16;
        p = (int *)realloc(removing, sizeof(int) * rcap);
        if (!p) DS_CG_FAIL
        removing = p;
      }
      removing[lists++] = removed;
    }
    lua_pop(L, 1);
  }
  at = 0;
  lists = 0;
  lua_pushnil(L);
  while (lua_next(L, 1)) {
    int reserved = 0, list = lua_gettop(L);
    for (j = 1;; j++) {
      lua_rawgeti(L, 2, j);
      if (lua_isnil(L, -1)) { lua_pop(L, 1); break; }
      if (lua_equal(L, list - 1, -1)) reserved = 1;
      lua_pop(L, 1);
      if (reserved) break;
    }
    if (!reserved) {
      n = (int)lua_objlen(L, list);
      if (removing[lists++] == 0) {
        at += n;
      } else {
        k = 0;
        for (i = 1; i <= n; i++) {
          if (keep[at++]) {
            k++;
            if (k != i) { lua_rawgeti(L, list, i); lua_rawseti(L, list, k); }
          }
        }
        for (i = k + 1; i <= n; i++) { lua_pushnil(L); lua_rawseti(L, list, i); }
      }
    }
    lua_pop(L, 1);
  }
#undef DS_CG_FAIL
  free(keep);
  free(removing);
  lua_pushboolean(L, 1);
  return 1;
}

/* border_finder.lua makeborder: DswCountNear(borders, x, z, limit) counts
 * the entries (in ipairs order) with |x-ent.x|^2 + |z-ent.z|^2 < limit. */
static int ds_count_near(lua_State *L) {
  double x, z, limit, ex, ez, ax, az;
  int i, count = 0;
  if (lua_type(L, 1) != LUA_TTABLE || lua_type(L, 2) != LUA_TNUMBER || lua_type(L, 3) != LUA_TNUMBER || lua_type(L, 4) != LUA_TNUMBER) return 0;
  x = lua_tonumber(L, 2); z = lua_tonumber(L, 3); limit = lua_tonumber(L, 4);
  lua_settop(L, 4);
  lua_pushliteral(L, "x"); /* 5 */
  lua_pushliteral(L, "z"); /* 6 */
  for (i = 1;; i++) {
    lua_rawgeti(L, 1, i);
    if (lua_isnil(L, -1)) { lua_pop(L, 1); break; }
    if (lua_type(L, -1) != LUA_TTABLE || !ds_num_field(L, -1, 5, &ex) || !ds_num_field(L, -1, 6, &ez)) return 0;
    lua_pop(L, 1);
    ax = fabs(x - ex);
    az = fabs(z - ez);
    if ((ax * ax) + (az * az) < limit) count++;
  }
  lua_pushnumber(L, count);
  return 1;
}

/* bunch_spawner.lua findEntsInRange: DswEntsInRange(bunch, x, z, limit),
 * a new list of the items (in ipairs order) with
 * |x-item.x|^2 + |z-item.z|^2 < limit, appended as table.insert does. */
static int ds_ents_in_range(lua_State *L) {
  double x, z, limit, ex, ez, ax, az;
  int i, n = 0;
  if (lua_type(L, 1) != LUA_TTABLE || lua_type(L, 2) != LUA_TNUMBER || lua_type(L, 3) != LUA_TNUMBER || lua_type(L, 4) != LUA_TNUMBER) return 0;
  x = lua_tonumber(L, 2); z = lua_tonumber(L, 3); limit = lua_tonumber(L, 4);
  lua_settop(L, 4);
  lua_pushliteral(L, "x"); /* 5 */
  lua_pushliteral(L, "z"); /* 6 */
  lua_newtable(L);         /* 7 */
  for (i = 1;; i++) {
    lua_rawgeti(L, 1, i);
    if (lua_isnil(L, -1)) { lua_pop(L, 1); break; }
    if (lua_type(L, -1) != LUA_TTABLE || !ds_num_field(L, -1, 5, &ex) || !ds_num_field(L, -1, 6, &ez)) return 0;
    ax = fabs(x - ex);
    az = fabs(z - ez);
    if ((ax * ax) + (az * az) < limit) lua_rawseti(L, 7, ++n);
    else lua_pop(L, 1);
  }
  return 1;
}

/* bunch_spawner.lua checkforblockingitems: DswAnyNear(entities, BUNCH_BLOCKERS,
 * x, z, limit) is whether any entity of a blocking prefab lies within
 * limit (squared distance, strict), then the list of blocking prefabs with
 * no entities (the script prints those, in order); nothing when a value is
 * not as expected. */
static int ds_any_near(lua_State *L) {
  double x, z, limit, ex, ez, ax, az;
  int j, i, any = 0, missing = 0;
  if (lua_type(L, 1) != LUA_TTABLE || lua_type(L, 2) != LUA_TTABLE || lua_type(L, 3) != LUA_TNUMBER || lua_type(L, 4) != LUA_TNUMBER || lua_type(L, 5) != LUA_TNUMBER) return 0;
  x = lua_tonumber(L, 3); z = lua_tonumber(L, 4); limit = lua_tonumber(L, 5);
  lua_settop(L, 5);
  lua_pushliteral(L, "x"); /* 6 */
  lua_pushliteral(L, "z"); /* 7 */
  lua_newtable(L);         /* 8: missing prefabs */
  for (j = 1;; j++) {
    lua_rawgeti(L, 2, j);
    if (lua_isnil(L, -1)) { lua_pop(L, 1); break; }
    lua_pushvalue(L, -1);
    lua_gettable(L, 1); /* entities[prefab], above prefab */
    if (!lua_toboolean(L, -1)) {
      lua_pop(L, 1);
      lua_rawseti(L, 8, ++missing);
      continue;
    }
    if (lua_type(L, -1) != LUA_TTABLE) return 0;
    lua_remove(L, -2);
    for (i = 1;; i++) {
      lua_rawgeti(L, -1, i);
      if (lua_isnil(L, -1)) { lua_pop(L, 1); break; }
      if (lua_type(L, -1) != LUA_TTABLE || !ds_num_field(L, -1, 6, &ex) || !ds_num_field(L, -1, 7, &ez)) return 0;
      lua_pop(L, 1);
      ax = fabs(x - ex);
      az = fabs(z - ez);
      if ((ax * ax) + (az * az) < limit) any = 1;
    }
    lua_pop(L, 1);
  }
  lua_pushboolean(L, any);
  if (missing) { lua_pushvalue(L, 8); return 2; }
  return 1;
}

/* dumper.lua keys(t): DswSortedKeys(t), the keys of t sorted by the
 * script's comparator (numbers before strings, each by `<`), when every
 * key is a number or a string: the order is then total, so any sort gives
 * table.sort's result. Nothing for other key types. */
static lua_State *ds_sort_L;
static int ds_key_less(int a, int b) {
  /* keys at tmp[a], tmp[b]; tmp is stack index 2 */
  int ta, tb, r;
  lua_rawgeti(ds_sort_L, 2, a);
  lua_rawgeti(ds_sort_L, 2, b);
  ta = lua_type(ds_sort_L, -2);
  tb = lua_type(ds_sort_L, -1);
  if (ta == tb) r = lua_lessthan(ds_sort_L, -2, -1);
  else r = ta == LUA_TNUMBER; /* "number" < "string" */
  lua_pop(ds_sort_L, 2);
  return r;
}

static void ds_merge_sort(int *v, int *w, int n) {
  int h, i, j, k;
  if (n < 2) return;
  h = n / 2;
  ds_merge_sort(v, w, h);
  ds_merge_sort(v + h, w, n - h);
  for (i = 0, j = h, k = 0; k < n; k++) {
    if (j >= n || (i < h && !ds_key_less(v[j], v[i]))) w[k] = v[i++];
    else w[k] = v[j++];
  }
  for (k = 0; k < n; k++) v[k] = w[k];
}

static int ds_sorted_keys(lua_State *L) {
  int n = 0, i, *v, *w;
  if (lua_type(L, 1) != LUA_TTABLE) return 0;
  lua_settop(L, 1);
  lua_newtable(L); /* 2: keys in pairs order */
  lua_pushnil(L);
  while (lua_next(L, 1)) {
    int t;
    lua_pop(L, 1);
    t = lua_type(L, -1);
    if (t != LUA_TNUMBER && t != LUA_TSTRING) return 0;
    lua_pushvalue(L, -1);
    lua_rawseti(L, 2, ++n);
  }
  v = (int *)malloc(sizeof(int) * (n + 1) * 2);
  if (!v) return 0;
  w = v + n + 1;
  for (i = 0; i < n; i++) v[i] = i + 1;
  ds_sort_L = L;
  ds_merge_sort(v, w, n);
  lua_createtable(L, n, 0); /* 3 */
  for (i = 0; i < n; i++) {
    lua_rawgeti(L, 2, v[i]);
    lua_rawseti(L, 3, i + 1);
  }
  free(v);
  return 1;
}

/* forest_map.lua pickspawnprefab: DswPickSpawnPrefab(items_in, ground_type,
 * terrain.filter) returns true and the script's result. It builds the
 * filtered table with the same sequence of table writes (so its pairs
 * order is the same) and draws with the script's math.random. Nothing,
 * before any draw, when a weight is not a number. */
static int ds_pick_spawn_prefab(lua_State *L) {
  double total = 0, rnd, v;
  if (lua_type(L, 1) != LUA_TTABLE) return 0;
  lua_settop(L, 3);
  lua_newtable(L); /* 4: items */
  if (!lua_isnil(L, 2)) {
    if (lua_type(L, 3) != LUA_TTABLE) return 0;
    lua_pushnil(L);
    while (lua_next(L, 1)) {
      lua_pop(L, 1);
      lua_pushvalue(L, -1);           /* item */
      lua_pushvalue(L, -1);
      lua_gettable(L, 1);             /* items_in[item] */
      lua_settable(L, 4);             /* items[item] = items_in[item] */
      lua_pushvalue(L, -1);
      lua_gettable(L, 3);             /* terrain.filter[item] */
      if (!lua_isnil(L, -1)) {
        int i;
        if (lua_type(L, -1) != LUA_TTABLE) return 0;
        for (i = 1;; i++) {
          lua_rawgeti(L, -1, i);
          if (lua_isnil(L, -1)) { lua_pop(L, 1); break; }
          if (lua_equal(L, -1, 2)) {
            lua_pushvalue(L, -3);     /* item */
            lua_pushnil(L);
            lua_settable(L, 4);       /* items[item] = nil */
          }
          lua_pop(L, 1);
        }
      }
      lua_pop(L, 1);
    }
  }
  lua_pushnil(L);
  while (lua_next(L, 4)) {
    if (!lua_isnumber(L, -1)) return 0;
    total = total + lua_tonumber(L, -1);
    lua_pop(L, 1);
  }
  lua_pushboolean(L, 1);
  if (!(total > 0)) return 1;
  lua_getglobal(L, "math");
  if (lua_type(L, -1) != LUA_TTABLE) return 0;
  lua_getfield(L, -1, "random");
  lua_call(L, 0, 1);
  rnd = lua_tonumber(L, -1) * total;
  lua_pop(L, 2);
  lua_pushnil(L);
  while (lua_next(L, 4)) {
    v = lua_tonumber(L, -1);
    rnd = rnd - v;
    if (rnd <= 0) {
      lua_pop(L, 1);
      return 2; /* true, key */
    }
    lua_pop(L, 1);
  }
  return 1;
}

/* bunch_spawner.lua round(): x*10 rounded half away from zero, /10. */
static double ds_round10(double x) {
  double num;
  x = x * 10;
  num = x >= 0 ? floor(x + 0.5) : ceil(x - 0.5);
  return num / 10;
}

/* bunch_spawner.lua placeitemoffgrids' test: DswOffgrid(ents, x, z) is
 * false when an entity shares x's or z's rounded coordinate or lies on a
 * rounded diagonal, else true; nothing when a value is not as expected. */
static int ds_offgrid(lua_State *L) {
  double x, z, ex, ez;
  int i;
  if (lua_type(L, 1) != LUA_TTABLE || lua_type(L, 2) != LUA_TNUMBER || lua_type(L, 3) != LUA_TNUMBER) return 0;
  x = lua_tonumber(L, 2); z = lua_tonumber(L, 3);
  lua_settop(L, 3);
  lua_pushliteral(L, "x"); /* 4 */
  lua_pushliteral(L, "z"); /* 5 */
  for (i = 1;; i++) {
    lua_rawgeti(L, 1, i);
    if (lua_isnil(L, -1)) break;
    if (lua_type(L, -1) != LUA_TTABLE || !ds_num_field(L, -1, 4, &ex) || !ds_num_field(L, -1, 5, &ez)) return 0;
    lua_pop(L, 1);
    if (ds_round10(x) == ds_round10(ex) || ds_round10(z) == ds_round10(ez) || fabs(ds_round10(ex - x)) == fabs(ds_round10(ez - z))) {
      lua_pushboolean(L, 0);
      return 1;
    }
  }
  lua_pushboolean(L, 1);
  return 1;
}

/* city_builder.lua FindTempEnts: DswFindTempEnts(data, x, z, range, prefabs),
 * the entities (ipairs order) of one of the prefabs (any, without a list)
 * within range; nothing when a value is not as expected. */
static int ds_find_temp_ents(lua_State *L) {
  double x, z, range, ex, ez, ax, az;
  int i, j, n = 0, any = 0;
  if (lua_type(L, 1) != LUA_TTABLE || lua_type(L, 2) != LUA_TNUMBER || lua_type(L, 3) != LUA_TNUMBER || lua_type(L, 4) != LUA_TNUMBER) return 0;
  x = lua_tonumber(L, 2); z = lua_tonumber(L, 3); range = lua_tonumber(L, 4);
  lua_settop(L, 5);
  if (!lua_toboolean(L, 5)) any = 1;
  else if (lua_type(L, 5) != LUA_TTABLE) return 0;
  lua_pushliteral(L, "x");      /* 6 */
  lua_pushliteral(L, "z");      /* 7 */
  lua_pushliteral(L, "prefab"); /* 8 */
  lua_newtable(L);              /* 9 */
  for (i = 1;; i++) {
    int test = any;
    lua_rawgeti(L, 1, i);
    if (lua_isnil(L, -1)) break;
    if (lua_type(L, -1) != LUA_TTABLE) return 0;
    if (!any) {
      lua_pushvalue(L, 8);
      lua_gettable(L, -2); /* entity.prefab */
      for (j = 1;; j++) {
        lua_rawgeti(L, 5, j);
        if (lua_isnil(L, -1)) { lua_pop(L, 1); break; }
        if (lua_equal(L, -2, -1)) test = 1;
        lua_pop(L, 1);
      }
      lua_pop(L, 1);
    }
    if (test) {
      if (!ds_num_field(L, -1, 6, &ex) || !ds_num_field(L, -1, 7, &ez)) return 0;
      ax = fabs(x - ex);
      az = fabs(z - ez);
      if ((ax * ax) + (az * az) <= range * range) { lua_rawseti(L, 9, ++n); continue; }
    }
    lua_pop(L, 1);
  }
  lua_pushvalue(L, 9);
  return 1;
}

/* dumper.lua DataDumper, the slow path (fastmode false) that worldgen
 * uses for the savedata: DswDataDumper(value, varname, ident) builds the
 * same text for trees of plain tables, strings, numbers and booleans. It
 * returns nothing, leaving the script to do it, for anything the slow path
 * treats specially (metatables, functions, userdata, a table reached
 * twice, keys other than numbers and strings). */
typedef struct { char *p; size_t n, cap; int ok; } ds_sbuf;

static void ds_sb_add(ds_sbuf *b, const char *s, size_t n) {
  if (!b->ok) return;
  if (b->n + n + 1 > b->cap) {
    size_t cap = b->cap ? b->cap : 64;
    char *p;
    while (b->n + n + 1 > cap) cap *= 2;
    p = (char *)realloc(b->p, cap);
    if (!p) { b->ok = 0; return; }
    b->p = p; b->cap = cap;
  }
  memcpy(b->p + b->n, s, n);
  b->n += n;
}
static void ds_sb_str(ds_sbuf *b, const char *s) { ds_sb_add(b, s, strlen(s)); }

/* string.format('%q', s) of Lua 5.1.4 (lstrlib.c addquoted). */
static void ds_sb_quoted(ds_sbuf *b, const char *s, size_t l) {
  ds_sb_add(b, "\"", 1);
  while (l--) {
    switch (*s) {
      case '"': case '\\': case '\n': { char e[2]; e[0] = '\\'; e[1] = *s; ds_sb_add(b, e, 2); break; }
      case '\r': ds_sb_add(b, "\\r", 2); break;
      case '\0': ds_sb_add(b, "\\000", 4); break;
      default: ds_sb_add(b, s, 1); break;
    }
    s++;
  }
  ds_sb_add(b, "\"", 1);
}

/* The value at idx as `..` would make it a string (numbers: lua_tolstring,
 * the same conversion). */
static void ds_sb_number(lua_State *L, ds_sbuf *b, int idx) {
  size_t l;
  const char *s;
  lua_pushvalue(L, idx);
  s = lua_tolstring(L, -1, &l);
  ds_sb_add(b, s, l);
  lua_pop(L, 1);
}

static int ds_is_ident(const char *s, size_t l) {
  size_t i;
  if (l == 0 || !(isalpha((unsigned char)s[0]) || s[0] == '_')) return 0;
  for (i = 1; i < l; i++)
    if (!(isalnum((unsigned char)s[i]) || s[i] == '_')) return 0;
  return 1;
}

static const char *const ds_lua_keywords[] = {
  "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in",
  "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while", NULL};

/* stack: 1 value, 2 varname, 3 ident, 4 seen-tables set */
static int ds_dump(lua_State *L, int idx, int ident, ds_sbuf *out);

static int ds_dump_table(lua_State *L, int idx, int ident, ds_sbuf *out) {
  int n, i, keys;
  double numidx = 1;
  size_t totallen = 0, count = 0;
  ds_sbuf *items;
  char sep[520];
  size_t seplen;
  if (lua_getmetatable(L, idx)) return 0;
  lua_pushvalue(L, idx);
  lua_rawget(L, 4);
  if (lua_toboolean(L, -1)) return 0; /* reached twice */
  lua_pop(L, 1);
  lua_pushvalue(L, idx);
  lua_pushboolean(L, 1);
  lua_rawset(L, 4);
  /* keys(value) */
  lua_pushcfunction(L, ds_sorted_keys);
  lua_pushvalue(L, idx);
  lua_call(L, 1, 1);
  if (lua_isnil(L, -1)) return 0;
  keys = lua_gettop(L);
  n = (int)lua_objlen(L, keys);
  items = (ds_sbuf *)calloc(n ? n : 1, sizeof(ds_sbuf));
  if (!items) return 0;
  for (i = 1; i <= n; i++) {
    ds_sbuf *s = &items[i - 1];
    int key, val, ok;
    s->ok = 1;
    lua_rawgeti(L, keys, i);
    key = lua_gettop(L);
    lua_pushvalue(L, key);
    lua_gettable(L, idx);
    val = lua_gettop(L);
    if (lua_type(L, key) == LUA_TNUMBER && lua_tonumber(L, key) == numidx) {
      numidx = numidx + 1;
    } else if (lua_type(L, key) == LUA_TSTRING) {
      size_t kl;
      const char *k = lua_tolstring(L, key, &kl);
      const char *const *kw;
      int reserved = 0;
      for (kw = ds_lua_keywords; *kw; kw++)
        if (strlen(*kw) == kl && memcmp(*kw, k, kl) == 0) reserved = 1;
      if (reserved) { ds_sb_str(s, "[\""); ds_sb_add(s, k, kl); ds_sb_str(s, "\"] = "); }
      else if (ds_is_ident(k, kl)) { ds_sb_add(s, k, kl); ds_sb_str(s, "="); }
      else { ds_sb_str(s, "["); ds_sb_quoted(s, k, kl); ds_sb_str(s, "]="); }
    } else if (lua_type(L, key) == LUA_TNUMBER) {
      ds_sb_str(s, "["); ds_sb_number(L, s, key); ds_sb_str(s, "]=");
    } else {
      ok = 0;
      goto fail;
    }
    ok = ds_dump(L, val, ident + 1, s);
  fail:
    lua_settop(L, keys);
    if (!ok || !s->ok) {
      int j;
      for (j = 0; j < n; j++) free(items[j].p);
      free(items);
      return 0;
    }
    totallen = totallen + s->n + 2;
    count++;
  }
  lua_settop(L, keys - 1);
  sep[0] = ' ';
  seplen = 1;
  if (totallen > 80) {
    int j;
    sep[0] = '\n';
    seplen = 1;
    for (j = 0; j < ident + 1 && seplen + 2 < sizeof(sep); j++) { sep[seplen++] = ' '; sep[seplen++] = ' '; }
    if (ident + 1 > 255) { for (i = 0; i < n; i++) free(items[i].p); free(items); return 0; }
  }
  ds_sb_str(out, "{");
  ds_sb_add(out, sep, seplen);
  for (i = 0; i < n; i++) {
    if (i > 0) { ds_sb_str(out, ","); ds_sb_add(out, sep, seplen); }
    ds_sb_add(out, items[i].p, items[i].n);
    free(items[i].p);
  }
  free(items);
  ds_sb_str(out, " ");
  if (seplen > 2) ds_sb_add(out, sep, seplen - 2); /* sep:sub(1, -3) */
  ds_sb_str(out, "}");
  (void)count;
  return out->ok;
}

static int ds_dump(lua_State *L, int idx, int ident, ds_sbuf *out) {
  if (!lua_checkstack(L, 8)) return 0;
  switch (lua_type(L, idx)) {
    case LUA_TSTRING: { size_t l; const char *s = lua_tolstring(L, idx, &l); ds_sb_quoted(out, s, l); return out->ok; }
    case LUA_TNUMBER: ds_sb_number(L, out, idx); return out->ok;
    case LUA_TBOOLEAN: ds_sb_str(out, lua_toboolean(L, idx) ? "true" : "false"); return out->ok;
    case LUA_TNIL: ds_sb_str(out, "nil"); return out->ok;
    case LUA_TTABLE: return ds_dump_table(L, idx, ident, out);
    default: return 0;
  }
}

static int ds_data_dumper(lua_State *L) {
  ds_sbuf out = {NULL, 0, 0, 1};
  int ident = 0, ok;
  lua_settop(L, 3);
  if (!lua_isnil(L, 3)) {
    if (lua_type(L, 3) != LUA_TNUMBER) return 0;
    ident = (int)lua_tointeger(L, 3);
  }
  if (lua_isnil(L, 2)) ds_sb_str(&out, "return ");
  else if (lua_type(L, 2) == LUA_TSTRING) {
    size_t l;
    const char *v = lua_tolstring(L, 2, &l);
    ds_sb_add(&out, v, l);
    if (ds_is_ident(v, l)) ds_sb_str(&out, " = ");
  } else return 0;
  lua_newtable(L); /* 4: seen tables */
  ok = ds_dump(L, 1, ident, &out);
  if (!ok || !out.ok) { free(out.p); return 0; }
  lua_pushlstring(L, out.p, out.n);
  free(out.p);
  return 1;
}

/* dumper.lua DataDumper's fast path into the save buffer (DST worldgen):
 * DswDataDumperFast(value, varname) is the text the buffer ends up with,
 * walking tables in pairs order (lua_next, as pairs does). Nothing for
 * values the fast path does not write plainly (functions, userdata,
 * keys other than strings, numbers and booleans, nesting deeper than 200). */
static int ds_dump_fast(lua_State *L, int idx, ds_sbuf *out, int depth);

static int ds_dump_fast_table(lua_State *L, int idx, ds_sbuf *out, int depth) {
  double numidx = 1;
  if (depth > 200 || !lua_checkstack(L, 8)) return 0;
  ds_sb_str(out, "{");
  lua_pushnil(L);
  while (lua_next(L, idx)) {
    int key = lua_gettop(L) - 1, val = lua_gettop(L);
    switch (lua_type(L, key)) {
      case LUA_TNUMBER:
        if (lua_tonumber(L, key) == numidx) numidx = numidx + 1;
        else { ds_sb_str(out, "["); ds_sb_number(L, out, key); ds_sb_str(out, "]="); }
        break;
      case LUA_TSTRING: {
        size_t kl;
        const char *k = lua_tolstring(L, key, &kl);
        const char *const *kw;
        int reserved = 0;
        for (kw = ds_lua_keywords; *kw; kw++)
          if (strlen(*kw) == kl && memcmp(*kw, k, kl) == 0) reserved = 1;
        if (reserved) { ds_sb_str(out, "[\""); ds_sb_add(out, k, kl); ds_sb_str(out, "\"] = "); }
        else if (ds_is_ident(k, kl)) { ds_sb_add(out, k, kl); ds_sb_str(out, "="); }
        else { ds_sb_str(out, "["); ds_sb_quoted(out, k, kl); ds_sb_str(out, "]="); }
        break;
      }
      case LUA_TBOOLEAN:
        ds_sb_str(out, lua_toboolean(L, key) ? "[true]=" : "[false]=");
        break;
      default:
        return 0;
    }
    if (!ds_dump_fast(L, val, out, depth + 1)) return 0;
    ds_sb_str(out, ",");
    lua_pop(L, 1);
  }
  /* RemoveLastCommaSaveString */
  if (out->ok && out->n > 0 && out->p[out->n - 1] == ',') out->n--;
  ds_sb_str(out, "}");
  return out->ok;
}

static int ds_dump_fast(lua_State *L, int idx, ds_sbuf *out, int depth) {
  switch (lua_type(L, idx)) {
    case LUA_TSTRING: { size_t l; const char *s = lua_tolstring(L, idx, &l); ds_sb_quoted(out, s, l); return out->ok; }
    case LUA_TNUMBER: ds_sb_number(L, out, idx); return out->ok;
    case LUA_TBOOLEAN: ds_sb_str(out, lua_toboolean(L, idx) ? "true" : "false"); return out->ok;
    case LUA_TNIL: ds_sb_str(out, "nil"); return out->ok;
    case LUA_TTABLE: return ds_dump_fast_table(L, idx, out, depth);
    default: return 0;
  }
}

static int ds_data_dumper_fast(lua_State *L) {
  ds_sbuf out = {NULL, 0, 0, 1};
  lua_settop(L, 2);
  if (lua_isnil(L, 2)) ds_sb_str(&out, "return ");
  else if (lua_type(L, 2) == LUA_TSTRING) {
    size_t l;
    const char *v = lua_tolstring(L, 2, &l);
    ds_sb_add(&out, v, l);
    if (ds_is_ident(v, l)) ds_sb_str(&out, " = ");
  } else return 0;
  if (!ds_dump_fast(L, 1, &out, 0) || !out.ok) { free(out.p); return 0; }
  lua_pushlstring(L, out.p, out.n);
  free(out.p);
  return 1;
}

void ds_register_helpers(lua_State *L) {
  lua_register(L, "DswDataDumperFast", ds_data_dumper_fast);
  lua_register(L, "DswDataDumper", ds_data_dumper);
  lua_register(L, "DswOffgrid", ds_offgrid);
  lua_register(L, "DswFindTempEnts", ds_find_temp_ents);
  lua_register(L, "DswPickSpawnPrefab", ds_pick_spawn_prefab);
  lua_register(L, "DswSortedKeys", ds_sorted_keys);
  lua_register(L, "DswClearGround", ds_clearground);
  lua_register(L, "DswCountNear", ds_count_near);
  lua_register(L, "DswEntsInRange", ds_ents_in_range);
  lua_register(L, "DswAnyNear", ds_any_near);
}
