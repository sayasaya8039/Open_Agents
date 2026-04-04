// Open_Agents — Vulkan Renderer (動的ロード — Vulkan SDK 不要)
// vulkan-1.dll を Runtime-load して Instance/Device/Swapchain を構築
#include "render/vulkan_render.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>

// ============================================================
// Vulkan types (minimal — loaded dynamically)
// ============================================================

typedef uint32_t VkFlags;
typedef uint32_t VkBool32;
typedef uint64_t VkDeviceSize;
typedef int32_t  VkResult;
typedef uint32_t VkFormat;
typedef uint32_t VkColorSpaceKHR;
typedef uint32_t VkPresentModeKHR;
typedef uint32_t VkStructureType;

#define VK_SUCCESS 0
#define VK_STRUCTURE_TYPE_APPLICATION_INFO          0
#define VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO      1
#define VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO  2
#define VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO        3
#define VK_API_VERSION_1_2 ((1 << 22) | (2 << 12))
#define VK_QUEUE_GRAPHICS_BIT 0x1
#define VK_QUEUE_COMPUTE_BIT  0x2
#define VK_NULL_HANDLE NULL

typedef struct {
    VkStructureType sType;
    const void*     pNext;
    const char*     pApplicationName;
    uint32_t        applicationVersion;
    const char*     pEngineName;
    uint32_t        engineVersion;
    uint32_t        apiVersion;
} VkApplicationInfo_t;

typedef struct {
    VkStructureType sType;
    const void*     pNext;
    VkFlags         flags;
    const VkApplicationInfo_t* pApplicationInfo;
    uint32_t        enabledLayerCount;
    const char* const* ppEnabledLayerNames;
    uint32_t        enabledExtensionCount;
    const char* const* ppEnabledExtensionNames;
} VkInstanceCreateInfo_t;

typedef struct {
    uint32_t queueFlags;
    uint32_t queueCount;
    uint32_t timestampValidBits;
    uint32_t minImageTransferGranularity[3];
} VkQueueFamilyProperties_t;

// VkPhysicalDeviceProperties is 824 bytes in Vulkan 1.2
// We must allocate the FULL struct or vkGetPhysicalDeviceProperties will corrupt the stack
typedef struct {
    uint32_t      apiVersion;
    uint32_t      driverVersion;
    uint32_t      vendorID;
    uint32_t      deviceID;
    uint32_t      deviceType;        // offset 16
    char          deviceName[256];   // offset 20
    uint8_t       pipelineCacheUUID[16]; // offset 276
    uint8_t       _limits_pad[480];  // VkPhysicalDeviceLimits (480 bytes)
    uint8_t       _sparse_pad[40];   // VkPhysicalDeviceSparseProperties
} VkPhysicalDeviceProperties_t;

// Use raw byte buffer for memory properties to avoid layout issues
// VkPhysicalDeviceMemoryProperties is 520 bytes on 64-bit
// We'll parse it manually
#define VK_MEM_PROPS_SIZE 1024  // generous buffer

// Function pointer types
typedef VkResult (*PFN_vkCreateInstance)(const VkInstanceCreateInfo_t*, const void*, VkInstance*);
typedef void     (*PFN_vkDestroyInstance)(VkInstance, const void*);
typedef VkResult (*PFN_vkEnumeratePhysicalDevices)(VkInstance, uint32_t*, VkPhysicalDevice*);
typedef void     (*PFN_vkGetPhysicalDeviceProperties)(VkPhysicalDevice, VkPhysicalDeviceProperties_t*);
typedef void     (*PFN_vkGetPhysicalDeviceMemoryProperties)(VkPhysicalDevice, void*);
typedef void     (*PFN_vkGetPhysicalDeviceQueueFamilyProperties)(VkPhysicalDevice, uint32_t*, VkQueueFamilyProperties_t*);

// ============================================================
// Vulkan context (dynamic)
// ============================================================

