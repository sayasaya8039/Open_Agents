//! SIMD-accelerated hot paths for Open_Agents terminal processing.
//!
//! All functions are exported with C ABI for FFI from C and Rust.
//! Designed for x86_64 AVX2 (256-bit vectors) with SSE2 fallback.

const std = @import("std");

// ============================================================
//  Vector type aliases
// ============================================================
const V32 = @Vector(32, u8);
const V16 = @Vector(16, u8);

// ============================================================
//  1. VT Parser — scan printable ASCII (0x20..0x7E)
// ============================================================

/// Scan `buf[0..len]` for the first byte outside the printable ASCII range
/// (0x20–0x7E).  Returns its offset, or `len` when every byte is printable.
/// Processes 32 bytes per iteration using AVX2-width vectors.
pub export fn oag_simd_scan_printable(
    buf: [*]const u8,
    len: u32,
) u32 {
    const n: usize = @intCast(len);
    var i: usize = 0;

    // --- SIMD fast path (32 B / iteration) ---
    while (i + 32 <= n) : (i += 32) {
        const data: V32 = buf[i..][0..32].*;
        // Subtract 0x20 so printable range 0x20..0x7E becomes 0x00..0x5E.
        // Any byte < 0x20 wraps to >= 0xE0 after unsigned sub; > 0x7E stays > 0x5E.
        // Single compare: (data - 0x20) > 0x5E → non-printable.
        const shifted = data -% @as(V32, @splat(0x20));
        const non_printable = shifted > @as(V32, @splat(0x5E));
        const mask_int: @Vector(32, u1) = @bitCast(non_printable);
        const mask_u32: u32 = @bitCast(mask_int);
        if (mask_u32 != 0) {
            return @intCast(i + @ctz(mask_u32));
        }
    }

    // --- Scalar tail ---
    while (i < n) : (i += 1) {
        const b = buf[i];
        if (b < 0x20 or b > 0x7E) {
            return @intCast(i);
        }
    }

    return len;
}

// ============================================================
//  2. Dirty Cell Detection — XOR-based diff
// ============================================================

/// Compare `old_grid` and `new_grid` (each `cell_count * cell_size` bytes).
/// Write the indices of changed ("dirty") cells into `dirty_out` and return
/// the total dirty count.  Uses 32-byte XOR to skip identical regions fast.
pub export fn oag_simd_dirty_cells(
    old_grid: [*]const u8,
    new_grid: [*]const u8,
    cell_count: u32,
    cell_size: u32,
    dirty_out: [*]u32,
) u32 {
    const total_bytes: usize = @as(usize, cell_count) * @as(usize, cell_size);
    const cs: usize = @intCast(cell_size);
    var dirty: u32 = 0;
    var byte_pos: usize = 0;

    // --- SIMD fast path: 32-byte XOR scan ---
    while (byte_pos + 32 <= total_bytes) : (byte_pos += 32) {
        const old_v: V32 = old_grid[byte_pos..][0..32].*;
        const new_v: V32 = new_grid[byte_pos..][0..32].*;
        const xor_v = old_v ^ new_v;
        const zero: V32 = @splat(0);
        const diff = xor_v != zero;
        const diff_int: @Vector(32, u1) = @bitCast(diff);
        const diff_u32: u32 = @bitCast(diff_int);

        if (diff_u32 == 0) continue; // Entire 32-byte chunk is identical

        // Mark every cell that overlaps this chunk
        const chunk_start = byte_pos;
        const chunk_end = byte_pos + 32;
        const first_cell = chunk_start / cs;
        const last_cell_inclusive = (chunk_end - 1) / cs;
        const last_cell = @min(last_cell_inclusive, @as(usize, cell_count) - 1);

        for (first_cell..last_cell + 1) |ci| {
            // Verify this specific cell actually differs (not just a neighbor
            // in the same 32-byte window).
            const cell_off = ci * cs;
            var differs = false;
            for (cell_off..cell_off + cs) |b| {
                if (old_grid[b] != new_grid[b]) {
                    differs = true;
                    break;
                }
            }
            if (differs) {
                // Deduplicate: only add if not already recorded
                if (dirty == 0 or dirty_out[dirty - 1] != @as(u32, @intCast(ci))) {
                    dirty_out[dirty] = @intCast(ci);
                    dirty += 1;
                }
            }
        }
    }

    // --- Scalar tail ---
    while (byte_pos < total_bytes) {
        const ci = byte_pos / cs;
        const cell_off = ci * cs;
        const cell_end = @min(cell_off + cs, total_bytes);
        var differs = false;
        for (cell_off..cell_end) |b| {
            if (old_grid[b] != new_grid[b]) {
                differs = true;
                break;
            }
        }
        if (differs) {
            if (dirty == 0 or dirty_out[dirty - 1] != @as(u32, @intCast(ci))) {
                dirty_out[dirty] = @intCast(ci);
                dirty += 1;
            }
        }
        byte_pos = cell_end;
    }

    return dirty;
}

// ============================================================
//  3. UTF-8 Decoder — SIMD ASCII fast-path + scalar multibyte
// ============================================================

