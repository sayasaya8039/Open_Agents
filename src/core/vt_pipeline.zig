// vt_pipeline.zig — Async VT parser with lock-free frame snapshots
// 3-Phase pipeline: PTY Read → VT Parse → Main Thread snapshot
//
// Phase 1: ReadFile(pipe) into 1MB ring buffer (lock-free SPSC)
// Phase 2: VT state machine parses ring → double-buffered grid
// Phase 3: Main thread reads active grid snapshot (no copy, no lock)

const std = @import("std");

// ============================================================
// Constants
// ============================================================

pub const GRID_MAX_COLS: u32 = 512;
pub const GRID_MAX_ROWS: u32 = 256;
pub const RAW_BUF_SIZE: u32 = 1024 * 1024; // 1MB, must be power of 2

const CSI_MAX_PARAMS: usize = 16;
const READ_CHUNK_SIZE: usize = 65536; // 64KB per ReadFile call

// ============================================================
// Windows kernel32 imports
// ============================================================

extern "kernel32" fn ReadFile(
    hFile: ?*anyopaque,
    lpBuffer: [*]u8,
    nNumberOfBytesToRead: u32,
    lpNumberOfBytesRead: *u32,
    lpOverlapped: ?*anyopaque,
) callconv(.c) i32;

extern "kernel32" fn Sleep(dwMilliseconds: u32) callconv(.c) void;

// ============================================================
// CellData — per-cell terminal state (C-compatible)
// ============================================================

pub const CellData = extern struct {
    codepoint: u32 = 0, // Unicode codepoint (0 = empty)
    fg_rgb: u32 = 0x00C0C0C0, // foreground RGB (default light gray)
    bg_rgb: u32 = 0x00000000, // background RGB (default black)
    flags: u16 = 0, // bold(0), italic(1), underline(2), reverse(3), dim(4), strikethrough(5)
    dirty: u16 = 0, // dirty generation counter
};

// ============================================================
// VT Parser State Machine
// ============================================================

const VtState = enum(u8) {
    ground, // Normal text
    escape, // ESC received
    csi_entry, // CSI (ESC [) parameter collection
    csi_param, // CSI parameter parsing
    osc_string, // OSC string
    dcs_entry, // DCS entry
};

// ============================================================
// VtPipeline — main pipeline structure
// ============================================================

