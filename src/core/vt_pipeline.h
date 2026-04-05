/* vt_pipeline.h — C FFI header for Zig VT pipeline
 *
 * 3-Phase async VT parser with lock-free double-buffered grid snapshots.
 * Thread-safe: main thread reads snapshots, pipeline threads parse PTY data.
 */

#ifndef OAG_VT_PIPELINE_H
#define OAG_VT_PIPELINE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ================================================================
 * Constants
 * ================================================================ */

#define OAG_GRID_MAX_COLS  512
#define OAG_GRID_MAX_ROWS  256
#define OAG_RAW_BUF_SIZE   (1024 * 1024)  /* 1MB */

/* ================================================================
 * CellData — per-cell terminal state
 *
 * flags layout:
 *   bit 0: bold
 *   bit 1: italic
 *   bit 2: underline
 *   bit 3: reverse
 *   bit 4: dim
 *   bit 5: strikethrough
 * ================================================================ */

typedef struct OagCellData {
    uint32_t codepoint;   /* Unicode codepoint (0 = empty) */
    uint32_t fg_rgb;      /* Foreground RGB (24-bit, 0x00RRGGBB) */
    uint32_t bg_rgb;      /* Background RGB (24-bit, 0x00RRGGBB) */
    uint16_t flags;       /* Attribute flags (see above) */
    uint16_t dirty;       /* Dirty generation counter */
} OagCellData;

/* Opaque pipeline handle */
typedef struct VtPipeline VtPipeline;

/* Frame-ready callback type */
typedef void (*OagFrameReadyCallback)(void* user_data);

/* ================================================================
 * Lifecycle
 * ================================================================ */

/* Create a new VT pipeline with given grid dimensions.
 * cols/rows are clamped to GRID_MAX_COLS/GRID_MAX_ROWS.
 * Returns NULL on allocation failure. */
VtPipeline* oag_vt_pipeline_create(uint32_t cols, uint32_t rows);

/* Stop threads and free the pipeline. */
void oag_vt_pipeline_destroy(VtPipeline* vt);

/* ================================================================
 * Configuration (call before start)
 * ================================================================ */

/* Set the PTY pipe handle (HANDLE from CreateProcess).
 * The pipeline reads from this handle in its read thread. */
void oag_vt_pipeline_set_pipe(VtPipeline* vt, void* handle);

/* Set the frame-ready callback.
 * Called from the parse thread when a new grid snapshot is available.
 * Typically used to trigger cx.notify() or similar main-thread wakeup. */
void oag_vt_pipeline_set_callback(VtPipeline* vt,
                                   OagFrameReadyCallback cb,
                                   void* user_data);

/* ================================================================
 * Thread control
 * ================================================================ */

/* Start the read and parse threads. Requires pipe handle to be set. */
void oag_vt_pipeline_start(VtPipeline* vt);

/* Stop threads and wait for them to join. */
void oag_vt_pipeline_stop(VtPipeline* vt);

/* ================================================================
 * Snapshot access (main thread, read-only)
 *
 * The returned pointers point into the active (read-only) grid buffer.
 * They remain valid until the next grid swap (next parse cycle).
 * Main thread should read and consume before the next frame.
 * ================================================================ */

/* Get pointer to the active grid's cell data.
 * Layout: row-major, [rows][cols] CellData.
 * Total cells = rows * cols. Access: grid[row * cols + col]. */
const OagCellData* oag_vt_pipeline_snapshot_grid(const VtPipeline* vt);

/* Get the current generation counter (increments on each frame). */
uint64_t oag_vt_pipeline_snapshot_generation(const VtPipeline* vt);

/* Get per-row dirty flags for the active grid.
 * Array of GRID_MAX_ROWS bools. true = row changed since last swap. */
const uint8_t* oag_vt_pipeline_dirty_rows(const VtPipeline* vt);

/* Get total number of dirty cells in the active grid. */
uint32_t oag_vt_pipeline_dirty_cell_count(const VtPipeline* vt);

/* Get current grid dimensions. */
void oag_vt_pipeline_grid_size(const VtPipeline* vt,
                                uint32_t* out_cols,
                                uint32_t* out_rows);

/* Get current cursor position. */
void oag_vt_pipeline_cursor(const VtPipeline* vt,
                             uint32_t* out_row,
                             uint32_t* out_col);

/* ================================================================
 * Statistics
 * ================================================================ */

/* Get pipeline statistics.
 * bytes_read: total bytes read from PTY
 * frames: total grid snapshots produced
 * parse_us: cumulative parse time in microseconds */
void oag_vt_pipeline_stats(const VtPipeline* vt,
                            uint64_t* bytes_read,
                            uint64_t* frames,
                            uint64_t* parse_us);

/* ================================================================
 * Direct feed (testing / non-PTY input)
 * ================================================================ */

/* Feed raw bytes into the pipeline's ring buffer.
 * Can be called from any thread. Useful for testing without a PTY. */
void oag_vt_pipeline_feed(VtPipeline* vt, const uint8_t* data, uint32_t len);

#ifdef __cplusplus
}
#endif

#endif /* OAG_VT_PIPELINE_H */