/// Decode UTF-8 bytes into an array of Unicode codepoints.
/// Pure-ASCII runs are bulk-expanded via SIMD (32 bytes → 32 codepoints).
/// Multi-byte sequences fall back to scalar decoding.
/// Returns the number of codepoints written.
pub export fn oag_simd_utf8_decode(
    buf: [*]const u8,
    len: u32,
    codepoints_out: [*]u32,
) u32 {
    const n: usize = @intCast(len);
    var i: usize = 0;
    var out: usize = 0;

    while (i < n) {
        // --- SIMD: try to decode 32 ASCII bytes at once ---
        if (i + 32 <= n) {
            const data: V32 = buf[i..][0..32].*;
            const hi_bit = data & @as(V32, @splat(0x80));
            const zero: V32 = @splat(0);
            const any_non_ascii = hi_bit != zero;
            const mask_int: @Vector(32, u1) = @bitCast(any_non_ascii);
            const mask_u32: u32 = @bitCast(mask_int);

            if (mask_u32 == 0) {
                // All 32 bytes are ASCII — widen to u32
                inline for (0..32) |k| {
                    codepoints_out[out + k] = @intCast(buf[i + k]);
                }
                i += 32;
                out += 32;
                continue;
            }

            // Partial ASCII: process up to the first non-ASCII byte
            const ascii_run = @ctz(mask_u32);
            for (0..ascii_run) |k| {
                codepoints_out[out + k] = @intCast(buf[i + k]);
            }
            i += ascii_run;
            out += ascii_run;
            // Fall through to scalar for the multi-byte sequence
        }

        // --- Scalar: decode one multi-byte (or single ASCII) codepoint ---
        if (i >= n) break;
        const b0 = buf[i];
        if (b0 < 0x80) {
            // 1-byte (ASCII)
            codepoints_out[out] = b0;
            i += 1;
            out += 1;
        } else if (b0 & 0xE0 == 0xC0) {
            // 2-byte
            if (i + 1 >= n) break; // truncated
            const b1 = buf[i + 1];
            codepoints_out[out] = (@as(u32, b0 & 0x1F) << 6) | @as(u32, b1 & 0x3F);
            i += 2;
            out += 1;
        } else if (b0 & 0xF0 == 0xE0) {
            // 3-byte
            if (i + 2 >= n) break;
            const b1 = buf[i + 1];
            const b2 = buf[i + 2];
            codepoints_out[out] = (@as(u32, b0 & 0x0F) << 12) |
                (@as(u32, b1 & 0x3F) << 6) |
                @as(u32, b2 & 0x3F);
            i += 3;
            out += 1;
        } else if (b0 & 0xF8 == 0xF0) {
            // 4-byte
            if (i + 3 >= n) break;
            const b1 = buf[i + 1];
            const b2 = buf[i + 2];
            const b3 = buf[i + 3];
            codepoints_out[out] = (@as(u32, b0 & 0x07) << 18) |
                (@as(u32, b1 & 0x3F) << 12) |
                (@as(u32, b2 & 0x3F) << 6) |
                @as(u32, b3 & 0x3F);
            i += 4;
            out += 1;
        } else {
            // Invalid lead byte — skip
            i += 1;
        }
    }

    return @intCast(out);
}

// ============================================================
//  4. Color Packing — fg/bg RGB + flags → u64
// ============================================================

/// Pack parallel arrays of fg RGB, bg RGB, and flags into a compact u64 per
/// cell.
///
/// Layout (MSB → LSB):
///   [63:40]  fg_rgb  (24 bits)
///   [39:16]  bg_rgb  (24 bits)
///   [15:0]   flags   (16 bits — bold, italic, underline, reverse, dim, strikethrough …)
pub export fn oag_simd_pack_colors(
    fg_colors: [*]const u32,
    bg_colors: [*]const u32,
    flags: [*]const u16,
    count: u32,
    packed_out: [*]u64,
) void {
    const n: usize = @intCast(count);
    var i: usize = 0;

    // --- SIMD path: 8 cells per iteration ---
    while (i + 8 <= n) : (i += 8) {
        inline for (0..8) |k| {
            const fg: u64 = @as(u64, fg_colors[i + k] & 0x00FFFFFF) << 40;
            const bg: u64 = @as(u64, bg_colors[i + k] & 0x00FFFFFF) << 16;
            const fl: u64 = @as(u64, flags[i + k]);
            packed_out[i + k] = fg | bg | fl;
        }
    }

    // --- Scalar tail ---
    while (i < n) : (i += 1) {
        const fg: u64 = @as(u64, fg_colors[i] & 0x00FFFFFF) << 40;
        const bg: u64 = @as(u64, bg_colors[i] & 0x00FFFFFF) << 16;
        const fl: u64 = @as(u64, flags[i]);
        packed_out[i] = fg | bg | fl;
    }
}

// ============================================================
//  5. VT Escape Sequence Classifier
// ============================================================

