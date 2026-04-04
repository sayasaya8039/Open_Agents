// Open_Agents — DirectML Backend (AMD / Intel GPU)
// Runtime-loads DirectML.dll for AMD Radeon and Intel Arc GPUs
// DirectML is Microsoft's hardware-accelerated ML operator library
// Works on any DirectX 12 capable GPU (AMD, Intel, NVIDIA fallback)
#include "backend/backend.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>

// We runtime-load DirectML to avoid SDK dependency at build time.
// DirectML ships with Windows 10 1903+ and can also be installed via NuGet.

// ============================================================
// DirectML types (minimal, from DirectML.h)
// ============================================================

// D3D12 forward declares
typedef void* ID3D12Device;
typedef void* ID3D12CommandQueue;
typedef void* ID3D12Resource;
typedef void* IDMLDevice;
typedef void* IDMLCommandRecorder;
typedef void* IDMLBindingTable;
typedef void* IDMLCompiledOperator;

typedef HRESULT (*pfn_DMLCreateDevice)(
    ID3D12Device* d3d12_device,
    unsigned int flags,
    void* riid,
    void** dml_device
);

// ============================================================
// DirectML context
// ============================================================

typedef struct {
    HMODULE        hlib_dml;      // DirectML.dll
    HMODULE        hlib_d3d12;    // d3d12.dll
    HMODULE        hlib_dxgi;     // dxgi.dll

    // Device info (from DXGI detection)
    char           gpu_name[128];
    uint64_t       vram_bytes;
    uint32_t       vendor_id;

    // D3D12 objects (would be actual COM pointers in full implementation)
    void*          d3d12_device;
    void*          dml_device;

    bool           initialized;
} dml_ctx_t;

// ============================================================
// Init — Runtime load DirectML + D3D12
// ============================================================

static bool dml_init(oag_backend_t* be) {
    dml_ctx_t* ctx = (dml_ctx_t*)calloc(1, sizeof(dml_ctx_t));
    be->ctx = ctx;

    // Load DirectML.dll
    ctx->hlib_dml = LoadLibraryA("DirectML.dll");
    if (!ctx->hlib_dml) {
        // Try Windows ML redistribution path
        ctx->hlib_dml = LoadLibraryA("directml.dll");
    }
    if (!ctx->hlib_dml) {
        printf("[DirectML] DirectML.dll not found\n");
        printf("[DirectML] Install: winget install Microsoft.DirectML\n");
        printf("[DirectML] Or download from github.com/microsoft/DirectML\n");
        free(ctx);
        be->ctx = NULL;
        return false;
    }

    // Load D3D12
    ctx->hlib_d3d12 = LoadLibraryA("d3d12.dll");
    if (!ctx->hlib_d3d12) {
        printf("[DirectML] d3d12.dll not found — DirectX 12 required\n");
        FreeLibrary(ctx->hlib_dml);
        free(ctx);
        be->ctx = NULL;
        return false;
    }

    printf("[DirectML] Libraries loaded: DirectML.dll + d3d12.dll\n");

    // In full implementation:
    // 1. Create D3D12 device on the target adapter
    // 2. DMLCreateDevice(d3d12_device, DML_CREATE_DEVICE_FLAG_NONE, ...)
    // 3. Create command queue, allocator, etc.
    // 4. Compile DML operators (MatMul, Add, Relu, etc.)

    // For now, mark as initialized with info from GPU detection
    ctx->initialized = true;

    return true;
}

static void dml_shutdown(oag_backend_t* be) {
    dml_ctx_t* ctx = (dml_ctx_t*)be->ctx;
    if (!ctx) return;

    // TODO: Release D3D12 + DML COM objects

    if (ctx->hlib_dml)   FreeLibrary(ctx->hlib_dml);
    if (ctx->hlib_d3d12) FreeLibrary(ctx->hlib_d3d12);
    free(ctx);
    be->ctx = NULL;
}