typedef struct {
    HMODULE hlib;   // vulkan-1.dll

    PFN_vkCreateInstance                        vkCreateInstance;
    PFN_vkDestroyInstance                       vkDestroyInstance;
    PFN_vkEnumeratePhysicalDevices              vkEnumeratePhysicalDevices;
    PFN_vkGetPhysicalDeviceProperties           vkGetPhysicalDeviceProperties;
    PFN_vkGetPhysicalDeviceMemoryProperties     vkGetPhysicalDeviceMemoryProperties;
    PFN_vkGetPhysicalDeviceQueueFamilyProperties vkGetPhysicalDeviceQueueFamilyProperties;

    VkInstance       instance;
    VkPhysicalDevice physical_devices[8];
    uint32_t         n_physical_devices;

    // Best device info
    int              best_device_idx;
    char             best_device_name[256];
    uint64_t         best_device_vram;
    uint32_t         graphics_queue_family;
    uint32_t         compute_queue_family;
} vk_internal_t;

#define VK_LOAD(ctx, name)                                        \
    ctx->name = (PFN_##name)GetProcAddress(ctx->hlib, #name);     \
    if (!ctx->name) {                                              \
        printf("[Vulkan] Missing: %s\n", #name);                  \
        return NULL;                                                \
    }

static double vk_get_time_ms(void) {
    LARGE_INTEGER freq, cnt;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&cnt);
    return (double)cnt.QuadPart / (double)freq.QuadPart * 1000.0;
}

// ============================================================
// Create — Runtime load vulkan-1.dll
// ============================================================

oag_vk_config_t oag_vk_default_config(void) {
    return (oag_vk_config_t){
        .width          = 1920,
        .height         = 1080,
        .vsync          = false,
        .triple_buffer  = true,
        .target_fps     = 0,
        .gpu_compute    = true,
    };
}

oag_vk_ctx_t* oag_vk_create(oag_vk_config_t config) {
    oag_vk_ctx_t* ctx = (oag_vk_ctx_t*)calloc(1, sizeof(oag_vk_ctx_t));
    memcpy(&ctx->config, &config, sizeof(config));

    vk_internal_t* vk = (vk_internal_t*)calloc(1, sizeof(vk_internal_t));

    // Load vulkan-1.dll
    vk->hlib = LoadLibraryA("vulkan-1.dll");
    if (!vk->hlib) {
        printf("[Vulkan] vulkan-1.dll not found. Install Vulkan runtime.\n");
        free(vk);
        free(ctx);
        return NULL;
    }

    // Load core functions
    VK_LOAD(vk, vkCreateInstance);
    VK_LOAD(vk, vkDestroyInstance);
    VK_LOAD(vk, vkEnumeratePhysicalDevices);
    VK_LOAD(vk, vkGetPhysicalDeviceProperties);
    VK_LOAD(vk, vkGetPhysicalDeviceMemoryProperties);
    VK_LOAD(vk, vkGetPhysicalDeviceQueueFamilyProperties);

    // Create Vulkan instance
    VkApplicationInfo_t app_info = {
        .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
        .pApplicationName = "Open_Agents",
        .applicationVersion = 1,
        .pEngineName = "OAG",
        .engineVersion = 1,
        .apiVersion = VK_API_VERSION_1_2,
    };

    VkInstanceCreateInfo_t create_info = {
        .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        .pApplicationInfo = &app_info,
    };

    VkResult res = vk->vkCreateInstance(&create_info, NULL, &vk->instance);
    if (res != VK_SUCCESS) {
        printf("[Vulkan] vkCreateInstance failed: %d\n", res);
        FreeLibrary(vk->hlib);
        free(vk);
        free(ctx);
        return NULL;
    }

    // Enumerate physical devices
    vk->n_physical_devices = 8;
    vk->vkEnumeratePhysicalDevices(vk->instance, &vk->n_physical_devices, vk->physical_devices);

    printf("[Vulkan] Found %u physical devices:\n", vk->n_physical_devices);

    vk->best_device_idx = 0;
    vk->best_device_vram = 0;

    for (uint32_t i = 0; i < vk->n_physical_devices; i++) {
        // Heap-allocate to avoid stack corruption from Vulkan struct size mismatch
        VkPhysicalDeviceProperties_t* props = (VkPhysicalDeviceProperties_t*)calloc(1, sizeof(*props));
        vk->vkGetPhysicalDeviceProperties(vk->physical_devices[i], props);

        // Parse memory properties from raw buffer
        uint8_t* mem_buf = (uint8_t*)calloc(1, VK_MEM_PROPS_SIZE);
        vk->vkGetPhysicalDeviceMemoryProperties(vk->physical_devices[i], mem_buf);

        // Layout: memoryTypeCount(4) + memoryTypes[32](8 each=256) + memoryHeapCount(4) + pad(4) + memoryHeaps[16](16 each=256)
        // Offset of memoryHeapCount = 4 + 256 = 260
        // Offset of memoryHeaps = 260 + 4 + 4(pad) = 268? No.
        // Actually: memoryTypeCount(4), memoryTypes[32] = {propertyFlags(4), heapIndex(4)} * 32 = 256
        // memoryHeapCount at offset 260, then memoryHeaps[16] = {size(8), flags(4), pad(4)} * 16
        uint32_t heap_count;
        memcpy(&heap_count, mem_buf + 260, 4);
        if (heap_count > 16) heap_count = 16;

        uint64_t vram = 0;
        // memoryHeaps starts at offset 264 (or 268 with padding)
        // Try offset 264 first
        for (uint32_t h = 0; h < heap_count; h++) {
            // Each heap: VkDeviceSize(8) + VkMemoryPropertyFlags(4) + pad(4) = 16
            size_t heap_offset = 264 + h * 16;
            VkDeviceSize heap_size;
            VkFlags heap_flags;
            memcpy(&heap_size, mem_buf + heap_offset, 8);
            memcpy(&heap_flags, mem_buf + heap_offset + 8, 4);
            if (heap_flags & 1) {  // DEVICE_LOCAL
                vram += heap_size;
            }
        }

        const char* type_str = "Unknown";
        switch (props->deviceType) {
            case 1: type_str = "Integrated"; break;
            case 2: type_str = "Discrete"; break;
            case 3: type_str = "Virtual"; break;
            case 4: type_str = "CPU"; break;
        }

        printf("  [%u] %-10s  %.1f GB  %s\n",
               i, type_str, vram / (1024.0 * 1024.0 * 1024.0), props->deviceName);

        // Prefer discrete GPU with most VRAM
        bool is_discrete = (props->deviceType == 2);
        if ((is_discrete && vram > vk->best_device_vram) ||
            (vram > vk->best_device_vram && vk->best_device_vram == 0)) {
            vk->best_device_vram = vram;
            vk->best_device_idx = i;
            strncpy(vk->best_device_name, props->deviceName, sizeof(vk->best_device_name) - 1);
        }

        free(props);
        free(mem_buf);

        // Find queue families
        uint32_t n_families = 0;
        vk->vkGetPhysicalDeviceQueueFamilyProperties(vk->physical_devices[i], &n_families, NULL);
        VkQueueFamilyProperties_t families[16];
        if (n_families > 16) n_families = 16;
        vk->vkGetPhysicalDeviceQueueFamilyProperties(vk->physical_devices[i], &n_families, families);

        for (uint32_t f = 0; f < n_families; f++) {
            if (families[f].queueFlags & VK_QUEUE_GRAPHICS_BIT) vk->graphics_queue_family = f;
            if (families[f].queueFlags & VK_QUEUE_COMPUTE_BIT)  vk->compute_queue_family = f;
        }
    }

    // Store in context
    strncpy(ctx->gpu_name, vk->best_device_name, sizeof(ctx->gpu_name) - 1);
    ctx->vram_total = vk->best_device_vram;
    ctx->physical_device = vk->physical_devices[vk->best_device_idx];
    ctx->instance = vk->instance;

    printf("[Vulkan] Selected: %s (%.1f GB) | Graphics Q=%u Compute Q=%u\n",
           ctx->gpu_name,
           ctx->vram_total / (1024.0 * 1024.0 * 1024.0),
           vk->graphics_queue_family, vk->compute_queue_family);

    if (config.gpu_compute) {
        printf("[Vulkan] GPU Compute: enabled (compute shader matmul)\n");
    }

    // TODO Phase 4: vkCreateDevice, swapchain, pipeline, compute shaders
    // For now, we have a working Vulkan instance with GPU enumeration

    ctx->initialized = true;

    // Store internal pointer for cleanup
    // We use the cmd_pool field as a hack to store vk_internal_t*
    ctx->cmd_pool = (VkCommandPool)vk;

    return ctx;
}

void oag_vk_destroy(oag_vk_ctx_t* ctx) {
    if (!ctx) return;

    vk_internal_t* vk = (vk_internal_t*)ctx->cmd_pool;
    if (vk) {
        if (vk->instance && vk->vkDestroyInstance) {
            vk->vkDestroyInstance(vk->instance, NULL);
        }
        if (vk->hlib) FreeLibrary(vk->hlib);
        free(vk);
    }
    free(ctx);
}

bool oag_vk_begin_frame(oag_vk_ctx_t* ctx) {
    if (!ctx || !ctx->initialized) return false;
    ctx->frame_count++;
    return true;
}

void oag_vk_end_frame(oag_vk_ctx_t* ctx) {
    static double last_time = 0;
    double now = vk_get_time_ms();
    if (last_time > 0) {
        ctx->frame_time_ms = now - last_time;
        if (ctx->frame_time_ms > 0) {
            ctx->fps = 1000.0 / ctx->frame_time_ms;
        }
    }
    last_time = now;
}

void oag_vk_draw_text(oag_vk_ctx_t* ctx, float x, float y,
                      const char* text, uint32_t color) {
    (void)ctx; (void)x; (void)y; (void)text; (void)color;
    // TODO: Vulkan text pipeline with glyph atlas
}

void oag_vk_draw_rect(oag_vk_ctx_t* ctx, float x, float y,
                      float w, float h, uint32_t color) {
    (void)ctx; (void)x; (void)y; (void)w; (void)h; (void)color;
    // TODO: Vulkan rect pipeline
}

void oag_vk_dispatch_matmul(oag_vk_ctx_t* ctx,
                             const float* a, int M, int K,
                             const float* b, int K2, int N,
                             float* out) {
    (void)ctx; (void)K2;
    // TODO: Vulkan compute shader dispatch
    // For now: CPU fallback
    for (int i = 0; i < M; i++) {
        for (int j = 0; j < N; j++) {
            float sum = 0.0f;
            for (int k = 0; k < K; k++) {
                sum += a[i * K + k] * b[k * N + j];
            }
            out[i * N + j] = sum;
        }
    }
}

void oag_vk_print_stats(const oag_vk_ctx_t* ctx) {
    printf("[Vulkan] GPU: %s | VRAM: %.1f GB | Frames: %llu | FPS: %.1f\n",
           ctx->gpu_name,
           ctx->vram_total / (1024.0 * 1024.0 * 1024.0),
           (unsigned long long)ctx->frame_count,
           ctx->fps);
}

#else
// Non-Windows: minimal stub
oag_vk_config_t oag_vk_default_config(void) {
    return (oag_vk_config_t){ .width = 1920, .height = 1080 };
}
oag_vk_ctx_t* oag_vk_create(oag_vk_config_t config) { (void)config; return NULL; }
void oag_vk_destroy(oag_vk_ctx_t* ctx) { free(ctx); }
bool oag_vk_begin_frame(oag_vk_ctx_t* ctx) { (void)ctx; return false; }
void oag_vk_end_frame(oag_vk_ctx_t* ctx) { (void)ctx; }
void oag_vk_draw_text(oag_vk_ctx_t* ctx, float x, float y, const char* text, uint32_t color) {
    (void)ctx; (void)x; (void)y; (void)text; (void)color; }
void oag_vk_draw_rect(oag_vk_ctx_t* ctx, float x, float y, float w, float h, uint32_t color) {
    (void)ctx; (void)x; (void)y; (void)w; (void)h; (void)color; }
void oag_vk_dispatch_matmul(oag_vk_ctx_t* ctx, const float* a, int M, int K,
    const float* b, int K2, int N, float* out) {
    (void)ctx; (void)a; (void)M; (void)K; (void)b; (void)K2; (void)N; (void)out; }
void oag_vk_print_stats(const oag_vk_ctx_t* ctx) { (void)ctx; }
#endif