/// Classify each byte in a VT byte stream into categories:
///   0 = printable ASCII (0x20-0x7E)
///   1 = control (0x00-0x1F, 0x7F) excluding ESC
///   2 = ESC (0x1B)
///   3 = CSI parameter (0x30-0x3F)
///   4 = CSI final (0x40-0x7E)  — same range as printable but contextually different
///   5 = high byte (0x80-0xFF)
///
/// Note: Without full state tracking, bytes in the printable range (0x40-0x7E)
/// are classified as printable (0) and bytes in 0x30-0x3F as CSI parameter (3).
/// The caller should use ESC markers to determine CSI context.
pub export fn oag_simd_classify_vt(
    buf: [*]const u8,
    len: u32,
    classes_out: [*]u8,
) void {
    const n: usize = @intCast(len);
    var i: usize = 0;

    // --- SIMD fast path (32 B / iteration) ---
    while (i + 32 <= n) : (i += 32) {
        const data: V32 = buf[i..][0..32].*;

        // Start with all zeros (printable)
        var result: V32 = @splat(0);

        // High byte detection (>= 0x80): class 5
        const hi_mask: @Vector(32, u1) = @bitCast(data >= @as(V32, @splat(0x80)));

        // Control chars: < 0x20 or == 0x7F: class 1
        const lo_ctrl: @Vector(32, u1) = @bitCast(data < @as(V32, @splat(0x20)));
        const del_ctrl: @Vector(32, u1) = @bitCast(data == @as(V32, @splat(0x7F)));
        const ctrl_mask = lo_ctrl | del_ctrl;

        // ESC (0x1B): class 2
        const esc_mask: @Vector(32, u1) = @bitCast(data == @as(V32, @splat(0x1B)));

        // CSI parameter range (0x30-0x3F): class 3
        const csi_lo: @Vector(32, u1) = @bitCast(data >= @as(V32, @splat(0x30)));
        const csi_hi: @Vector(32, u1) = @bitCast(data <= @as(V32, @splat(0x3F)));
        const csi_param_mask = csi_lo & csi_hi;

        // Apply classes in priority order (higher priority overwrites)
        // 5: high byte
        const hi_select: V32 = @select(u8, @as(@Vector(32, bool), @bitCast(hi_mask)), @as(V32, @splat(5)), result);
        result = hi_select;

        // 3: CSI parameter (only for non-high bytes)
        const not_hi = ~hi_mask;
        const csi_final_mask = csi_param_mask & not_hi;
        result = @select(u8, @as(@Vector(32, bool), @bitCast(csi_final_mask)), @as(V32, @splat(3)), result);

        // 1: control (overwrites CSI param for control chars in 0x00-0x1F)
        const ctrl_and_not_hi = ctrl_mask & not_hi;
        result = @select(u8, @as(@Vector(32, bool), @bitCast(ctrl_and_not_hi)), @as(V32, @splat(1)), result);

        // 2: ESC (highest priority single byte)
        result = @select(u8, @as(@Vector(32, bool), @bitCast(esc_mask)), @as(V32, @splat(2)), result);

        classes_out[i..][0..32].* = result;
    }

    // --- Scalar tail ---
    while (i < n) : (i += 1) {
        const b = buf[i];
        if (b >= 0x80) {
            classes_out[i] = 5;
        } else if (b == 0x1B) {
            classes_out[i] = 2;
        } else if (b < 0x20 or b == 0x7F) {
            classes_out[i] = 1;
        } else if (b >= 0x30 and b <= 0x3F) {
            classes_out[i] = 3;
        } else {
            classes_out[i] = 0; // printable
        }
    }
}

// ============================================================
//  6. simdjson-like JSON Structural Character Detection
// ============================================================

/// Detect JSON structural characters ({, }, [, ], :, ,, ") in the buffer.
/// Returns a bitmask per 32-byte chunk where bit i indicates buf[chunk*32+i]
/// is a structural character. Returns the number of chunks processed.
pub export fn oag_simd_detect_json(
    buf: [*]const u8,
    len: u32,
    structural_bits: [*]u32,
) u32 {
    const n: usize = @intCast(len);
    var chunk: u32 = 0;
    var i: usize = 0;

    // --- SIMD fast path (32 B / chunk) ---
    while (i + 32 <= n) : (i += 32) {
        const data: V32 = buf[i..][0..32].*;

        const m1: @Vector(32, u1) = @bitCast(data == @as(V32, @splat('{')));
        const m2: @Vector(32, u1) = @bitCast(data == @as(V32, @splat('}')));
        const m3: @Vector(32, u1) = @bitCast(data == @as(V32, @splat('[')));
        const m4: @Vector(32, u1) = @bitCast(data == @as(V32, @splat(']')));
        const m5: @Vector(32, u1) = @bitCast(data == @as(V32, @splat(':')));
        const m6: @Vector(32, u1) = @bitCast(data == @as(V32, @splat(',')));
        const m7: @Vector(32, u1) = @bitCast(data == @as(V32, @splat('"')));

        const combined = m1 | m2 | m3 | m4 | m5 | m6 | m7;
        structural_bits[chunk] = @bitCast(combined);
        chunk += 1;
    }

    // --- Scalar tail (partial last chunk) ---
    if (i < n) {
        var tail_bits: u32 = 0;
        var j: u5 = 0;
        while (i < n) : (i += 1) {
            const b = buf[i];
            if (b == '{' or b == '}' or b == '[' or b == ']' or
                b == ':' or b == ',' or b == '"')
            {
                tail_bits |= @as(u32, 1) << j;
            }
            j +%= 1;
        }
        structural_bits[chunk] = tail_bits;
        chunk += 1;
    }

    return chunk;
}

// ============================================================
//  7. Markdown Line Type Detection
// ============================================================

pub const MarkdownLineType = enum(u8) {
    plain = 0,
    heading1 = 1,
    heading2 = 2,
    heading3 = 3,
    heading4 = 4,
    code_fence = 5,
    list_item = 6,
    blockquote = 7,
    empty = 8,
};