pub const VtPipeline = struct {
    // ---- Raw I/O ring buffer (Phase 1 → Phase 2) ----
    raw_ring: [RAW_BUF_SIZE]u8,
    raw_write: std.atomic.Value(u32),
    raw_read: std.atomic.Value(u32),

    // ---- Grid double buffer (Phase 2 → Phase 3) ----
    grid: [2][GRID_MAX_ROWS][GRID_MAX_COLS]CellData,
    active_grid: std.atomic.Value(u8), // 0 or 1
    grid_generation: std.atomic.Value(u64),

    // ---- Damage tracking ----
    dirty_rows: [2][GRID_MAX_ROWS]bool,
    dirty_cell_count: [2]std.atomic.Value(u32),

    // ---- VT parser state ----
    state: VtState,
    cursor_row: u32,
    cursor_col: u32,
    cols: u32,
    rows: u32,

    // ---- SGR attributes (current) ----
    current_fg: u32,
    current_bg: u32,
    current_flags: u16,

    // ---- CSI parameter accumulation ----
    csi_params: [CSI_MAX_PARAMS]u32,
    csi_param_count: u32,
    csi_current_param: u32,

    // ---- Threads ----
    read_thread: ?std.Thread,
    parse_thread: ?std.Thread,
    running: std.atomic.Value(bool),

    // ---- I/O handle ----
    pipe_handle: ?*anyopaque, // HANDLE from CreateProcess

    // ---- Callback ----
    on_frame_ready: ?*const fn (?*anyopaque) callconv(.c) void,
    callback_data: ?*anyopaque,

    // ---- Stats ----
    bytes_read: u64,
    frames_produced: u64,
    parse_time_us: u64,

    // ---- Scroll region ----
    scroll_top: u32,
    scroll_bottom: u32,

    // ============================================================
    // Initialization
    // ============================================================

    pub fn init(cols: u32, rows: u32) VtPipeline {
        const c = if (cols > 0 and cols <= GRID_MAX_COLS) cols else 80;
        const r = if (rows > 0 and rows <= GRID_MAX_ROWS) rows else 24;

        var self: VtPipeline = undefined;

        // Zero raw ring
        @memset(&self.raw_ring, 0);
        self.raw_write = std.atomic.Value(u32).init(0);
        self.raw_read = std.atomic.Value(u32).init(0);

        // Zero grids
        for (0..2) |g| {
            for (0..GRID_MAX_ROWS) |row| {
                for (0..GRID_MAX_COLS) |col| {
                    self.grid[g][row][col] = CellData{};
                }
                self.dirty_rows[g][row] = false;
            }
            self.dirty_cell_count[g] = std.atomic.Value(u32).init(0);
        }

        self.active_grid = std.atomic.Value(u8).init(0);
        self.grid_generation = std.atomic.Value(u64).init(0);

        // Parser state
        self.state = .ground;
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.cols = c;
        self.rows = r;

        // SGR defaults
        self.current_fg = 0x00C0C0C0;
        self.current_bg = 0x00000000;
        self.current_flags = 0;

        // CSI params
        @memset(&self.csi_params, 0);
        self.csi_param_count = 0;
        self.csi_current_param = 0;

        // Threads
        self.read_thread = null;
        self.parse_thread = null;
        self.running = std.atomic.Value(bool).init(false);

        // I/O
        self.pipe_handle = null;

        // Callback
        self.on_frame_ready = null;
        self.callback_data = null;

        // Stats
        self.bytes_read = 0;
        self.frames_produced = 0;
        self.parse_time_us = 0;

        // Scroll region
        self.scroll_top = 0;
        self.scroll_bottom = r - 1;

        return self;
    }

    // ============================================================
    // Thread management
    // ============================================================

    pub fn start(self: *VtPipeline) void {
        if (self.running.load(.acquire)) return;
        self.running.store(true, .release);

        self.read_thread = std.Thread.spawn(.{}, readThreadFn, .{self}) catch null;
        self.parse_thread = std.Thread.spawn(.{}, parseThreadFn, .{self}) catch null;
    }

    pub fn stop(self: *VtPipeline) void {
        self.running.store(false, .release);

        if (self.read_thread) |t| {
            t.join();
            self.read_thread = null;
        }
        if (self.parse_thread) |t| {
            t.join();
            self.parse_thread = null;
        }
    }

    // ============================================================
    // Feed data directly (testing / non-PTY input)
    // ============================================================

    pub fn feed(self: *VtPipeline, data: [*]const u8, len: u32) void {
        const wp = self.raw_write.load(.acquire);
        var i: u32 = 0;
        while (i < len) : (i += 1) {
            const pos = (wp +% i) & (RAW_BUF_SIZE - 1);
            self.raw_ring[pos] = data[i];
        }
        self.raw_write.store((wp +% len) & (RAW_BUF_SIZE - 1), .release);
        self.bytes_read += len;
    }

    // ============================================================
    // Snapshot access (main thread — read-only)
    // ============================================================

    pub fn snapshotGrid(self: *const VtPipeline) [*]const CellData {
        const ag = self.active_grid.load(.acquire);
        return @ptrCast(&self.grid[ag][0][0]);
    }

    pub fn snapshotGeneration(self: *const VtPipeline) u64 {
        return self.grid_generation.load(.acquire);
    }

    pub fn getDirtyRows(self: *const VtPipeline) [*]const bool {
        const ag = self.active_grid.load(.acquire);
        return &self.dirty_rows[ag];
    }

    pub fn getDirtyCellCount(self: *const VtPipeline) u32 {
        const ag = self.active_grid.load(.acquire);
        return self.dirty_cell_count[ag].load(.acquire);
    }

    // ============================================================
    // Phase 1: PTY Read Thread
    // ============================================================

    fn readThreadFn(self: *VtPipeline) void {
        var buf: [READ_CHUNK_SIZE]u8 = undefined;

        while (self.running.load(.acquire)) {
            const handle = self.pipe_handle orelse {
                Sleep(1);
                continue;
            };

            var bytes_read_count: u32 = 0;
            const ok = ReadFile(handle, &buf, @intCast(buf.len), &bytes_read_count, null);
            if (ok == 0 or bytes_read_count == 0) {
                Sleep(1);
                continue;
            }

            // Push to raw ring buffer (SPSC: single producer)
            const wp = self.raw_write.load(.acquire);
            var i: u32 = 0;
            while (i < bytes_read_count) : (i += 1) {
                const pos = (wp +% i) & (RAW_BUF_SIZE - 1);
                self.raw_ring[pos] = buf[i];
            }
            self.raw_write.store((wp +% bytes_read_count) & (RAW_BUF_SIZE - 1), .release);
            self.bytes_read += bytes_read_count;
        }
    }

    // ============================================================
    // Phase 2: VT Parse Thread
    // ============================================================

    fn parseThreadFn(self: *VtPipeline) void {
        while (self.running.load(.acquire)) {
            const rp = self.raw_read.load(.acquire);
            const wp = self.raw_write.load(.acquire);

            if (rp == wp) {
                Sleep(1);
                continue;
            }

            const avail: u32 = if (wp >= rp) wp - rp else RAW_BUF_SIZE - rp + wp;
            const write_grid: u8 = 1 - self.active_grid.load(.acquire); // write to inactive

            // Clear dirty tracking for write grid
            for (0..self.rows) |row| {
                self.dirty_rows[write_grid][row] = false;
            }
            self.dirty_cell_count[write_grid].store(0, .release);

            // Copy current grid state from active to write grid (so we build on latest)
            const active = self.active_grid.load(.acquire);
            for (0..self.rows) |row| {
                for (0..self.cols) |col| {
                    self.grid[write_grid][row][col] = self.grid[active][row][col];
                }
            }

            var pos: u32 = 0;
            while (pos < avail) {
                const ring_pos = (rp +% pos) & (RAW_BUF_SIZE - 1);
                const byte = self.raw_ring[ring_pos];
                self.processByte(write_grid, byte);
                pos += 1;
            }

            self.raw_read.store((rp +% avail) & (RAW_BUF_SIZE - 1), .release);

            // Swap grid (atomic)
            self.active_grid.store(write_grid, .release);
            _ = self.grid_generation.fetchAdd(1, .release);
            self.frames_produced += 1;

            // Notify main thread
            if (self.on_frame_ready) |cb| {
                cb(self.callback_data);
            }
        }
    }

    // ============================================================
    // Byte-level VT state machine
    // ============================================================

    fn processByte(self: *VtPipeline, write_grid: u8, byte: u8) void {
        switch (self.state) {
            .ground => {
                if (byte >= 0x20 and byte <= 0x7E) {
                    self.putChar(write_grid, @as(u32, byte));
                } else if (byte == 0x1B) {
                    self.state = .escape;
                } else if (byte == 0x0A) {
                    // LF — line feed
                    if (self.cursor_row >= self.scroll_bottom) {
                        self.scrollUp(write_grid, 1);
                    } else {
                        self.cursor_row += 1;
                    }
                } else if (byte == 0x0D) {
                    // CR
                    self.cursor_col = 0;
                } else if (byte == 0x08) {
                    // BS
                    if (self.cursor_col > 0) self.cursor_col -= 1;
                } else if (byte == 0x09) {
                    // TAB — advance to next 8-column tab stop
                    self.cursor_col = @min((self.cursor_col + 8) & ~@as(u32, 7), self.cols - 1);
                } else if (byte == 0x07) {
                    // BEL — ignore
                } else if (byte >= 0xC0) {
                    // UTF-8 lead byte — emit replacement for now
                    self.putChar(write_grid, @as(u32, byte));
                }
            },
            .escape => {
                if (byte == '[') {
                    self.state = .csi_entry;
                    self.resetCsiParams();
                } else if (byte == ']') {
                    self.state = .osc_string;
                } else if (byte == 'P') {
                    self.state = .dcs_entry;
                } else if (byte == 'M') {
                    // RI — Reverse Index
                    if (self.cursor_row > self.scroll_top) {
                        self.cursor_row -= 1;
                    } else {
                        self.scrollDown(write_grid, 1);
                    }
                    self.state = .ground;
                } else if (byte == 'D') {
                    // IND — Index (same as LF)
                    if (self.cursor_row >= self.scroll_bottom) {
                        self.scrollUp(write_grid, 1);
                    } else {
                        self.cursor_row += 1;
                    }
                    self.state = .ground;
                } else if (byte == 'E') {
                    // NEL — Next Line
                    self.cursor_col = 0;
                    if (self.cursor_row >= self.scroll_bottom) {
                        self.scrollUp(write_grid, 1);
                    } else {
                        self.cursor_row += 1;
                    }
                    self.state = .ground;
                } else if (byte == 'c') {
                    // RIS — Full Reset
                    self.resetTerminal(write_grid);
                    self.state = .ground;
                } else {
                    self.state = .ground; // Unknown escape
                }
            },
            .csi_entry, .csi_param => {
                if (byte >= '0' and byte <= '9') {
                    self.accumulateCsiParam(byte);
                    self.state = .csi_param;
                } else if (byte == ';') {
                    self.nextCsiParam();
                    self.state = .csi_param;
                } else if (byte >= 0x40 and byte <= 0x7E) {
                    // CSI final byte
                    self.nextCsiParam(); // finalize last param
                    self.executeCsi(write_grid, byte);
                    self.state = .ground;
                } else if (byte == '?') {
                    // Private mode indicator — skip for now
                    self.state = .csi_param;
                } else {
                    self.state = .ground; // Invalid
                }
            },
            .osc_string => {
                if (byte == 0x07) {
                    self.state = .ground; // BEL terminates
                } else if (byte == 0x1B) {
                    self.state = .escape; // ST starts with ESC
                }
                // else: accumulate OSC (currently ignored)
            },
            .dcs_entry => {
                if (byte == 0x1B) {
                    self.state = .escape;
                } else if (byte == 0x07) {
                    self.state = .ground;
                }
            },
        }
    }

    // ============================================================
    // putChar — write a character to the grid
    // ============================================================

    fn putChar(self: *VtPipeline, write_grid: u8, codepoint: u32) void {
        if (self.cursor_col >= self.cols) {
            // Auto-wrap
            self.cursor_col = 0;
            if (self.cursor_row >= self.scroll_bottom) {
                self.scrollUp(write_grid, 1);
            } else {
                self.cursor_row += 1;
            }
        }

        const g = &self.grid[write_grid][self.cursor_row][self.cursor_col];
        g.* = .{
            .codepoint = codepoint,
            .fg_rgb = self.current_fg,
            .bg_rgb = self.current_bg,
            .flags = self.current_flags,
            .dirty = 1,
        };
        self.dirty_rows[write_grid][self.cursor_row] = true;
        _ = self.dirty_cell_count[write_grid].fetchAdd(1, .release);
        self.cursor_col += 1;
    }

    // ============================================================
    // CSI parameter handling
    // ============================================================

    fn resetCsiParams(self: *VtPipeline) void {
        @memset(&self.csi_params, 0);
        self.csi_param_count = 0;
        self.csi_current_param = 0;
    }

    fn accumulateCsiParam(self: *VtPipeline, byte: u8) void {
        self.csi_current_param = self.csi_current_param * 10 + @as(u32, byte - '0');
    }

    fn nextCsiParam(self: *VtPipeline) void {
        if (self.csi_param_count < CSI_MAX_PARAMS) {
            self.csi_params[self.csi_param_count] = self.csi_current_param;
            self.csi_param_count += 1;
        }
        self.csi_current_param = 0;
    }

    fn csiParam(self: *const VtPipeline, idx: usize, default: u32) u32 {
        if (idx < self.csi_param_count) {
            const v = self.csi_params[idx];
            return if (v == 0) default else v;
        }
        return default;
    }

    // ============================================================
    // CSI command dispatch
    // ============================================================

    fn executeCsi(self: *VtPipeline, write_grid: u8, final_byte: u8) void {
        switch (final_byte) {
            'm' => self.executeSgr(),
            'H', 'f' => {
                // CUP — Cursor Position
                const row = self.csiParam(0, 1);
                const col = self.csiParam(1, 1);
                self.cursor_row = @min(row -| 1, self.rows - 1);
                self.cursor_col = @min(col -| 1, self.cols - 1);
            },
            'A' => {
                // CUU — Cursor Up
                const n = self.csiParam(0, 1);
                self.cursor_row -|= n;
            },
            'B' => {
                // CUD — Cursor Down
                const n = self.csiParam(0, 1);
                self.cursor_row = @min(self.cursor_row + n, self.rows - 1);
            },
            'C' => {
                // CUF — Cursor Forward
                const n = self.csiParam(0, 1);
                self.cursor_col = @min(self.cursor_col + n, self.cols - 1);
            },
            'D' => {
                // CUB — Cursor Back
                const n = self.csiParam(0, 1);
                self.cursor_col -|= n;
            },
            'E' => {
                // CNL — Cursor Next Line
                const n = self.csiParam(0, 1);
                self.cursor_row = @min(self.cursor_row + n, self.rows - 1);
                self.cursor_col = 0;
            },
            'F' => {
                // CPL — Cursor Previous Line
                const n = self.csiParam(0, 1);
                self.cursor_row -|= n;
                self.cursor_col = 0;
            },
            'G' => {
                // CHA — Cursor Horizontal Absolute
                const col = self.csiParam(0, 1);
                self.cursor_col = @min(col -| 1, self.cols - 1);
            },
            'J' => {
                // ED — Erase in Display
                const mode = self.csiParam(0, 0);
                self.eraseInDisplay(write_grid, mode);
            },
            'K' => {
                // EL — Erase in Line
                const mode = self.csiParam(0, 0);
                self.eraseInLine(write_grid, mode);
            },
            'L' => {
                // IL — Insert Lines
                const n = self.csiParam(0, 1);
                self.insertLines(write_grid, n);
            },
            'M' => {
                // DL — Delete Lines
                const n = self.csiParam(0, 1);
                self.deleteLines(write_grid, n);
            },
            'P' => {
                // DCH — Delete Characters
                const n = self.csiParam(0, 1);
                self.deleteChars(write_grid, n);
            },
            'S' => {
                // SU — Scroll Up
                const n = self.csiParam(0, 1);
                self.scrollUp(write_grid, n);
            },
            'T' => {
                // SD — Scroll Down
                const n = self.csiParam(0, 1);
                self.scrollDown(write_grid, n);
            },
            'd' => {
                // VPA — Vertical Position Absolute
                const row = self.csiParam(0, 1);
                self.cursor_row = @min(row -| 1, self.rows - 1);
            },
            'r' => {
                // DECSTBM — Set Scrolling Region
                const top = self.csiParam(0, 1);
                const bottom = self.csiParam(1, self.rows);
                self.scroll_top = @min(top -| 1, self.rows - 1);
                self.scroll_bottom = @min(bottom -| 1, self.rows - 1);
                if (self.scroll_top > self.scroll_bottom) {
                    self.scroll_top = 0;
                    self.scroll_bottom = self.rows - 1;
                }
                self.cursor_row = 0;
                self.cursor_col = 0;
            },
            '@' => {
                // ICH — Insert Characters
                const n = self.csiParam(0, 1);
                self.insertChars(write_grid, n);
            },
            'X' => {
                // ECH — Erase Characters
                const n = self.csiParam(0, 1);
                self.eraseChars(write_grid, n);
            },
            else => {}, // Unknown CSI — ignore
        }
    }

    // ============================================================
    // SGR — Select Graphic Rendition
    // ============================================================

    fn executeSgr(self: *VtPipeline) void {
        if (self.csi_param_count == 0) {
            // ESC[m → reset
            self.current_fg = 0x00C0C0C0;
            self.current_bg = 0x00000000;
            self.current_flags = 0;
            return;
        }

        var i: usize = 0;
        while (i < self.csi_param_count) : (i += 1) {
            const p = self.csi_params[i];
            switch (p) {
                0 => {
                    self.current_fg = 0x00C0C0C0;
                    self.current_bg = 0x00000000;
                    self.current_flags = 0;
                },
                1 => self.current_flags |= (1 << 0), // bold
                2 => self.current_flags |= (1 << 4), // dim
                3 => self.current_flags |= (1 << 1), // italic
                4 => self.current_flags |= (1 << 2), // underline
                7 => self.current_flags |= (1 << 3), // reverse
                9 => self.current_flags |= (1 << 5), // strikethrough
                22 => self.current_flags &= ~@as(u16, (1 << 0) | (1 << 4)), // no bold/dim
                23 => self.current_flags &= ~@as(u16, 1 << 1), // no italic
                24 => self.current_flags &= ~@as(u16, 1 << 2), // no underline
                27 => self.current_flags &= ~@as(u16, 1 << 3), // no reverse
                29 => self.current_flags &= ~@as(u16, 1 << 5), // no strikethrough
                // Standard foreground colors (30-37)
                30...37 => self.current_fg = ansi256ToRgb(p - 30),
                // Bright foreground colors (90-97)
                90...97 => self.current_fg = ansi256ToRgb(p - 90 + 8),
                // Standard background colors (40-47)
                40...47 => self.current_bg = ansi256ToRgb(p - 40),
                // Bright background colors (100-107)
                100...107 => self.current_bg = ansi256ToRgb(p - 100 + 8),
                39 => self.current_fg = 0x00C0C0C0, // default fg
                49 => self.current_bg = 0x00000000, // default bg
                // Extended color: 38;2;r;g;b or 38;5;n
                38 => {
                    if (i + 1 < self.csi_param_count and self.csi_params[i + 1] == 2) {
                        // 24-bit: 38;2;r;g;b
                        if (i + 4 < self.csi_param_count) {
                            const r = self.csi_params[i + 2] & 0xFF;
                            const g = self.csi_params[i + 3] & 0xFF;
                            const b = self.csi_params[i + 4] & 0xFF;
                            self.current_fg = (r << 16) | (g << 8) | b;
                            i += 4;
                        }
                    } else if (i + 1 < self.csi_param_count and self.csi_params[i + 1] == 5) {
                        // 256-color: 38;5;n
                        if (i + 2 < self.csi_param_count) {
                            self.current_fg = ansi256ToRgb(self.csi_params[i + 2]);
                            i += 2;
                        }
                    }
                },
                // Extended color: 48;2;r;g;b or 48;5;n
                48 => {
                    if (i + 1 < self.csi_param_count and self.csi_params[i + 1] == 2) {
                        // 24-bit: 48;2;r;g;b
                        if (i + 4 < self.csi_param_count) {
                            const r = self.csi_params[i + 2] & 0xFF;
                            const g = self.csi_params[i + 3] & 0xFF;
                            const b = self.csi_params[i + 4] & 0xFF;
                            self.current_bg = (r << 16) | (g << 8) | b;
                            i += 4;
                        }
                    } else if (i + 1 < self.csi_param_count and self.csi_params[i + 1] == 5) {
                        // 256-color: 48;5;n
                        if (i + 2 < self.csi_param_count) {
                            self.current_bg = ansi256ToRgb(self.csi_params[i + 2]);
                            i += 2;
                        }
                    }
                },
                else => {}, // Unknown SGR param — ignore
            }
        }
    }

    // ============================================================
    // Erase operations
    // ============================================================

    fn eraseInDisplay(self: *VtPipeline, write_grid: u8, mode: u32) void {
        const empty = CellData{
            .fg_rgb = self.current_fg,
            .bg_rgb = self.current_bg,
        };

        switch (mode) {
            0 => {
                // Erase below (from cursor to end)
                var col = self.cursor_col;
                while (col < self.cols) : (col += 1) {
                    self.grid[write_grid][self.cursor_row][col] = empty;
                }
                self.dirty_rows[write_grid][self.cursor_row] = true;
                var row = self.cursor_row + 1;
                while (row < self.rows) : (row += 1) {
                    self.clearRow(write_grid, row, empty);
                }
            },
            1 => {
                // Erase above (from start to cursor)
                var row: u32 = 0;
                while (row < self.cursor_row) : (row += 1) {
                    self.clearRow(write_grid, row, empty);
                }
                var col: u32 = 0;
                while (col <= self.cursor_col and col < self.cols) : (col += 1) {
                    self.grid[write_grid][self.cursor_row][col] = empty;
                }
                self.dirty_rows[write_grid][self.cursor_row] = true;
            },
            2, 3 => {
                // Erase all
                var row: u32 = 0;
                while (row < self.rows) : (row += 1) {
                    self.clearRow(write_grid, row, empty);
                }
                self.cursor_row = 0;
                self.cursor_col = 0;
            },
            else => {},
        }
    }

    fn eraseInLine(self: *VtPipeline, write_grid: u8, mode: u32) void {
        const empty = CellData{
            .fg_rgb = self.current_fg,
            .bg_rgb = self.current_bg,
        };

        switch (mode) {
            0 => {
                // Erase to right
                var col = self.cursor_col;
                while (col < self.cols) : (col += 1) {
                    self.grid[write_grid][self.cursor_row][col] = empty;
                }
            },
            1 => {
                // Erase to left
                var col: u32 = 0;
                while (col <= self.cursor_col and col < self.cols) : (col += 1) {
                    self.grid[write_grid][self.cursor_row][col] = empty;
                }
            },
            2 => {
                // Erase entire line
                self.clearRow(write_grid, self.cursor_row, empty);
            },
            else => {},
        }
        self.dirty_rows[write_grid][self.cursor_row] = true;
    }

    fn eraseChars(self: *VtPipeline, write_grid: u8, n: u32) void {
        const empty = CellData{
            .fg_rgb = self.current_fg,
            .bg_rgb = self.current_bg,
        };
        var col = self.cursor_col;
        const end = @min(self.cursor_col + n, self.cols);
        while (col < end) : (col += 1) {
            self.grid[write_grid][self.cursor_row][col] = empty;
        }
        self.dirty_rows[write_grid][self.cursor_row] = true;
    }

    fn clearRow(self: *VtPipeline, write_grid: u8, row: u32, empty: CellData) void {
        var col: u32 = 0;
        while (col < self.cols) : (col += 1) {
            self.grid[write_grid][row][col] = empty;
        }
        self.dirty_rows[write_grid][row] = true;
    }

    // ============================================================
    // Scroll operations
    // ============================================================

    fn scrollUp(self: *VtPipeline, write_grid: u8, n: u32) void {
        const empty = CellData{
            .fg_rgb = self.current_fg,
            .bg_rgb = self.current_bg,
        };
        const lines = @min(n, self.scroll_bottom - self.scroll_top + 1);

        // Shift rows up
        var row = self.scroll_top;
        while (row + lines <= self.scroll_bottom) : (row += 1) {
            for (0..self.cols) |col| {
                self.grid[write_grid][row][col] = self.grid[write_grid][row + lines][col];
            }
            self.dirty_rows[write_grid][row] = true;
        }
        // Clear bottom lines
        while (row <= self.scroll_bottom) : (row += 1) {
            self.clearRow(write_grid, row, empty);
        }
    }

    fn scrollDown(self: *VtPipeline, write_grid: u8, n: u32) void {
        const empty = CellData{
            .fg_rgb = self.current_fg,
            .bg_rgb = self.current_bg,
        };
        const lines = @min(n, self.scroll_bottom - self.scroll_top + 1);

        // Shift rows down (iterate from bottom)
        var row = self.scroll_bottom;
        while (row >= self.scroll_top + lines) : (row -= 1) {
            for (0..self.cols) |col| {
                self.grid[write_grid][row][col] = self.grid[write_grid][row - lines][col];
            }
            self.dirty_rows[write_grid][row] = true;
            if (row == 0) break;
        }
        // Clear top lines
        var r = self.scroll_top;
        while (r < self.scroll_top + lines) : (r += 1) {
            self.clearRow(write_grid, r, empty);
        }
    }

    // ============================================================
    // Line / character insert & delete
    // ============================================================

    fn insertLines(self: *VtPipeline, write_grid: u8, n: u32) void {
        if (self.cursor_row < self.scroll_top or self.cursor_row > self.scroll_bottom) return;
        const empty = CellData{
            .fg_rgb = self.current_fg,
            .bg_rgb = self.current_bg,
        };
        const lines = @min(n, self.scroll_bottom - self.cursor_row + 1);

        // Shift lines down from cursor
        var row = self.scroll_bottom;
        while (row >= self.cursor_row + lines) : (row -= 1) {
            for (0..self.cols) |col| {
                self.grid[write_grid][row][col] = self.grid[write_grid][row - lines][col];
            }
            self.dirty_rows[write_grid][row] = true;
            if (row == 0) break;
        }
        // Clear inserted lines
        var r = self.cursor_row;
        while (r < self.cursor_row + lines) : (r += 1) {
            self.clearRow(write_grid, r, empty);
        }
    }

    fn deleteLines(self: *VtPipeline, write_grid: u8, n: u32) void {
        if (self.cursor_row < self.scroll_top or self.cursor_row > self.scroll_bottom) return;
        const empty = CellData{
            .fg_rgb = self.current_fg,
            .bg_rgb = self.current_bg,
        };
        const lines = @min(n, self.scroll_bottom - self.cursor_row + 1);

        // Shift lines up from cursor
        var row = self.cursor_row;
        while (row + lines <= self.scroll_bottom) : (row += 1) {
            for (0..self.cols) |col| {
                self.grid[write_grid][row][col] = self.grid[write_grid][row + lines][col];
            }
            self.dirty_rows[write_grid][row] = true;
        }
        // Clear bottom lines
        while (row <= self.scroll_bottom) : (row += 1) {
            self.clearRow(write_grid, row, empty);
        }
    }

    fn insertChars(self: *VtPipeline, write_grid: u8, n: u32) void {
        const count = @min(n, self.cols - self.cursor_col);
        const empty = CellData{
            .fg_rgb = self.current_fg,
            .bg_rgb = self.current_bg,
        };

        // Shift chars right
        var col = self.cols - 1;
        while (col >= self.cursor_col + count) : (col -= 1) {
            self.grid[write_grid][self.cursor_row][col] = self.grid[write_grid][self.cursor_row][col - count];
            if (col == 0) break;
        }
        // Clear inserted positions
        var c = self.cursor_col;
        while (c < self.cursor_col + count) : (c += 1) {
            self.grid[write_grid][self.cursor_row][c] = empty;
        }
        self.dirty_rows[write_grid][self.cursor_row] = true;
    }

    fn deleteChars(self: *VtPipeline, write_grid: u8, n: u32) void {
        const count = @min(n, self.cols - self.cursor_col);
        const empty = CellData{
            .fg_rgb = self.current_fg,
            .bg_rgb = self.current_bg,
        };

        // Shift chars left
        var col = self.cursor_col;
        while (col + count < self.cols) : (col += 1) {
            self.grid[write_grid][self.cursor_row][col] = self.grid[write_grid][self.cursor_row][col + count];
        }
        // Clear end
        while (col < self.cols) : (col += 1) {
            self.grid[write_grid][self.cursor_row][col] = empty;
        }
        self.dirty_rows[write_grid][self.cursor_row] = true;
    }

    // ============================================================
    // Terminal reset
    // ============================================================

    fn resetTerminal(self: *VtPipeline, write_grid: u8) void {
        self.state = .ground;
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.current_fg = 0x00C0C0C0;
        self.current_bg = 0x00000000;
        self.current_flags = 0;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;

        const empty = CellData{};
        var row: u32 = 0;
        while (row < self.rows) : (row += 1) {
            self.clearRow(write_grid, row, empty);
        }
    }
};

