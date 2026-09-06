/* Just enough of Firedancer's util layer to build fd_base64.c on its own.

   fd_base64.c below is vendored unchanged from Firedancer
   (src/ballet/base64/fd_base64.c at 5c767e1d, 2023-10-27, Apache-2.0, the
   licence beside it) so that the benchmark measures
   the code Firedancer actually runs, compiled by a C compiler, rather than a
   paraphrase of it in Rust. Everything it needs from fd_ballet_base.h is
   spelled out here. */

#ifndef HEADER_fd_base64_shim_h
#define HEADER_fd_base64_shim_h

#include <stdint.h>

typedef unsigned char uchar;
typedef signed char schar;
typedef unsigned long ulong;
typedef unsigned int uint;

#define FD_UNLIKELY(c) __builtin_expect(!!(c), 0)

static inline int fd_ulong_is_aligned(ulong x, ulong a) { return !(x & (a - 1UL)); }
static inline ulong fd_ulong_bswap(ulong x) { return __builtin_bswap64(x); }
static inline uchar const *fd_type_pun_const(void const *p) { return (uchar const *)p; }

long fd_base64_decode(uchar *out, char const *in, ulong in_len);
ulong fd_base64_encode(char *encoded, void const *_data, ulong data_len);

#endif
