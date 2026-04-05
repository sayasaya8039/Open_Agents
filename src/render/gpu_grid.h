// Open_Agents — DX12 GPU Grid Renderer
// Instanced-draw grid with DirectWrite glyph atlas
// Architecture:
//   1. DirectWrite rasterizes glyphs → R8 atlas bitmap (CPU, one-time)
//   2. Atlas uploaded to GPU texture (one-time, or on font change)
//   3. Cell structured buffer: { glyph_idx, fg_rgba, bg_rgba, flags }
//   4. DrawInstanced(6, cell_count) — quad per cell, atlas sampling in PS
//   5. Render target → readback buffer → CPU-accessible RGBA pixels

#ifndef OAG_GPU_GRID_H
#define OAG_GPU_GRID_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Per-cell data for the structured buffer
typedef struct {
    uint32_t glyph_idx;   // atlas index (0 = empty, 1+ = glyph)
    uint32_t fg_rgba;     // foreground color packed RGBA
    uint32_t bg_rgba;     // background color packed RGBA
    uint32_t flags;       // bit 0=bold, 1=italic, 2=underline, 3=reverse
} oag_gpu_cell_t;

// Opaque renderer handle
typedef struct oag_gpu_grid oag_gpu_grid_t;

// Lifecycle
oag_gpu_grid_t* oag_gpu_grid_create(uint32_t width, uint32_t height, float font_size);
void            oag_gpu_grid_destroy(oag_gpu_grid_t* r);

// Glyph rasterization — returns atlas index for the codepoint
uint32_t oag_gpu_grid_rasterize_glyph(oag_gpu_grid_t* r, uint32_t codepoint);

// Update dirty cells in the structured buffer
void oag_gpu_grid_update_cells(
    oag_gpu_grid_t* r,
    const oag_gpu_cell_t* cells,
    uint32_t count,
    const uint32_t* dirty_indices,
    uint32_t dirty_count
);

// Render one frame — returns pointer to RGBA readback pixels (width * height * 4)
const uint8_t* oag_gpu_grid_render(
    oag_gpu_grid_t* r,
    uint32_t cell_count,
    uint32_t term_cols,
    float cell_w,
    float cell_h
);

// Resize the render target + readback buffer
uint8_t oag_gpu_grid_resize(oag_gpu_grid_t* r, uint32_t width, uint32_t height);

// Accessors
uint32_t oag_gpu_grid_width(const oag_gpu_grid_t* r);
uint32_t oag_gpu_grid_height(const oag_gpu_grid_t* r);
uint32_t oag_gpu_grid_pixel_stride(const oag_gpu_grid_t* r);

#ifdef __cplusplus
}
#endif

#endif // OAG_GPU_GRID_H