/// Detect Markdown structural line types by examining each line's leading bytes.
/// Returns the number of lines found.
pub export fn oag_simd_detect_markdown(
    buf: [*]const u8,
    len: u32,
    line_types: [*]u8,
) u32 {
    const n: usize = @intCast(len);
    var line_count: u32 = 0;
    var line_start: usize = 0;

    // Process each line
    while (line_start <= n) {
        // Find the end of this line
        var line_end: usize = line_start;
        while (line_end < n and buf[line_end] != 0x0A) : (line_end += 1) {}

        const line_len = line_end - line_start;

        // Classify based on leading bytes
        if (line_len == 0) {
            line_types[line_count] = @intFromEnum(MarkdownLineType.empty);
        } else {
            const first = buf[line_start];
            if (first == '#') {
                // Count consecutive '#'
                var level: usize = 0;
                while (level < line_len and buf[line_start + level] == '#') : (level += 1) {}
                if (level >= 4) {
                    line_types[line_count] = @intFromEnum(MarkdownLineType.heading4);
                } else if (level == 3) {
                    line_types[line_count] = @intFromEnum(MarkdownLineType.heading3);
                } else if (level == 2) {
                    line_types[line_count] = @intFromEnum(MarkdownLineType.heading2);
                } else {
                    line_types[line_count] = @intFromEnum(MarkdownLineType.heading1);
                }
            } else if (line_len >= 3 and buf[line_start] == '`' and buf[line_start + 1] == '`' and buf[line_start + 2] == '`') {
                line_types[line_count] = @intFromEnum(MarkdownLineType.code_fence);
            } else if (line_len >= 2 and first == '-' and buf[line_start + 1] == ' ') {
                line_types[line_count] = @intFromEnum(MarkdownLineType.list_item);
            } else if (line_len >= 2 and first == '>' and buf[line_start + 1] == ' ') {
                line_types[line_count] = @intFromEnum(MarkdownLineType.blockquote);
            } else {
                line_types[line_count] = @intFromEnum(MarkdownLineType.plain);
            }
        }

        line_count += 1;
        // Move past the newline
        if (line_end >= n) break;
        line_start = line_end + 1;
    }

    return line_count;
}

// ============================================================
//  8. Fast Newline Position Detection
// ============================================================

/// Find all newline (0x0A) positions in the buffer. 32 bytes per cycle.
/// Returns the number of newlines found.
pub export fn oag_simd_find_newlines(
    buf: [*]const u8,
    len: u32,
    positions_out: [*]u32,
) u32 {
    const n: usize = @intCast(len);
    var count: u32 = 0;
    var i: usize = 0;

    // --- SIMD fast path (32 B / iteration) ---
    while (i + 32 <= n) : (i += 32) {
        const data: V32 = buf[i..][0..32].*;
        const nl_mask: @Vector(32, u1) = @bitCast(data == @as(V32, @splat(0x0A)));
        var mask_u32: u32 = @bitCast(nl_mask);

        while (mask_u32 != 0) {
            const bit_pos = @ctz(mask_u32);
            positions_out[count] = @intCast(i + bit_pos);
            count += 1;
            // Clear the lowest set bit
            mask_u32 &= mask_u32 - 1;
        }
    }

    // --- Scalar tail ---
    while (i < n) : (i += 1) {
        if (buf[i] == 0x0A) {
            positions_out[count] = @intCast(i);
            count += 1;
        }
    }

    return count;
}

// ============================================================
//  9. SGR Parameter Fast Parser
// ============================================================