// ============================================================
// ANSI 256-color → 24-bit RGB lookup
// ============================================================

fn ansi256ToRgb(idx: u32) u32 {
    // Standard 16 colors
    const base16 = [16]u32{
        0x000000, 0xAA0000, 0x00AA00, 0xAA5500,
        0x0000AA, 0xAA00AA, 0x00AAAA, 0xAAAAAA,
        0x555555, 0xFF5555, 0x55FF55, 0xFFFF55,
        0x5555FF, 0xFF55FF, 0x55FFFF, 0xFFFFFF,
    };

    if (idx < 16) return base16[idx];

    if (idx < 232) {
        // 6x6x6 color cube (indices 16-231)
        const ci = idx - 16;
        const r_idx = ci / 36;
        const g_idx = (ci % 36) / 6;
        const b_idx = ci % 6;
        const r: u32 = if (r_idx == 0) 0 else 55 + r_idx * 40;
        const g: u32 = if (g_idx == 0) 0 else 55 + g_idx * 40;
        const b: u32 = if (b_idx == 0) 0 else 55 + b_idx * 40;
        return (r << 16) | (g << 8) | b;
    }

    if (idx < 256) {
        // Grayscale (indices 232-255)
        const level: u32 = 8 + (idx - 232) * 10;
        return (level << 16) | (level << 8) | level;
    }

    return 0xC0C0C0; // fallback
}

