// simd_ops.h — SIMD-accelerated hot paths (Zig implementation)
// Auto-generated C header for FFI bindings.

#ifndef OAG_SIMD_OPS_H
#define OAG_SIMD_OPS_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// VT escape sequence scanner — returns offset of first non-printable byte
uint32_t oag_simd_scan_printable(const uint8_t* buf, uint32_t len);

// Dirty cell detection — XOR compare two grids, output dirty cell indices
uint32_t oag_simd_dirty_cells(
    const uint8_t* old_grid,
    const uint8_t* new_grid,
    uint32_t cell_count,
    uint32_t cell_size,
    uint32_t* dirty_out
);

// UTF-8 to codepoint decoder — SIMD-accelerated ASCII fast path
uint32_t oag_simd_utf8_decode(
    const uint8_t* buf,
    uint32_t len,
    uint32_t* codepoints_out
);

// Color attribute packing — fg/bg RGB + flags → packed u64
void oag_simd_pack_colors(
    const uint32_t* fg_colors,
    const uint32_t* bg_colors,
    const uint16_t* flags,
    uint32_t count,
    uint64_t* packed_out
);

#ifdef __cplusplus
}
#endif

#endif // OAG_SIMD_OPS_H
