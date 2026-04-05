/**
 * event_loop.h — C FFI header for Zig async event loop with 4ms batching
 *
 * Usage:
 *   oag_event_loop_t* el = oag_event_loop_create();
 *   oag_event_loop_set_callback(el, my_callback, my_data);
 *   oag_event_loop_start(el);
 *   // ... in callback or main loop:
 *   oag_event_t buf[256];
 *   uint32_t n = oag_event_loop_drain(el, buf, 256);
 *   // process n events ...
 *   oag_event_loop_destroy(el);
 */

#ifndef OAG_EVENT_LOOP_H
#define OAG_EVENT_LOOP_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    OAG_EVENT_KEY_PRESS   = 0,
    OAG_EVENT_RESIZE      = 1,
    OAG_EVENT_TIMER_TICK  = 2,
    OAG_EVENT_AI_RESPONSE = 3,
    OAG_EVENT_QUIT        = 4,
} oag_event_type_t;

/* Modifier flags */
#define OAG_MOD_CTRL  0x01
#define OAG_MOD_ALT   0x02
#define OAG_MOD_SHIFT 0x04

typedef struct {
    uint8_t  type;        /* oag_event_type_t */
    int32_t  key_code;    /* Virtual key code (key_press only) */
    uint8_t  modifier;    /* OAG_MOD_* flags */
    uint8_t  _pad[2];
    uint64_t timestamp_ms;
} oag_event_t;

typedef void (*oag_batch_callback_t)(void* user_data);

typedef struct oag_event_loop oag_event_loop_t;

/**
 * Create a new event loop. Returns NULL on allocation failure.
 */
oag_event_loop_t* oag_event_loop_create(void);

/**
 * Set the batch-ready callback. Called from the timer thread when the
 * 4ms batch window expires and there are pending events.
 */
void oag_event_loop_set_callback(oag_event_loop_t* el,
                                  oag_batch_callback_t cb,
                                  void* user_data);

/**
 * Start the event loop (spawns input + timer threads).
 */
void oag_event_loop_start(oag_event_loop_t* el);

/**
 * Stop the event loop (joins threads).
 */
void oag_event_loop_stop(oag_event_loop_t* el);

/**
 * Destroy the event loop (stops + frees).
 */
void oag_event_loop_destroy(oag_event_loop_t* el);

/**
 * Drain batched events into events_out. Returns number of events copied.
 * Call from the main thread (typically in the batch callback).
 */
uint32_t oag_event_loop_drain(oag_event_loop_t* el,
                               oag_event_t* events_out,
                               uint32_t max_events);

/**
 * Push an external event (e.g., AI response notification).
 * Thread-safe (single producer assumption — serialize external pushes).
 * Returns true on success, false if ring buffer is full.
 */
bool oag_event_loop_push(oag_event_loop_t* el, const oag_event_t* event);

/**
 * Get performance statistics.
 */
void oag_event_loop_stats(const oag_event_loop_t* el,
                           uint64_t* events_received,
                           uint64_t* batches_dispatched,
                           uint64_t* events_coalesced);

#ifdef __cplusplus
}
#endif

#endif /* OAG_EVENT_LOOP_H */
