// event_loop.zig — Zig async event loop with 4ms batching for Open Agents
// Windows-only, lock-free SPSC ring buffer, C FFI exports

const std = @import("std");

// ─── Windows API extern declarations ───

extern "kernel32" fn GetStdHandle(nStdHandle: u32) callconv(.c) ?*anyopaque;
extern "kernel32" fn WaitForSingleObject(hHandle: ?*anyopaque, dwMilliseconds: u32) callconv(.c) u32;
extern "kernel32" fn ReadConsoleInputW(
    hConsoleInput: ?*anyopaque,
    lpBuffer: [*]INPUT_RECORD,
    nLength: u32,
    lpNumberOfEventsRead: *u32,
) callconv(.c) i32;
extern "kernel32" fn QueryPerformanceCounter(lpPerformanceCount: *i64) callconv(.c) i32;
extern "kernel32" fn QueryPerformanceFrequency(lpFrequency: *i64) callconv(.c) i32;
extern "kernel32" fn Sleep(dwMilliseconds: u32) callconv(.c) void;

const STD_INPUT_HANDLE: u32 = 0xFFFFFFF6; // (DWORD)-10
const WAIT_OBJECT_0: u32 = 0;
const KEY_EVENT: u16 = 0x0001;
const WINDOW_BUFFER_SIZE_EVENT: u16 = 0x0004;

// ─── Windows struct definitions ───

const KEY_EVENT_RECORD = extern struct {
    bKeyDown: i32,
    wRepeatCount: u16,
    wVirtualKeyCode: u16,
    wVirtualScanCode: u16,
    uChar: extern union {
        UnicodeChar: u16,
        AsciiChar: u8,
    },
    dwControlKeyState: u32,
};

const WINDOW_BUFFER_SIZE_RECORD = extern struct {
    dwSizeX: i16,
    dwSizeY: i16,
};

const INPUT_RECORD = extern struct {
    EventType: u16,
    _pad: u16,
    Event: extern union {
        KeyEvent: KEY_EVENT_RECORD,
        WindowBufferSizeEvent: WINDOW_BUFFER_SIZE_RECORD,
        _other: [16]u8,
    },
};

// ─── Modifier flags ───

const MOD_CTRL: u8 = 0x01;
const MOD_ALT: u8 = 0x02;
const MOD_SHIFT: u8 = 0x04;

// Windows dwControlKeyState masks
const LEFT_ALT_PRESSED: u32 = 0x0002;
const RIGHT_ALT_PRESSED: u32 = 0x0001;
const LEFT_CTRL_PRESSED: u32 = 0x0008;
const RIGHT_CTRL_PRESSED: u32 = 0x0004;
const SHIFT_PRESSED: u32 = 0x0010;

// ─── Constants ───

const RING_SIZE: u32 = 4096;
const RING_MASK: u32 = RING_SIZE - 1;
const BATCH_WINDOW_MS: u64 = 4;
const TIMER_INTERVAL_MS: u64 = 2;

// ─── Event types ───

pub const EventType = enum(u8) {
    key_press = 0,
    resize = 1,
    timer_tick = 2,
    ai_response = 3,
    quit = 4,
};

pub const Event = extern struct {
    type: EventType,
    key_code: i32,
    modifier: u8,
    _pad: [2]u8 = .{ 0, 0 },
    timestamp_ms: u64,
};

// ─── Timestamp ───

var qpc_freq: i64 = 0;

fn getTimestampMs() u64 {
    if (qpc_freq == 0) {
        _ = QueryPerformanceFrequency(&qpc_freq);
    }
    var cnt: i64 = 0;
    _ = QueryPerformanceCounter(&cnt);
    return @intCast(@divTrunc(cnt * 1000, qpc_freq));
}

// ─── EventLoop ───

pub const EventLoop = struct {
    // Ring buffer (SPSC lock-free)
    ring: [RING_SIZE]Event,
    write_pos: std.atomic.Value(u32),
    read_pos: std.atomic.Value(u32),

    // Batch state
    batch_start_ms: u64,
    has_pending: std.atomic.Value(bool),

    // Threads
    input_thread: ?std.Thread,
    timer_thread: ?std.Thread,
    running: std.atomic.Value(bool),

    // Callback (C function pointer)
    on_batch_ready: ?*const fn (?*anyopaque) callconv(.c) void,
    user_data: ?*anyopaque,

    // Performance counters
    events_received: u64,
    batches_dispatched: u64,
    events_coalesced: u64,

    const Self = @This();

    pub fn init() Self {
        return Self{
            .ring = undefined,
            .write_pos = std.atomic.Value(u32).init(0),
            .read_pos = std.atomic.Value(u32).init(0),
            .batch_start_ms = 0,
            .has_pending = std.atomic.Value(bool).init(false),
            .input_thread = null,
            .timer_thread = null,
            .running = std.atomic.Value(bool).init(false),
            .on_batch_ready = null,
            .user_data = null,
            .events_received = 0,
            .batches_dispatched = 0,
            .events_coalesced = 0,
        };
    }

    /// Push an event into the ring buffer (producer side)
    pub fn pushEvent(self: *Self, event: Event) bool {
        const wp = self.write_pos.load(.acquire);
        const rp = self.read_pos.load(.acquire);
        const next_wp = (wp + 1) & RING_MASK;
        if (next_wp == rp) return false; // full

        self.ring[wp] = event;
        self.write_pos.store(next_wp, .release);

        // Start batch window on first pending event
        if (!self.has_pending.load(.acquire)) {
            self.batch_start_ms = getTimestampMs();
            self.has_pending.store(true, .release);
        }
        self.events_received += 1;
        return true;
    }

    /// Drain events from ring buffer (consumer side, called from main thread)
    pub fn drain(self: *Self, events_out: [*]Event, max_events: u32) u32 {
        var count: u32 = 0;
        var rp = self.read_pos.load(.acquire);
        const wp = self.write_pos.load(.acquire);

        while (rp != wp and count < max_events) {
            events_out[count] = self.ring[rp];
            rp = (rp + 1) & RING_MASK;
            count += 1;
        }
        self.read_pos.store(rp, .release);
        return count;
    }

    /// Start input + timer threads
    pub fn start(self: *Self) void {
        self.running.store(true, .release);
        self.input_thread = std.Thread.spawn(.{}, inputThreadFn, .{self}) catch null;
        self.timer_thread = std.Thread.spawn(.{}, timerThreadFn, .{self}) catch null;
    }

    /// Stop threads
    pub fn stop(self: *Self) void {
        self.running.store(false, .release);
        if (self.input_thread) |t| {
            t.join();
            self.input_thread = null;
        }
        if (self.timer_thread) |t| {
            t.join();
            self.timer_thread = null;
        }
    }
};

