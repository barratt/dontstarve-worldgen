# Lua 5.1.4

The official lua-5.1.4.tar.gz sources (MIT licence, see `COPYRIGHT`),
unmodified except for two `DS_PARITY` blocks in `src/luaconf.h`, which
build.rs enables:

- float-to-int conversion switchable at run time between round-half-even
  (Don't Starve's x86 build) and truncation (Don't Starve Together's x64
  build);
- the libm, `sprintf` and `strtod` calls routed through `src/c/crt.c`, so
  they can use the MSVCR90 runtime both games link.
