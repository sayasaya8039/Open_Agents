// Open_Agents — CPU Backend (AVX2/FMA + Multi-threaded)
#include "backend/backend.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

#ifdef _MSC_VER
#include <intrin.h>
#else
#include <immintrin.h>
#endif

#ifdef _WIN32
#include <windows.h>
#else
#include <pthread.h>
#include <unistd.h>
#endif

// ============================================================
// CPU context
// ============================================================

typedef struct {
    int      n_threads;
    bool     has_avx2;
    bool     has_avx512;
    bool     has_fma;
    uint64_t total_ram;
} cpu_ctx_t;

static void detect_cpu_features(cpu_ctx_t* ctx) {
    #if defined(__x86_64__) || defined(_M_X64) || defined(__i386__) || defined(_M_IX86)
    int regs[4];
    #ifdef _MSC_VER
    __cpuid(regs, 7);
    #else
    __asm__ __volatile__(
        "cpuid" : "=a"(regs[0]), "=b"(regs[1]), "=c"(regs[2]), "=d"(regs[3])
        : "a"(7), "c"(0));
    #endif
    ctx->has_avx2   = (regs[1] >> 5) & 1;
    ctx->has_avx512 = (regs[1] >> 16) & 1;
    ctx->has_fma    = 1;  // assumed on modern CPUs
    #endif
}

// ============================================================
// Init / Shutdown
// ============================================================

static bool cpu_init(oag_backend_t* be) {
    cpu_ctx_t* ctx = (cpu_ctx_t*)calloc(1, sizeof(cpu_ctx_t));
    be->ctx = ctx;

    #ifdef _WIN32
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    ctx->n_threads = (int)si.dwNumberOfProcessors;

    MEMORYSTATUSEX mem;
    mem.dwLength = sizeof(mem);
    GlobalMemoryStatusEx(&mem);
    ctx->total_ram = mem.ullTotalPhys;
    #else
    ctx->n_threads = (int)sysconf(_SC_NPROCESSORS_ONLN);
    ctx->total_ram = 0;
    #endif

    detect_cpu_features(ctx);

    printf("[CPU] Threads: %d | AVX2: %s | AVX512: %s | FMA: %s | RAM: %.1f GB\n",
           ctx->n_threads,
           ctx->has_avx2 ? "YES" : "NO",
           ctx->has_avx512 ? "YES" : "NO",
           ctx->has_fma ? "YES" : "NO",
           ctx->total_ram / (1024.0 * 1024.0 * 1024.0));

    return true;
}

static void cpu_shutdown(oag_backend_t* be) {
    free(be->ctx);
    be->ctx = NULL;
}

// ============================================================
// Device info
// ============================================================

static bool cpu_get_device_info(oag_backend_t* be, oag_device_info_t* info) {
    cpu_ctx_t* ctx = (cpu_ctx_t*)be->ctx;
    snprintf(info->name, sizeof(info->name), "CPU (x86_64, %d threads)", ctx->n_threads);
    info->type          = OAG_BACKEND_CPU;
    info->memory_total  = ctx->total_ram;
    info->memory_free   = ctx->total_ram / 2;  // estimate
    info->compute_units = ctx->n_threads;

    info->capabilities = OAG_CAP_F32_MATMUL;
    if (ctx->has_avx2) info->capabilities |= OAG_CAP_SIMD_AVX2;
    if (ctx->has_avx512) info->capabilities |= OAG_CAP_SIMD_AVX512;

    return true;
}

// ============================================================
// Core operations
// ============================================================

static void cpu_matmul(oag_backend_t* be, oag_tensor_t* dst,
                       const oag_tensor_t* a, const oag_tensor_t* b) {
    (void)be;
    oag_tensor_matmul(dst, a, b);
}

static void cpu_matmul_q4(oag_backend_t* be, oag_tensor_t* dst,
                           const void* a_quant, ggml_type_t a_type,
                           int64_t M, int64_t K,
                           const oag_tensor_t* b) {
    (void)be;
    // Dequantize then matmul (for Q4 direct matmul, optimize later)
    int64_t shape_a[2] = { M, K };
    oag_tensor_t* a_f32 = oag_tensor_dequant(a_quant, a_type, 2, shape_a);
    oag_tensor_matmul(dst, a_f32, b);
    oag_tensor_free(a_f32);
}

static void cpu_silu(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be;
    oag_tensor_silu(dst, src);
}

static void cpu_gelu(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be;
    oag_tensor_gelu(dst, src);
}

