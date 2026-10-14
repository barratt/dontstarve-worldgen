/* C runtime parity for the vendored Lua (original project code).
 *
 * Both games link Lua against the Visual C++ 2008 runtime (MSVCR90). The
 * DS_PARITY block in luaconf.h routes the calls whose results depend on the
 * runtime here, so every platform (Windows, Linux, macOS, WebAssembly)
 * produces the games' results:
 *
 * - floating-point printf conversions are formatted the way MSVCR90 does
 *   (src/msvc_fmt.rs);
 * - the transcendental functions are musl's (vendor/musl-math), not the
 *   platform's, whose results vary;
 * - strtod is the platform's (correctly rounded).
 *
 * This file is compiled WITHOUT the redirection macros.
 */
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

double ds_strtod(const char *s, char **end) { return strtod(s, end); }

/* vendor/musl-math, prefixed (names.h). */
double dsm_sin(double); double dsm_cos(double); double dsm_tan(double);
double dsm_asin(double); double dsm_acos(double); double dsm_atan(double);
double dsm_exp(double); double dsm_log(double); double dsm_log10(double);
double dsm_sinh(double); double dsm_cosh(double); double dsm_tanh(double);
double dsm_pow(double, double); double dsm_atan2(double, double);

double ds_sin(double x) { return dsm_sin(x); }
double ds_cos(double x) { return dsm_cos(x); }
double ds_tan(double x) { return dsm_tan(x); }
double ds_asin(double x) { return dsm_asin(x); }
double ds_acos(double x) { return dsm_acos(x); }
double ds_atan(double x) { return dsm_atan(x); }
double ds_exp(double x) { return dsm_exp(x); }
double ds_log(double x) { return dsm_log(x); }
double ds_log10(double x) { return dsm_log10(x); }
double ds_sinh(double x) { return dsm_sinh(x); }
double ds_cosh(double x) { return dsm_cosh(x); }
double ds_tanh(double x) { return dsm_tanh(x); }
double ds_pow(double x, double y) { return dsm_pow(x, y); }
double ds_atan2(double y, double x) { return dsm_atan2(y, x); }

/* MSVCR90-style double formatting, implemented in Rust (src/msvc_fmt.rs).
   Returns -1 when spec is not a single double conversion. */
int ds_fmt_double(char *out, size_t cap, const char *spec, double v);

/* Lua calls sprintf with one conversion per call ("%.14g" for tostring, one
   string.format item otherwise). Double conversions are formatted the way
   MSVCR90 does; everything else (integers, strings, pointers) prints the
   same in any C runtime. Lua's buffers are at least LUAI_MAXNUMBER2STR (32)
   bytes for numbers and MAX_ITEM (512) for string.format items. */
int ds_sprintf(char *buf, const char *fmt, ...) {
  va_list ap;
  int r;
  size_t n = strlen(fmt);
  char conv = n ? fmt[n - 1] : 0;
  va_start(ap, fmt);
  if (fmt[0] == '%' && strchr(fmt + 1, '%') == NULL &&
      (conv == 'e' || conv == 'E' || conv == 'f' || conv == 'g' || conv == 'G')) {
    double v = va_arg(ap, double);
    r = ds_fmt_double(buf, 512, fmt, v);
    if (r >= 0) {
      va_end(ap);
      return r;
    }
    /* Not a plain double conversion after all: print it normally. */
    va_end(ap);
    va_start(ap, fmt);
  }
  r = vsprintf(buf, fmt, ap);
  va_end(ap);
  return r;
}
