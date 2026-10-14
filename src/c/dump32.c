/* Precompiled Lua 5.1 chunks for the WebAssembly build.
 *
 * Adapted from Lua 5.1.4's ldump.c (Copyright (C) 1994-2008 Lua.org,
 * PUC-Rio; MIT licence, see vendor/lua-5.1.4/COPYRIGHT). Lua's own dumper
 * writes size_t fields at the compiling host's width; this one always
 * writes them as 32 bits, the width of the wasm32 build that loads them,
 * so a 64-bit `ds-worldgen pack` can produce chunks for the web module.
 * With `strip`, debug information (line numbers, local and upvalue names,
 * source names) is left out.
 */
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#define LUA_CORE

#include "lua.h"
#include "lobject.h"
#include "lstate.h"
#include "lundump.h"

typedef struct {
  lua_State *L;
  lua_Writer writer;
  void *data;
  int strip;
  int status;
} DumpState;

static void DumpBlock(const void *b, size_t size, DumpState *D) {
  if (D->status == 0) D->status = (*D->writer)(D->L, b, size, D->data);
}

static void DumpChar(int y, DumpState *D) {
  char x = (char)y;
  DumpBlock(&x, 1, D);
}

static void DumpInt(int x, DumpState *D) {
  int32_t v = (int32_t)x;
  DumpBlock(&v, 4, D);
}

static void DumpSize(size_t x, DumpState *D) {
  uint32_t v = (uint32_t)x;
  DumpBlock(&v, 4, D);
}

static void DumpNumber(lua_Number x, DumpState *D) { DumpBlock(&x, sizeof(x), D); }

static void DumpString(const TString *s, DumpState *D) {
  if (s == NULL || getstr(s) == NULL) {
    DumpSize(0, D);
  } else {
    size_t size = s->tsv.len + 1; /* include the trailing '\0' */
    DumpSize(size, D);
    DumpBlock(getstr(s), size, D);
  }
}

static void DumpFunction(const Proto *f, const TString *p, DumpState *D);

static void DumpConstants(const Proto *f, DumpState *D) {
  int i, n = f->sizek;
  DumpInt(n, D);
  for (i = 0; i < n; i++) {
    const TValue *o = &f->k[i];
    DumpChar(ttype(o), D);
    switch (ttype(o)) {
      case LUA_TNIL: break;
      case LUA_TBOOLEAN: DumpChar(bvalue(o), D); break;
      case LUA_TNUMBER: DumpNumber(nvalue(o), D); break;
      case LUA_TSTRING: DumpString(rawtsvalue(o), D); break;
      default: break;
    }
  }
  n = f->sizep;
  DumpInt(n, D);
  for (i = 0; i < n; i++) DumpFunction(f->p[i], f->source, D);
}

static void DumpDebug(const Proto *f, DumpState *D) {
  int i, n;
  n = D->strip ? 0 : f->sizelineinfo;
  DumpInt(n, D);
  for (i = 0; i < n; i++) DumpInt(f->lineinfo[i], D);
  n = D->strip ? 0 : f->sizelocvars;
  DumpInt(n, D);
  for (i = 0; i < n; i++) {
    DumpString(f->locvars[i].varname, D);
    DumpInt(f->locvars[i].startpc, D);
    DumpInt(f->locvars[i].endpc, D);
  }
  n = D->strip ? 0 : f->sizeupvalues;
  DumpInt(n, D);
  for (i = 0; i < n; i++) DumpString(f->upvalues[i], D);
}

static void DumpFunction(const Proto *f, const TString *p, DumpState *D) {
  int i;
  DumpString((f->source == p || D->strip) ? NULL : f->source, D);
  DumpInt(f->linedefined, D);
  DumpInt(f->lastlinedefined, D);
  DumpChar(f->nups, D);
  DumpChar(f->numparams, D);
  DumpChar(f->is_vararg, D);
  DumpChar(f->maxstacksize, D);
  DumpInt(f->sizecode, D);
  for (i = 0; i < f->sizecode; i++) {
    uint32_t ins = (uint32_t)f->code[i];
    DumpBlock(&ins, 4, D);
  }
  DumpConstants(f, D);
  DumpDebug(f, D);
}

/* The header luaU_header writes on a little-endian wasm32 build. */
static void DumpHeader(DumpState *D) {
  char h[LUAC_HEADERSIZE];
  memcpy(h, LUA_SIGNATURE, sizeof(LUA_SIGNATURE) - 1);
  h[4] = LUAC_VERSION;
  h[5] = LUAC_FORMAT;
  h[6] = 1;                         /* little endian */
  h[7] = 4;                         /* sizeof(int) */
  h[8] = 4;                         /* sizeof(size_t) */
  h[9] = 4;                         /* sizeof(Instruction) */
  h[10] = (char)sizeof(lua_Number); /* 8 */
  h[11] = (((lua_Number)0.5) == 0); /* integral numbers? */
  DumpBlock(h, LUAC_HEADERSIZE, D);
}

/* Dumps the Lua function on top of the stack as a wasm32 chunk. Returns 0 on
   success, 1 if the top of the stack is not a Lua function, or the
   writer's error. */
int ds_dump_wasm32(lua_State *L, lua_Writer writer, void *data, int strip) {
  DumpState D;
  TValue *o = L->top - 1;
  if (!(ttisfunction(o) && !clvalue(o)->c.isC)) return 1;
  D.L = L;
  D.writer = writer;
  D.data = data;
  D.strip = strip;
  D.status = 0;
  DumpHeader(&D);
  DumpFunction(clvalue(o)->l.p, NULL, &D);
  return D.status;
}
