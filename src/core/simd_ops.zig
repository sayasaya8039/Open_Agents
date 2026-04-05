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
