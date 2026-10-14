# musl maths (subset)

The double-precision transcendental functions from musl 1.2.6
(`src/math/`; MIT licence, see `COPYRIGHT`), unmodified. Both games'
worldgen results depend on these functions down to the last bit, and musl's
versions (`exp`, `log` and `pow` from ARM's optimized-routines, the fdlibm
trigonometry) reproduce the games' values on every world checked, while
platform libraries differ. Compiling them into the generator makes native
and WebAssembly builds compute identical results.

Original project files (not from musl): `libm.h` (replaces musl's internal
header with portable C), `features.h` and `names.h` (prefix every symbol
with `dsm_` so the platform's C library is never used instead).
