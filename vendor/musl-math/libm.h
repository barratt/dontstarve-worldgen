/* Replacement for musl's internal "libm.h" (original project code). It
   provides what the vendored double-precision sources use, in portable C
   (the original relies on GNU extensions and long double layouts). */
#ifndef DSM_LIBM_H
#define DSM_LIBM_H

#include <float.h>
#include <math.h>
#include <stdint.h>
#include <string.h>
#include "dsm_prelude.h"

/* Support non-nearest rounding mode; no signaling NaNs. */
#define WANT_ROUNDING 1
#define WANT_SNAN 0
#define issignaling_inline(x) 0
#define TOINT_INTRINSICS 0

#if defined(__GNUC__) || defined(__clang__)
#define predict_true(x) __builtin_expect(!!(x), 1)
#define predict_false(x) __builtin_expect(x, 0)
#else
#define predict_true(x) (x)
#define predict_false(x) (x)
#endif

static inline double eval_as_double(double x) { double y = x; return y; }
static inline double fp_barrier(double x) { volatile double y = x; return y; }
static inline void fp_force_eval(double x) { volatile double y; y = x; (void)y; }
/* Only raises floating-point exceptions; never changes a result. */
#define FORCE_EVAL(x) fp_force_eval((double)(x))

static inline uint64_t asuint64(double f) { uint64_t i; memcpy(&i, &f, 8); return i; }
static inline double asdouble(uint64_t i) { double f; memcpy(&f, &i, 8); return f; }
static inline uint32_t asuint(float f) { uint32_t i; memcpy(&i, &f, 4); return i; }
static inline float asfloat(uint32_t i) { float f; memcpy(&f, &i, 4); return f; }

#define EXTRACT_WORDS(hi, lo, d) do { uint64_t __u = asuint64(d); (hi) = __u >> 32; (lo) = (uint32_t)__u; } while (0)
#define GET_HIGH_WORD(hi, d) do { (hi) = asuint64(d) >> 32; } while (0)
#define GET_LOW_WORD(lo, d) do { (lo) = (uint32_t)asuint64(d); } while (0)
#define INSERT_WORDS(d, hi, lo) do { (d) = asdouble(((uint64_t)(hi) << 32) | (uint32_t)(lo)); } while (0)
#define SET_HIGH_WORD(d, hi) INSERT_WORDS(d, hi, (uint32_t)asuint64(d))
#define SET_LOW_WORD(d, lo) INSERT_WORDS(d, asuint64(d) >> 32, lo)

int __rem_pio2_large(double *, double *, int, int, int);
int __rem_pio2(double, double *);
double __sin(double, double, int);
double __cos(double, double);
double __tan(double, double, int);
double __expo2(double, double);
double __math_xflow(uint32_t, double);
double __math_uflow(uint32_t);
double __math_oflow(uint32_t);
double __math_divzero(uint32_t);
double __math_invalid(double);

double sin(double);
double cos(double);
double tan(double);
double asin(double);
double acos(double);
double atan(double);
double atan2(double, double);
double exp(double);
double log(double);
double log10(double);
double pow(double, double);
double sinh(double);
double cosh(double);
double tanh(double);
double expm1(double);

#endif