/// Parse SGR (Select Graphic Rendition) parameter bytes.
/// Input: parameter bytes between CSI and 'm' (e.g., "38;2;255;128;0").
/// Outputs: fg color (packed RGB), bg color (packed RGB), attribute flags.
///
/// Flags layout:
///   bit 0: bold      bit 1: dim       bit 2: italic
///   bit 3: underline bit 4: blink     bit 5: inverse
///   bit 6: hidden    bit 7: strikethrough
pub export fn oag_simd_sgr_parse(
    params: [*]const u8,
    len: u32,
    fg_out: *u32,
    bg_out: *u32,
    flags_out: *u16,
) void {
    const n: usize = @intCast(len);
    fg_out.* = 0;
    bg_out.* = 0;
    flags_out.* = 0;

    // Extract parameter numbers by scanning for ';' separators
    // We'll collect up to 16 parameters
    var param_values: [16]u32 = [_]u32{0} ** 16;
    var param_count: usize = 0;
    var current_num: u32 = 0;
    var has_digit = false;

    // Use SIMD to find semicolons for longer strings
    var i: usize = 0;
    if (n >= 32) {
        // SIMD semicolon scan: find positions, then scalar parse numbers between them
        while (i + 32 <= n) : (i += 32) {
            const data: V32 = params[i..][0..32].*;
            const semi_mask: @Vector(32, u1) = @bitCast(data == @as(V32, @splat(';')));
            const digit_lo: @Vector(32, u1) = @bitCast(data >= @as(V32, @splat('0')));
            const digit_hi: @Vector(32, u1) = @bitCast(data <= @as(V32, @splat('9')));
            const is_digit = digit_lo & digit_hi;
            _ = is_digit;

            // Process byte by byte within this chunk (SIMD detected structure)
            var semi_u32: u32 = @bitCast(semi_mask);
            // Still process digits sequentially for number accumulation
            for (0..32) |k| {
                const b = params[i + k];
                if (b >= '0' and b <= '9') {
                    current_num = current_num * 10 + (b - '0');
                    has_digit = true;
                } else if (b == ';') {
                    if (param_count < 16) {
                        param_values[param_count] = if (has_digit) current_num else 0;
                        param_count += 1;
                    }
                    current_num = 0;
                    has_digit = false;
                    semi_u32 &= semi_u32 - 1;
                }
            }
        }
    }

    // --- Scalar tail ---
    while (i < n) : (i += 1) {
        const b = params[i];
        if (b >= '0' and b <= '9') {
            current_num = current_num * 10 + (b - '0');
            has_digit = true;
        } else if (b == ';') {
            if (param_count < 16) {
                param_values[param_count] = if (has_digit) current_num else 0;
                param_count += 1;
            }
            current_num = 0;
            has_digit = false;
        }
    }
    // Last parameter (no trailing ';')
    if (has_digit and param_count < 16) {
        param_values[param_count] = current_num;
        param_count += 1;
    }

    // Interpret SGR codes
    var pi: usize = 0;
    while (pi < param_count) {
        const code = param_values[pi];
        switch (code) {
            0 => {
                // Reset
                fg_out.* = 0;
                bg_out.* = 0;
                flags_out.* = 0;
            },
            1 => flags_out.* |= (1 << 0), // bold
            2 => flags_out.* |= (1 << 1), // dim
            3 => flags_out.* |= (1 << 2), // italic
            4 => flags_out.* |= (1 << 3), // underline
            5 => flags_out.* |= (1 << 4), // blink
            7 => flags_out.* |= (1 << 5), // inverse
            8 => flags_out.* |= (1 << 6), // hidden
            9 => flags_out.* |= (1 << 7), // strikethrough
            38 => {
                // Foreground color
                if (pi + 1 < param_count and param_values[pi + 1] == 2) {
                    // 24-bit: 38;2;R;G;B
                    if (pi + 4 < param_count) {
                        const r = param_values[pi + 2] & 0xFF;
                        const g = param_values[pi + 3] & 0xFF;
                        const b_val = param_values[pi + 4] & 0xFF;
                        fg_out.* = (r << 16) | (g << 8) | b_val;
                        pi += 4;
                    }
                } else if (pi + 1 < param_count and param_values[pi + 1] == 5) {
                    // 256-color: 38;5;N — store index directly
                    if (pi + 2 < param_count) {
                        fg_out.* = param_values[pi + 2] & 0xFF;
                        pi += 2;
                    }
                }
            },
            48 => {
                // Background color
                if (pi + 1 < param_count and param_values[pi + 1] == 2) {
                    // 24-bit: 48;2;R;G;B
                    if (pi + 4 < param_count) {
                        const r = param_values[pi + 2] & 0xFF;
                        const g = param_values[pi + 3] & 0xFF;
                        const b_val = param_values[pi + 4] & 0xFF;
                        bg_out.* = (r << 16) | (g << 8) | b_val;
                        pi += 4;
                    }
                } else if (pi + 1 < param_count and param_values[pi + 1] == 5) {
                    // 256-color: 48;5;N
                    if (pi + 2 < param_count) {
                        bg_out.* = param_values[pi + 2] & 0xFF;
                        pi += 2;
                    }
                }
            },
            else => {},
        }
        pi += 1;
    }
}

// ============================================================
//  9. JSON/Markdown → immediate highlight color packing
// ============================================================

/// Highlight color palette for AI output (One Dark theme)
const HL_JSON_KEY: u32 = 0x61AFEF; // blue
const HL_JSON_STRING: u32 = 0x98C379; // green
const HL_JSON_NUMBER: u32 = 0xD19A66; // orange
const HL_JSON_BOOL: u32 = 0xD19A66; // orange
const HL_JSON_NULL: u32 = 0xC678DD; // purple
const HL_JSON_STRUCTURAL: u32 = 0xABB2BF; // light gray
const HL_MD_HEADING: u32 = 0x61AFEF; // blue
const HL_MD_CODE: u32 = 0x98C379; // green
const HL_MD_BOLD: u32 = 0xE5C07B; // yellow
const HL_MD_LINK: u32 = 0x56B6C2; // cyan
const HL_DEFAULT_FG: u32 = 0xCDD6F4; // text

/// Scan a line of text and pack highlight colors per character.
/// Uses SIMD structural detection for JSON, prefix checks for Markdown.
/// Each output u32 = RGB foreground color for that character position.
///
/// mode: 0 = auto-detect, 1 = force JSON, 2 = force Markdown
pub export fn oag_simd_highlight_line(
    buf: [*]const u8,
    len: u32,
    mode: u32,
    colors_out: [*]u32,
) void {
    const n: usize = @intCast(len);
    if (n == 0) return;

    // Auto-detect: check first non-space character
    const effective_mode: u32 = if (mode != 0) mode else blk: {
        var skip: usize = 0;
        while (skip < n and buf[skip] == ' ') : (skip += 1) {}
        if (skip >= n) break :blk 2; // empty → markdown (plain)
        const first = buf[skip];
        if (first == '{' or first == '[' or first == '"') break :blk 1; // JSON
        break :blk 2; // Markdown
    };

    if (effective_mode == 1) {
        highlightJson(buf, n, colors_out);
    } else {
        highlightMarkdown(buf, n, colors_out);
    }
}

