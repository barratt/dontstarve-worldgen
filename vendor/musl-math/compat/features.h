/* Stand-in for <features.h> (original project code), for C libraries that
   have none (MSVC, macOS). The vendored musl sources include it only for
   musl's `hidden` macro, which dsm_prelude.h already defines. */
#ifndef DSM_COMPAT_FEATURES_H
#define DSM_COMPAT_FEATURES_H
#endif