// ============================================================
// C FFI Exports
// ============================================================

export fn oag_vt_pipeline_create(cols: u32, rows: u32) ?*VtPipeline {
    const allocator = std.heap.page_allocator;
    const vt = allocator.create(VtPipeline) catch return null;
    vt.* = VtPipeline.init(cols, rows);
    return vt;
}

export fn oag_vt_pipeline_destroy(vt: *VtPipeline) void {
    vt.stop();
    const allocator = std.heap.page_allocator;
    allocator.destroy(vt);
}

export fn oag_vt_pipeline_set_pipe(vt: *VtPipeline, handle: ?*anyopaque) void {
    vt.pipe_handle = handle;
}

export fn oag_vt_pipeline_set_callback(
    vt: *VtPipeline,
    cb: ?*const fn (?*anyopaque) callconv(.c) void,
    data: ?*anyopaque,
) void {
    vt.on_frame_ready = cb;
    vt.callback_data = data;
}

export fn oag_vt_pipeline_start(vt: *VtPipeline) void {
    vt.start();
}

export fn oag_vt_pipeline_stop(vt: *VtPipeline) void {
    vt.stop();
}

export fn oag_vt_pipeline_snapshot_grid(vt: *const VtPipeline) [*]const CellData {
    return vt.snapshotGrid();
}