fn highlightJson(buf: [*]const u8, n: usize, colors_out: [*]u32) void {
    // State machine: track if we're in a string, after a colon (value), etc.
    var in_string = false;
    var after_colon = false;
    var i: usize = 0;

    while (i < n) : (i += 1) {
        const c = buf[i];

        if (in_string) {
            colors_out[i] = if (after_colon) HL_JSON_STRING else HL_JSON_KEY;
            if (c == '"' and (i == 0 or buf[i - 1] != '\\')) {
                in_string = false;
                after_colon = false;
            }
        } else {
            switch (c) {
                '"' => {
                    in_string = true;
                    colors_out[i] = if (after_colon) HL_JSON_STRING else HL_JSON_KEY;
                },
                '{', '}', '[', ']', ',' => {
                    colors_out[i] = HL_JSON_STRUCTURAL;
                    after_colon = false;
                },
                ':' => {
                    colors_out[i] = HL_JSON_STRUCTURAL;
                    after_colon = true;
                },
                '0'...'9', '-', '.' => {
                    colors_out[i] = HL_JSON_NUMBER;
                },
                't', 'r', 'u', 'e', 'f', 'a', 'l', 's' => {
                    // true/false — check context
                    colors_out[i] = if (after_colon) HL_JSON_BOOL else HL_DEFAULT_FG;
                },
                'n' => {
                    colors_out[i] = if (after_colon) HL_JSON_NULL else HL_DEFAULT_FG;
                },
                else => {
                    colors_out[i] = HL_DEFAULT_FG;
                },
            }
        }
    }
}

fn highlightMarkdown(buf: [*]const u8, n: usize, colors_out: [*]u32) void {
    // Detect line type from prefix
    var skip: usize = 0;
    while (skip < n and buf[skip] == ' ') : (skip += 1) {}

    // Default: fill with default fg
    for (0..n) |i| {
        colors_out[i] = HL_DEFAULT_FG;
    }

    if (skip >= n) return; // empty line

    const first = buf[skip];

    // Heading: # ## ### ####
    if (first == '#') {
        var level: usize = 0;
        var pos = skip;
        while (pos < n and buf[pos] == '#') : (pos += 1) {
            level += 1;
        }
        if (level >= 1 and level <= 4) {
            for (0..n) |i| {
                colors_out[i] = HL_MD_HEADING;
            }
            return;
        }
    }

    // Code fence: ```
    if (skip + 2 < n and buf[skip] == '`' and buf[skip + 1] == '`' and buf[skip + 2] == '`') {
        for (0..n) |i| {
            colors_out[i] = HL_MD_CODE;
        }
        return;
    }

    // Bold: **text**
    var i: usize = 0;
    while (i + 1 < n) {
        if (buf[i] == '*' and buf[i + 1] == '*') {
            colors_out[i] = HL_MD_BOLD;
            colors_out[i + 1] = HL_MD_BOLD;
            i += 2;
            // Color until closing **
            while (i + 1 < n) {
                if (buf[i] == '*' and buf[i + 1] == '*') {
                    colors_out[i] = HL_MD_BOLD;
                    colors_out[i + 1] = HL_MD_BOLD;
                    i += 2;
                    break;
                }
                colors_out[i] = HL_MD_BOLD;
                i += 1;
            }
        } else if (buf[i] == '`') {
            // Inline code
            colors_out[i] = HL_MD_CODE;
            i += 1;
            while (i < n and buf[i] != '`') {
                colors_out[i] = HL_MD_CODE;
                i += 1;
            }
            if (i < n) {
                colors_out[i] = HL_MD_CODE;
                i += 1;
            }
        } else if (buf[i] == '[') {
            // Link: [text](url)
            colors_out[i] = HL_MD_LINK;
            i += 1;
            while (i < n and buf[i] != ']') {
                colors_out[i] = HL_MD_LINK;
                i += 1;
            }
            if (i < n) {
                colors_out[i] = HL_MD_LINK;
                i += 1;
            }
        } else {
            i += 1;
        }
    }
}

// ============================================================
//  Tests
// ============================================================

test "scan_printable — pure ASCII" {
    const data = "Hello, World! 0123456789 abcdef";
    const result = oag_simd_scan_printable(data.ptr, @intCast(data.len));
    try std.testing.expectEqual(@as(u32, @intCast(data.len)), result);
}

test "scan_printable — ESC in middle" {
    const data = "Hello\x1BWorld";
    const result = oag_simd_scan_printable(data.ptr, @intCast(data.len));
    try std.testing.expectEqual(@as(u32, 5), result);
}

test "scan_printable — control char at start" {
    const data = "\x00Hello";
    const result = oag_simd_scan_printable(data.ptr, @intCast(data.len));
    try std.testing.expectEqual(@as(u32, 0), result);
}

test "scan_printable — DEL (0x7F)" {
    const data = "ABCD\x7F";
    const result = oag_simd_scan_printable(data.ptr, @intCast(data.len));
    try std.testing.expectEqual(@as(u32, 4), result);
}

test "scan_printable — long ASCII run (> 32B)" {
    const data = "A" ** 64;
    const result = oag_simd_scan_printable(data.ptr, 64);
    try std.testing.expectEqual(@as(u32, 64), result);
}

test "dirty_cells — identical grids" {
    var grid = [_]u8{ 0xAA, 0xBB, 0xCC, 0xDD } ** 8; // 32 bytes
    var dirty_buf: [32]u32 = undefined;
    const result = oag_simd_dirty_cells(&grid, &grid, 4, 8, &dirty_buf);
    try std.testing.expectEqual(@as(u32, 0), result);
}

test "dirty_cells — single cell changed" {
    var old = [_]u8{0} ** 32;
    var new = [_]u8{0} ** 32;
    // Change bytes in cell index 2 (cell_size=8 → bytes 16..23)
    new[16] = 0xFF;
    var dirty_buf: [32]u32 = undefined;
    const result = oag_simd_dirty_cells(&old, &new, 4, 8, &dirty_buf);
    try std.testing.expectEqual(@as(u32, 1), result);
    try std.testing.expectEqual(@as(u32, 2), dirty_buf[0]);
}