// ─── Thread functions ───

fn extractModifiers(state: u32) u8 {
    var mods: u8 = 0;
    if (state & (LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED) != 0) mods |= MOD_CTRL;
    if (state & (LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED) != 0) mods |= MOD_ALT;
    if (state & SHIFT_PRESSED != 0) mods |= MOD_SHIFT;
    return mods;
}

fn inputThreadFn(el: *EventLoop) void {
    const hin = GetStdHandle(STD_INPUT_HANDLE) orelse return;

    var input_buf: [64]INPUT_RECORD = undefined;

    while (el.running.load(.acquire)) {
        // Wait for console input with 50ms timeout
        const wait_result = WaitForSingleObject(hin, 50);
        if (wait_result != WAIT_OBJECT_0) continue;

        var events_read: u32 = 0;
        const rc = ReadConsoleInputW(hin, &input_buf, 64, &events_read);
        if (rc == 0) continue;

        const now = getTimestampMs();

        var i: u32 = 0;
        while (i < events_read) : (i += 1) {
            const rec = input_buf[i];

            switch (rec.EventType) {
                KEY_EVENT => {
                    const ke = rec.Event.KeyEvent;
                    if (ke.bKeyDown == 0) continue; // key up, skip

                    const event = Event{
                        .type = .key_press,
                        .key_code = @as(i32, ke.wVirtualKeyCode),
                        .modifier = extractModifiers(ke.dwControlKeyState),
                        .timestamp_ms = now,
                    };
                    _ = el.pushEvent(event);
                },
                WINDOW_BUFFER_SIZE_EVENT => {
                    const event = Event{
                        .type = .resize,
                        .key_code = 0,
                        .modifier = 0,
                        .timestamp_ms = now,
                    };
                    _ = el.pushEvent(event);
                },
                else => {},
            }
        }
    }
}

fn timerThreadFn(el: *EventLoop) void {
    while (el.running.load(.acquire)) {
        Sleep(TIMER_INTERVAL_MS);

        if (el.has_pending.load(.acquire)) {
            const now = getTimestampMs();
            if (now - el.batch_start_ms >= BATCH_WINDOW_MS) {
                // Batch window expired → fire callback
                el.has_pending.store(false, .release);
                el.batches_dispatched += 1;
                if (el.on_batch_ready) |cb| {
                    cb(el.user_data);
                }
            }
        }
    }
}

// ─── C FFI exports ───

export fn oag_event_loop_create() ?*EventLoop {
    const allocator = std.heap.page_allocator;
    const el = allocator.create(EventLoop) catch return null;
    el.* = EventLoop.init();
    return el;
}

export fn oag_event_loop_set_callback(
    el: *EventLoop,
    cb: *const fn (?*anyopaque) callconv(.c) void,
    user_data: ?*anyopaque,
) void {
    el.on_batch_ready = cb;
    el.user_data = user_data;
}

export fn oag_event_loop_start(el: *EventLoop) void {
    el.start();
}

export fn oag_event_loop_stop(el: *EventLoop) void {
    el.stop();
}

export fn oag_event_loop_destroy(el: *EventLoop) void {
    el.stop();
    const allocator = std.heap.page_allocator;
    allocator.destroy(el);
}

export fn oag_event_loop_drain(
    el: *EventLoop,
    events_out: [*]Event,
    max_events: u32,
) u32 {
    return el.drain(events_out, max_events);
}

export fn oag_event_loop_push(el: *EventLoop, event: *const Event) bool {
    return el.pushEvent(event.*);
}

export fn oag_event_loop_stats(
    el: *const EventLoop,
    events_received: *u64,
    batches_dispatched: *u64,
    events_coalesced: *u64,
) void {
    events_received.* = el.events_received;
    batches_dispatched.* = el.batches_dispatched;
    events_coalesced.* = el.events_coalesced;
}