export fn oag_vt_pipeline_snapshot_generation(vt: *const VtPipeline) u64 {
    return vt.snapshotGeneration();
}

export fn oag_vt_pipeline_dirty_rows(vt: *const VtPipeline) [*]const bool {
    return vt.getDirtyRows();
}

export fn oag_vt_pipeline_dirty_cell_count(vt: *const VtPipeline) u32 {
    return vt.getDirtyCellCount();
}

export fn oag_vt_pipeline_grid_size(vt: *const VtPipeline, out_cols: *u32, out_rows: *u32) void {
    out_cols.* = vt.cols;
    out_rows.* = vt.rows;
}

export fn oag_vt_pipeline_cursor(vt: *const VtPipeline, out_row: *u32, out_col: *u32) void {
    out_row.* = vt.cursor_row;
    out_col.* = vt.cursor_col;
}

export fn oag_vt_pipeline_stats(vt: *const VtPipeline, out_bytes: *u64, out_frames: *u64, out_parse_us: *u64) void {
    out_bytes.* = vt.bytes_read;
    out_frames.* = vt.frames_produced;
    out_parse_us.* = vt.parse_time_us;
}

export fn oag_vt_pipeline_feed(vt: *VtPipeline, data: [*]const u8, len: u32) void {
    vt.feed(data, len);
}