test "dirty_cells — multiple cells changed" {
    var old = [_]u8{0} ** 32;
    var new = [_]u8{0} ** 32;
    new[0] = 1; // cell 0
    new[24] = 1; // cell 3
    var dirty_buf: [32]u32 = undefined;
    const result = oag_simd_dirty_cells(&old, &new, 4, 8, &dirty_buf);
    try std.testing.expectEqual(@as(u32, 2), result);
    try std.testing.expectEqual(@as(u32, 0), dirty_buf[0]);
    try std.testing.expectEqual(@as(u32, 3), dirty_buf[1]);
}

test "utf8_decode — ASCII only" {
    const data = "Hello!";
    var out: [64]u32 = undefined;
    const n = oag_simd_utf8_decode(data.ptr, @intCast(data.len), &out);
    try std.testing.expectEqual(@as(u32, 6), n);
    try std.testing.expectEqual(@as(u32, 'H'), out[0]);
    try std.testing.expectEqual(@as(u32, '!'), out[5]);
}

test "utf8_decode — Japanese text" {
    // "日本語" = E6 97 A5  E6 9C AC  E8 AA 9E
    const data = "日本語";
    var out: [64]u32 = undefined;
    const n = oag_simd_utf8_decode(data.ptr, @intCast(data.len), &out);
    try std.testing.expectEqual(@as(u32, 3), n);
    try std.testing.expectEqual(@as(u32, 0x65E5), out[0]); // 日
    try std.testing.expectEqual(@as(u32, 0x672C), out[1]); // 本
    try std.testing.expectEqual(@as(u32, 0x8A9E), out[2]); // 語
}

test "utf8_decode — mixed ASCII and multibyte" {
    const data = "Hi\xC3\xA9"; // "Hié"  (é = U+00E9, 2-byte)
    var out: [64]u32 = undefined;
    const n = oag_simd_utf8_decode(data.ptr, @intCast(data.len), &out);
    try std.testing.expectEqual(@as(u32, 3), n);
    try std.testing.expectEqual(@as(u32, 'H'), out[0]);
    try std.testing.expectEqual(@as(u32, 'i'), out[1]);
    try std.testing.expectEqual(@as(u32, 0xE9), out[2]);
}

test "utf8_decode — 4-byte emoji" {
    // 😀 = U+1F600 = F0 9F 98 80
    const data = "\xF0\x9F\x98\x80";
    var out: [64]u32 = undefined;
    const n = oag_simd_utf8_decode(data.ptr, @intCast(data.len), &out);
    try std.testing.expectEqual(@as(u32, 1), n);
    try std.testing.expectEqual(@as(u32, 0x1F600), out[0]);
}

test "pack_colors — basic" {
    const fg = [_]u32{ 0x00FF8800, 0x0000FF00 };
    const bg = [_]u32{ 0x00112233, 0x00445566 };
    const fl = [_]u16{ 0x0001, 0x0006 }; // bold; italic+underline
    var out: [2]u64 = undefined;

    oag_simd_pack_colors(&fg, &bg, &fl, 2, &out);

    // Cell 0: fg=0xFF8800 bg=0x112233 flags=0x0001
    //   (0xFF8800 << 40) | (0x112233 << 16) | 0x0001
    const expected0: u64 = (@as(u64, 0xFF8800) << 40) | (@as(u64, 0x112233) << 16) | 0x0001;
    try std.testing.expectEqual(expected0, out[0]);

    // Cell 1: fg=0x00FF00 bg=0x445566 flags=0x0006
    const expected1: u64 = (@as(u64, 0x00FF00) << 40) | (@as(u64, 0x445566) << 16) | 0x0006;
    try std.testing.expectEqual(expected1, out[1]);
}

test "pack_colors — ignores upper 8 bits of fg/bg" {
    const fg = [_]u32{0xDEADBEEF}; // upper byte 0xDE should be masked
    const bg = [_]u32{0xFF112233};
    const fl = [_]u16{0};
    var out: [1]u64 = undefined;

    oag_simd_pack_colors(&fg, &bg, &fl, 1, &out);

    const expected: u64 = (@as(u64, 0xADBEEF) << 40) | (@as(u64, 0x112233) << 16);
    try std.testing.expectEqual(expected, out[0]);
}

// ============================================================
//  Tests — classify_vt
// ============================================================

test "classify_vt — mixed input" {
    const data = "AB\x1B[31mCD\x00\xFF";
    var classes: [data.len]u8 = undefined;
    oag_simd_classify_vt(data.ptr, @intCast(data.len), &classes);

    try std.testing.expectEqual(@as(u8, 0), classes[0]); // 'A' printable
    try std.testing.expectEqual(@as(u8, 0), classes[1]); // 'B' printable
    try std.testing.expectEqual(@as(u8, 2), classes[2]); // ESC
    try std.testing.expectEqual(@as(u8, 0), classes[3]); // '[' printable (0x5B)
    try std.testing.expectEqual(@as(u8, 3), classes[4]); // '3' CSI param
    try std.testing.expectEqual(@as(u8, 3), classes[5]); // '1' CSI param
    try std.testing.expectEqual(@as(u8, 0), classes[6]); // 'm' printable (0x6D)
    try std.testing.expectEqual(@as(u8, 0), classes[7]); // 'C' printable
    try std.testing.expectEqual(@as(u8, 0), classes[8]); // 'D' printable
    try std.testing.expectEqual(@as(u8, 1), classes[9]); // NUL control
    try std.testing.expectEqual(@as(u8, 5), classes[10]); // 0xFF high byte
}

