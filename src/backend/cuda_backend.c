// Open_Agents — CUDA Backend (NVIDIA GPU)
// Runtime-loads nvcuda.dll to avoid hard CUDA SDK dependency
#include "backend/backend.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>

// ============================================================
// CUDA Driver API types (loaded dynamically)
// ============================================================

typedef int CUresult;
typedef int CUdevice;
typedef void* CUcontext;
typedef void* CUmodule;
typedef void* CUfunction;
typedef void* CUdeviceptr;
typedef void* CUstream;

#define CUDA_SUCCESS 0

typedef CUresult (*pfn_cuInit)(unsigned int);
typedef CUresult (*pfn_cuDeviceGetCount)(int*);
typedef CUresult (*pfn_cuDeviceGet)(CUdevice*, int);
typedef CUresult (*pfn_cuDeviceGetName)(char*, int, CUdevice);
typedef CUresult (*pfn_cuDeviceTotalMem)(size_t*, CUdevice);
typedef CUresult (*pfn_cuDeviceGetAttribute)(int*, int, CUdevice);
typedef CUresult (*pfn_cuCtxCreate)(CUcontext*, unsigned int, CUdevice);
typedef CUresult (*pfn_cuCtxDestroy)(CUcontext);
typedef CUresult (*pfn_cuMemAlloc)(CUdeviceptr*, size_t);
typedef CUresult (*pfn_cuMemFree)(CUdeviceptr);
typedef CUresult (*pfn_cuMemcpyHtoD)(CUdeviceptr, const void*, size_t);
typedef CUresult (*pfn_cuMemcpyDtoH)(void*, CUdeviceptr, size_t);

// Compute capability attribute IDs
#define CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR 75
#define CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR 76
#define CU_DEVICE_ATTRIBUTE_MULTIPROCESSOR_COUNT     16
#define CU_DEVICE_ATTRIBUTE_MAX_THREADS_PER_BLOCK    1

// ============================================================
// CUDA context (dynamically loaded)
// ============================================================

typedef struct {
    HMODULE           hlib;          // nvcuda.dll handle
    CUcontext         context;
    CUdevice          device;
    char              device_name[128];
    size_t            total_mem;
    int               sm_count;
    int               compute_major;
    int               compute_minor;

    // Function pointers
    pfn_cuInit              cuInit;
    pfn_cuDeviceGetCount    cuDeviceGetCount;
    pfn_cuDeviceGet         cuDeviceGet;
    pfn_cuDeviceGetName     cuDeviceGetName;
    pfn_cuDeviceTotalMem    cuDeviceTotalMem;
    pfn_cuDeviceGetAttribute cuDeviceGetAttribute;
    pfn_cuCtxCreate         cuCtxCreate;
    pfn_cuCtxDestroy        cuCtxDestroy;
    pfn_cuMemAlloc          cuMemAlloc;
    pfn_cuMemFree           cuMemFree;
    pfn_cuMemcpyHtoD        cuMemcpyHtoD;
    pfn_cuMemcpyDtoH        cuMemcpyDtoH;
} cuda_ctx_t;

