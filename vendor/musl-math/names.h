/* Original project code: every symbol of the vendored musl maths gets a
   dsm_ prefix, so it never clashes with (or is replaced by) the platform's
   C library. Included after the system headers. */
#ifndef DSM_NAMES_H
#define DSM_NAMES_H
#define sin dsm_sin
#define cos dsm_cos
#define tan dsm_tan
#define asin dsm_asin
#define acos dsm_acos
#define atan dsm_atan
#define atan2 dsm_atan2
#define exp dsm_exp
#define log dsm_log
#define log10 dsm_log10
#define pow dsm_pow
#define sinh dsm_sinh
#define cosh dsm_cosh
#define tanh dsm_tanh
#define expm1 dsm_expm1
#define __sin dsm___sin
#define __cos dsm___cos
#define __tan dsm___tan
#define __rem_pio2 dsm___rem_pio2
#define __rem_pio2_large dsm___rem_pio2_large
#define __expo2 dsm___expo2
#define __math_oflow dsm___math_oflow
#define __math_uflow dsm___math_uflow
#define __math_xflow dsm___math_xflow
#define __math_invalid dsm___math_invalid
#define __math_divzero dsm___math_divzero
#define __exp_data dsm___exp_data
#define __log_data dsm___log_data
#define __pow_log_data dsm___pow_log_data
#endif