test "classify_vt — ESC sequence with CSI" {
    const data = "\x1B[0;38;2;255;0;0m";
    var classes: [data.len]u8 = undefined;
    oag_simd_classify_vt(data.ptr, @intCast(data.len), &classes);

    try std.testing.expectEqual(@as(u8, 2), classes[0]); // ESC
    try std.testing.expectEqual(@as(u8, 0), classes[1]); // '['
    try std.testing.expectEqual(@as(u8, 3), classes[2]); // '0' CSI param
    try std.testing.expectEqual(@as(u8, 3), classes[3]); // ';' CSI param
}

// ============================================================
//  Tests — detect_json
// ============================================================

test "detect_json — simple object" {
    const data = "{\"key\": \"value\"}";
    var bits: [1]u32 = undefined;
    const chunks = oag_simd_detect_json(data.ptr, @intCast(data.len), &bits);
    try std.testing.expectEqual(@as(u32, 1), chunks);

    // bit 0: '{', bit 1: '"', bit 5: '"', bit 6: ':', bit 8: '"',
    // bit 13: '"', bit 14: '}'
    try std.testing.expect(bits[0] & 1 != 0); // '{'
    try std.testing.expect(bits[0] & (1 << 1) != 0); // first '"'
    try std.testing.expect(bits[0] & (1 << (data.len - 1)) != 0); // '}'
}

test "detect_json — nested structure" {
    const data = "{\"a\":[1,2],\"b\":{}}";
    var bits: [1]u32 = undefined;
    const chunks = oag_simd_detect_json(data.ptr, @intCast(data.len), &bits);
    try std.testing.expectEqual(@as(u32, 1), chunks);

    // Verify structural chars are detected
    try std.testing.expect(bits[0] & 1 != 0); // '{'
    try std.testing.expect(bits[0] & (1 << (data.len - 1)) != 0); // '}'
}

// ============================================================
//  Tests — detect_markdown
// ============================================================

test "detect_markdown — heading levels" {
    const data = "# H1\n## H2\n### H3\n#### H4\nplain text";
    var types: [16]u8 = undefined;
    const lines = oag_simd_detect_markdown(data.ptr, @intCast(data.len), &types);
    try std.testing.expectEqual(@as(u32, 5), lines);
    try std.testing.expectEqual(@as(u8, 1), types[0]); // heading1
    try std.testing.expectEqual(@as(u8, 2), types[1]); // heading2
    try std.testing.expectEqual(@as(u8, 3), types[2]); // heading3
    try std.testing.expectEqual(@as(u8, 4), types[3]); // heading4
    try std.testing.expectEqual(@as(u8, 0), types[4]); // plain
}

test "detect_markdown — code fence, list, blockquote, empty" {
    const data = "```rust\n- item\n> quote\n\ntext";
    var types: [16]u8 = undefined;
    const lines = oag_simd_detect_markdown(data.ptr, @intCast(data.len), &types);
    try std.testing.expectEqual(@as(u32, 5), lines);
    try std.testing.expectEqual(@as(u8, 5), types[0]); // code_fence
    try std.testing.expectEqual(@as(u8, 6), types[1]); // list_item
    try std.testing.expectEqual(@as(u8, 7), types[2]); // blockquote
    try std.testing.expectEqual(@as(u8, 8), types[3]); // empty
    try std.testing.expectEqual(@as(u8, 0), types[4]); // plain
}

// ============================================================
//  Tests — find_newlines
// ============================================================

test "find_newlines — multiple lines" {
    const data = "line1\nline2\nline3\n";
    var positions: [16]u32 = undefined;
    const count = oag_simd_find_newlines(data.ptr, @intCast(data.len), &positions);
    try std.testing.expectEqual(@as(u32, 3), count);
    try std.testing.expectEqual(@as(u32, 5), positions[0]);
    try std.testing.expectEqual(@as(u32, 11), positions[1]);
    try std.testing.expectEqual(@as(u32, 17), positions[2]);
}

test "find_newlines — no newlines" {
    const data = "no newlines here";
    var positions: [16]u32 = undefined;
    const count = oag_simd_find_newlines(data.ptr, @intCast(data.len), &positions);
    try std.testing.expectEqual(@as(u32, 0), count);
}

// ============================================================
//  Tests — sgr_parse
// ============================================================

test "sgr_parse — 24bit foreground color" {
    const data = "38;2;255;128;0";
    var fg: u32 = undefined;
    var bg: u32 = undefined;
    var flags: u16 = undefined;
    oag_simd_sgr_parse(data.ptr, @intCast(data.len), &fg, &bg, &flags);

    try std.testing.expectEqual(@as(u32, (255 << 16) | (128 << 8) | 0), fg);
    try std.testing.expectEqual(@as(u32, 0), bg);
    try std.testing.expectEqual(@as(u16, 0), flags);
}

test "sgr_parse — bold + italic + 24bit bg" {
    const data = "1;3;48;2;10;20;30";
    var fg: u32 = undefined;
    var bg: u32 = undefined;
    var flags: u16 = undefined;
    oag_simd_sgr_parse(data.ptr, @intCast(data.len), &fg, &bg, &flags);

    try std.testing.expectEqual(@as(u32, 0), fg);
    try std.testing.expectEqual(@as(u32, (10 << 16) | (20 << 8) | 30), bg);
    // bold (bit 0) + italic (bit 2) = 0b0101 = 5
    try std.testing.expectEqual(@as(u16, 5), flags);
}
