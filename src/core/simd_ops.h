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

// VT escape byte classifier — categorize each byte (printable/control/ESC/CSI/high)
void oag_simd_classify_vt(
    const uint8_t* buf,
    uint32_t len,
    uint8_t* classes_out
);

// simdjson-like JSON structural character detection — bitmask per 32-byte chunk
uint32_t oag_simd_detect_json(
    const uint8_t* buf,
    uint32_t len,
    uint32_t* structural_bits
);

// Markdown line type detection — classify each line's structural type
uint32_t oag_simd_detect_markdown(
    const uint8_t* buf,
    uint32_t len,
    uint8_t* line_types
);

// Fast newline (0x0A) position detection — returns positions and count
uint32_t oag_simd_find_newlines(
    const uint8_t* buf,
    uint32_t len,
    uint32_t* positions_out
);

// SGR parameter fast parser — extract fg/bg colors and attribute flags
void oag_simd_sgr_parse(
    const uint8_t* params,
    uint32_t len,
    uint32_t* fg_out,
    uint32_t* bg_out,
    uint16_t* flags_out
);

// JSON/Markdown highlight color packing per character
// mode: 0 = auto-detect, 1 = force JSON, 2 = force Markdown
// colors_out: RGB foreground color per character position
void oag_simd_highlight_line(
    const uint8_t* buf,
    uint32_t len,
    uint32_t mode,
    uint32_t* colors_out
);

#ifdef __cplusplus
}
#endif

#endif // OAG_SIMD_OPS_H
