/* Stand-in for musl's <features.h> (original project code): the maths
   sources only need the `hidden` visibility macro. build.rs forces it into
   every file, ahead of the system headers, so names.h renames them all. */
#ifndef DSM_FEATURES_H
#define DSM_FEATURES_H
#define hidden
#include "names.h"
#endif