static void cpu_rms_norm(oag_backend_t* be, oag_tensor_t* dst,
                          const oag_tensor_t* src, float eps) {
    (void)be;
    oag_tensor_rms_norm(dst, src, eps);
}

static void cpu_softmax(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be;
    oag_tensor_softmax(dst, src);
}

static void cpu_rope(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src,
                     int n_head, int head_dim, int pos, float freq_base) {
    (void)be;
    oag_tensor_rope(dst, src, n_head, head_dim, pos, freq_base);
}

// ============================================================
// Vtable
// ============================================================

const oag_backend_vtable_t oag_backend_cpu_vtable = {
    .name          = "CPU (AVX2+FMA)",
    .type          = OAG_BACKEND_CPU,
    .init          = cpu_init,
    .shutdown      = cpu_shutdown,
    .get_device_info = cpu_get_device_info,
    .matmul        = cpu_matmul,
    .matmul_q4     = cpu_matmul_q4,
    .silu          = cpu_silu,
    .gelu          = cpu_gelu,
    .rms_norm      = cpu_rms_norm,
    .softmax       = cpu_softmax,
    .rope          = cpu_rope,
    .flash_attn    = NULL,  // TODO: implement CPU flash attention
};

// ============================================================
// Backend registry
// ============================================================

static const oag_backend_vtable_t* registered_backends[OAG_BACKEND_COUNT] = { NULL };
static bool registry_initialized = false;

static void ensure_registry(void) {
    if (registry_initialized) return;
    registered_backends[OAG_BACKEND_CPU]      = &oag_backend_cpu_vtable;
    registered_backends[OAG_BACKEND_CUDA]     = &oag_backend_cuda_vtable;
    registered_backends[OAG_BACKEND_DIRECTML] = &oag_backend_directml_vtable;
    registered_backends[OAG_BACKEND_NPU]      = &oag_backend_npu_vtable;
    registry_initialized = true;
}

bool oag_backend_register(const oag_backend_vtable_t* vt) {
    ensure_registry();
    if (vt->type >= OAG_BACKEND_COUNT) return false;
    registered_backends[vt->type] = vt;
    return true;
}

oag_backend_t* oag_backend_create(oag_backend_type_t type) {
    ensure_registry();
    if (type >= OAG_BACKEND_COUNT || !registered_backends[type]) {
        return NULL;
    }

    oag_backend_t* be = (oag_backend_t*)calloc(1, sizeof(oag_backend_t));
    be->vt = registered_backends[type];
    be->ctx = NULL;

    if (!be->vt->init(be)) {
        free(be);
        return NULL;
    }

    return be;
}

void oag_backend_destroy(oag_backend_t* be) {
    if (!be) return;
    if (be->vt->shutdown) be->vt->shutdown(be);
    free(be);
}

oag_backend_t* oag_backend_auto_select(void) {
    ensure_registry();

    // GPU auto-detection via DXGI
    oag_gpu_detect_t det = oag_gpu_detect();
    oag_gpu_detect_print(&det);

    if (det.best_gpu_idx >= 0) {
        oag_backend_type_t recommended = det.recommended_backend;

        // Try recommended GPU backend
        oag_backend_t* gpu = oag_backend_create(recommended);
        if (gpu) {
            printf("[Backend] Auto-selected: %s for %s\n",
                   oag_backend_type_name(recommended),
                   det.gpus[det.best_gpu_idx].name);
            return gpu;
        }

        // If CUDA failed on NVIDIA, try DirectML as fallback
        if (recommended == OAG_BACKEND_CUDA) {
            printf("[Backend] CUDA init failed, trying DirectML fallback...\n");
            gpu = oag_backend_create(OAG_BACKEND_DIRECTML);
            if (gpu) return gpu;
        }
    }

    // Fallback: CPU
    printf("[Backend] Using CPU backend\n");
    return oag_backend_create(OAG_BACKEND_CPU);
}

const char* oag_backend_type_name(oag_backend_type_t type) {
    switch (type) {
        case OAG_BACKEND_CPU:      return "CPU";
        case OAG_BACKEND_CUDA:     return "CUDA";
        case OAG_BACKEND_DIRECTML: return "DirectML";
        case OAG_BACKEND_NPU:      return "NPU";
        case OAG_BACKEND_WASM:     return "WASM";
        case OAG_BACKEND_MOJO:     return "Mojo";
        case OAG_BACKEND_JULIA:    return "Julia";
        case OAG_BACKEND_MLX:      return "MLX";
        default:                   return "Unknown";
    }
}
