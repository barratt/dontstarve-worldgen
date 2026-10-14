/* Forced into every vendored musl source by build.rs (original project
   code). The system headers come first, untouched; then musl's `hidden`
   and the dsm_ renames (names.h) apply to the musl code alone. */
#ifndef DSM_PRELUDE_H
#define DSM_PRELUDE_H
#include <float.h>
#include <math.h>
#include <stdint.h>
#include <string.h>
#ifndef hidden
#define hidden
#endif
#include "names.h"
#endif
