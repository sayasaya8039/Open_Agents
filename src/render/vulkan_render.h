// Open_Agents — Vulkan Renderer (超高速描画)
// GPU-accelerated UI rendering via Vulkan
#ifndef OAG_VULKAN_RENDER_H
#define OAG_VULKAN_RENDER_H

#include <stdint.h>
#include <stdbool.h>

// Forward declare Vulkan types (avoid including vulkan.h everywhere)
typedef struct VkInstance_T*       VkInstance;
typedef struct VkPhysicalDevice_T* VkPhysicalDevice;
typedef struct VkDevice_T*         VkDevice;
typedef struct VkQueue_T*          VkQueue;
typedef struct VkSurfaceKHR_T*     VkSurfaceKHR;
typedef struct VkSwapchainKHR_T*   VkSwapchainKHR;
typedef struct VkCommandPool_T*    VkCommandPool;
typedef struct VkRenderPass_T*     VkRenderPass;
typedef struct VkPipeline_T*       VkPipeline;

typedef struct {
    uint32_t width;
    uint32_t height;
    bool     vsync;
    bool     triple_buffer;
    uint32_t target_fps;       // 0 = unlimited
    bool     gpu_compute;      // use compute shaders for tensor ops
} oag_vk_config_t;

typedef struct {
    oag_vk_config_t  config;
    VkInstance        instance;
    VkPhysicalDevice  physical_device;
    VkDevice          device;
    VkQueue           graphics_queue;
    VkQueue           compute_queue;   // for tensor compute
    VkSurfaceKHR      surface;
    VkSwapchainKHR    swapchain;
    VkCommandPool     cmd_pool;
    VkRenderPass      render_pass;
    void*             _internal;     // vk_internal_t* (avoid type-pun via cmd_pool)

    // GPU info
    char              gpu_name[256];
    uint64_t          vram_total;
    uint64_t          vram_used;

    // Frame timing
    double            frame_time_ms;
    uint64_t          frame_count;
    double            fps;

    bool              initialized;
} oag_vk_ctx_t;

// Default config
oag_vk_config_t oag_vk_default_config(void);

// Lifecycle
oag_vk_ctx_t* oag_vk_create(oag_vk_config_t config);
void          oag_vk_destroy(oag_vk_ctx_t* ctx);

// Per-frame
bool oag_vk_begin_frame(oag_vk_ctx_t* ctx);
void oag_vk_end_frame(oag_vk_ctx_t* ctx);

// Text rendering (for code editor UI)
void oag_vk_draw_text(oag_vk_ctx_t* ctx, float x, float y,
                      const char* text, uint32_t color);

// Rectangle (for UI panels)
void oag_vk_draw_rect(oag_vk_ctx_t* ctx, float x, float y,
                      float w, float h, uint32_t color);

// GPU compute dispatch (for tensor operations)
void oag_vk_dispatch_matmul(oag_vk_ctx_t* ctx,
                             const float* a, int M, int K,
                             const float* b, int K2, int N,
                             float* out);

// Stats
void oag_vk_print_stats(const oag_vk_ctx_t* ctx);

#endif // OAG_VULKAN_RENDER_H