// ============================================================
// Device info
// ============================================================

static bool dml_get_device_info(oag_backend_t* be, oag_device_info_t* info) {
    dml_ctx_t* ctx = (dml_ctx_t*)be->ctx;
    if (!ctx) return false;

    snprintf(info->name, sizeof(info->name), "%s (DirectML)",
             ctx->gpu_name[0] ? ctx->gpu_name : "AMD/Intel GPU");
    info->type          = OAG_BACKEND_DIRECTML;
    info->memory_total  = ctx->vram_bytes;
    info->memory_free   = ctx->vram_bytes;  // TODO: query actual
    info->compute_units = 0;  // TODO: query from D3D12
    info->capabilities  = OAG_CAP_F32_MATMUL | OAG_CAP_F16_MATMUL;
    return true;
}

// ============================================================
// Core operations (DirectML operator dispatch)
// Full impl would compile DML operators and dispatch via D3D12 command lists.
// For now: CPU fallback.
// ============================================================

static void dml_matmul(oag_backend_t* be, oag_tensor_t* dst,
                       const oag_tensor_t* a, const oag_tensor_t* b) {
    (void)be;
    // TODO: DMLCreateOperator(DML_OPERATOR_GEMM, ...) → compile → dispatch
    oag_tensor_matmul(dst, a, b);
}

static void dml_matmul_q4(oag_backend_t* be, oag_tensor_t* dst,
                           const void* a_quant, ggml_type_t a_type,
                           int64_t M, int64_t K,
                           const oag_tensor_t* b) {
    (void)be;
    int64_t shape_a[2] = { M, K };
    oag_tensor_t* a_f32 = oag_tensor_dequant(a_quant, a_type, 2, shape_a);
    oag_tensor_matmul(dst, a_f32, b);
    oag_tensor_free(a_f32);
}

static void dml_silu(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be;
    // TODO: DML_OPERATOR_ACTIVATION_SIGMOID + element_mul
    oag_tensor_silu(dst, src);
}

static void dml_gelu(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be;
    oag_tensor_gelu(dst, src);
}

static void dml_rms_norm(oag_backend_t* be, oag_tensor_t* dst,
                          const oag_tensor_t* src, float eps) {
    (void)be;
    // TODO: DML_OPERATOR_MEAN_VARIANCE_NORMALIZATION
    oag_tensor_rms_norm(dst, src, eps);
}

static void dml_softmax(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be;
    // TODO: DML_OPERATOR_ACTIVATION_SOFTMAX
    oag_tensor_softmax(dst, src);
}

static void dml_rope(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src,
                     int n_head, int head_dim, int pos, float freq_base) {
    (void)be;
    oag_tensor_rope(dst, src, n_head, head_dim, pos, freq_base);
}

// ============================================================
// Vtable
// ============================================================

const oag_backend_vtable_t oag_backend_directml_vtable = {
    .name          = "DirectML (AMD/Intel GPU)",
    .type          = OAG_BACKEND_DIRECTML,
    .init          = dml_init,
    .shutdown      = dml_shutdown,
    .get_device_info = dml_get_device_info,
    .matmul        = dml_matmul,
    .matmul_q4     = dml_matmul_q4,
    .silu          = dml_silu,
    .gelu          = dml_gelu,
    .rms_norm      = dml_rms_norm,
    .softmax       = dml_softmax,
    .rope          = dml_rope,
    .flash_attn    = NULL,
};

#else
// Non-Windows stub
static bool dml_init_stub(oag_backend_t* be) { (void)be; return false; }
static void dml_shutdown_stub(oag_backend_t* be) { (void)be; }
const oag_backend_vtable_t oag_backend_directml_vtable = {
    .name = "DirectML (unavailable)", .type = OAG_BACKEND_DIRECTML,
    .init = dml_init_stub, .shutdown = dml_shutdown_stub,
};
#endif