#define LOAD_FN(ctx, name)                                       \
    ctx->name = (pfn_##name)GetProcAddress(ctx->hlib, #name);    \
    if (!ctx->name) {                                            \
        printf("[CUDA] Missing: %s\n", #name);                   \
        return false;                                             \
    }

// ============================================================
// Init — runtime load nvcuda.dll
// ============================================================

static bool cuda_init(oag_backend_t* be) {
    cuda_ctx_t* ctx = (cuda_ctx_t*)calloc(1, sizeof(cuda_ctx_t));
    be->ctx = ctx;

    // Try to load CUDA driver
    ctx->hlib = LoadLibraryA("nvcuda.dll");
    if (!ctx->hlib) {
        printf("[CUDA] nvcuda.dll not found — NVIDIA driver not installed\n");
        free(ctx);
        be->ctx = NULL;
        return false;
    }

    // Load function pointers
    LOAD_FN(ctx, cuInit);
    LOAD_FN(ctx, cuDeviceGetCount);
    LOAD_FN(ctx, cuDeviceGet);
    LOAD_FN(ctx, cuDeviceGetName);
    LOAD_FN(ctx, cuDeviceTotalMem);
    LOAD_FN(ctx, cuDeviceGetAttribute);
    LOAD_FN(ctx, cuCtxCreate);
    LOAD_FN(ctx, cuCtxDestroy);
    LOAD_FN(ctx, cuMemAlloc);
    LOAD_FN(ctx, cuMemFree);
    LOAD_FN(ctx, cuMemcpyHtoD);
    LOAD_FN(ctx, cuMemcpyDtoH);

    // Initialize CUDA
    CUresult err = ctx->cuInit(0);
    if (err != CUDA_SUCCESS) {
        printf("[CUDA] cuInit failed: %d\n", err);
        FreeLibrary(ctx->hlib);
        free(ctx);
        be->ctx = NULL;
        return false;
    }

    // Get device count
    int device_count = 0;
    ctx->cuDeviceGetCount(&device_count);
    if (device_count == 0) {
        printf("[CUDA] No CUDA devices found\n");
        FreeLibrary(ctx->hlib);
        free(ctx);
        be->ctx = NULL;
        return false;
    }

    // Use first device
    ctx->cuDeviceGet(&ctx->device, 0);
    ctx->cuDeviceGetName(ctx->device_name, sizeof(ctx->device_name), ctx->device);
    ctx->cuDeviceTotalMem(&ctx->total_mem, ctx->device);

    ctx->cuDeviceGetAttribute(&ctx->compute_major,
                               CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR,
                               ctx->device);
    ctx->cuDeviceGetAttribute(&ctx->compute_minor,
                               CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR,
                               ctx->device);
    ctx->cuDeviceGetAttribute(&ctx->sm_count,
                               CU_DEVICE_ATTRIBUTE_MULTIPROCESSOR_COUNT,
                               ctx->device);

    // Create context
    err = ctx->cuCtxCreate(&ctx->context, 0, ctx->device);
    if (err != CUDA_SUCCESS) {
        printf("[CUDA] cuCtxCreate failed: %d\n", err);
        FreeLibrary(ctx->hlib);
        free(ctx);
        be->ctx = NULL;
        return false;
    }

    printf("[CUDA] %s | SM %d.%d | %d SMs | %.1f GB VRAM\n",
           ctx->device_name,
           ctx->compute_major, ctx->compute_minor,
           ctx->sm_count,
           ctx->total_mem / (1024.0 * 1024.0 * 1024.0));

    return true;
}

static void cuda_shutdown(oag_backend_t* be) {
    cuda_ctx_t* ctx = (cuda_ctx_t*)be->ctx;
    if (!ctx) return;

    if (ctx->context) {
        ctx->cuCtxDestroy(ctx->context);
    }
    if (ctx->hlib) {
        FreeLibrary(ctx->hlib);
    }
    free(ctx);
    be->ctx = NULL;
}

// ============================================================
// Device info
// ============================================================

static bool cuda_get_device_info(oag_backend_t* be, oag_device_info_t* info) {
    cuda_ctx_t* ctx = (cuda_ctx_t*)be->ctx;
    if (!ctx) return false;

    snprintf(info->name, sizeof(info->name), "%s (CUDA SM%d.%d)",
             ctx->device_name, ctx->compute_major, ctx->compute_minor);
    info->type          = OAG_BACKEND_CUDA;
    info->memory_total  = ctx->total_mem;
    info->memory_free   = ctx->total_mem;  // TODO: query actual free
    info->compute_units = ctx->sm_count;
    info->capabilities  = OAG_CAP_F32_MATMUL | OAG_CAP_F16_MATMUL |
                          OAG_CAP_Q4_MATMUL | OAG_CAP_FLASH_ATTN;
    return true;
}

// ============================================================
// Core operations (CUDA kernel dispatch)
// For now: upload → CPU compute → download
// Phase 2: actual CUDA kernels via cuModuleLoad
// ============================================================

static void cuda_matmul(oag_backend_t* be, oag_tensor_t* dst,
                        const oag_tensor_t* a, const oag_tensor_t* b) {
    (void)be;
    // TODO: Replace with cuBLAS sgemm via runtime loading
    // For now: CPU fallback
    oag_tensor_matmul(dst, a, b);
}

static void cuda_matmul_q4(oag_backend_t* be, oag_tensor_t* dst,
                            const void* a_quant, ggml_type_t a_type,
                            int64_t M, int64_t K,
                            const oag_tensor_t* b) {
    (void)be;
    int64_t shape_a[2] = { M, K };
    oag_tensor_t* a_f32 = oag_tensor_dequant(a_quant, a_type, 2, shape_a);
    oag_tensor_matmul(dst, a_f32, b);
    oag_tensor_free(a_f32);
}

static void cuda_silu(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be;
    oag_tensor_silu(dst, src);
}

static void cuda_gelu(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be;
    oag_tensor_gelu(dst, src);
}

static void cuda_rms_norm(oag_backend_t* be, oag_tensor_t* dst,
                           const oag_tensor_t* src, float eps) {
    (void)be;
    oag_tensor_rms_norm(dst, src, eps);
}

static void cuda_softmax(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be;
    oag_tensor_softmax(dst, src);
}

static void cuda_rope(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src,
                      int n_head, int head_dim, int pos, float freq_base) {
    (void)be;
    oag_tensor_rope(dst, src, n_head, head_dim, pos, freq_base);
}

// ============================================================
// Vtable
// ============================================================

const oag_backend_vtable_t oag_backend_cuda_vtable = {
    .name          = "CUDA (NVIDIA GPU)",
    .type          = OAG_BACKEND_CUDA,
    .init          = cuda_init,
    .shutdown      = cuda_shutdown,
    .get_device_info = cuda_get_device_info,
    .matmul        = cuda_matmul,
    .matmul_q4     = cuda_matmul_q4,
    .silu          = cuda_silu,
    .gelu          = cuda_gelu,
    .rms_norm      = cuda_rms_norm,
    .softmax       = cuda_softmax,
    .rope          = cuda_rope,
    .flash_attn    = NULL,  // TODO: FlashAttention-2 CUDA kernel
};

#else
// Non-Windows stub
static bool cuda_init_stub(oag_backend_t* be) { (void)be; return false; }
static void cuda_shutdown_stub(oag_backend_t* be) { (void)be; }
const oag_backend_vtable_t oag_backend_cuda_vtable = {
    .name = "CUDA (unavailable)", .type = OAG_BACKEND_CUDA,
    .init = cuda_init_stub, .shutdown = cuda_shutdown_stub,
};
#endif
