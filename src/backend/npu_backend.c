// Open_Agents — NPU Backend (Intel AI Boost via OpenVINO)
// Runtime-loads openvino_c.dll for Intel NPU/iGPU acceleration
// Intel Ultra 9 185H has built-in NPU (Intel AI Boost, 11 TOPS)
#include "backend/backend.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>

// ============================================================
// OpenVINO C API types (loaded dynamically)
// ============================================================

typedef int ov_status_e;
#define OV_STATUS_OK 0

typedef void* ov_core_t;
typedef void* ov_model_t;
typedef void* ov_compiled_model_t;
typedef void* ov_infer_request_t;
typedef void* ov_tensor_t;
typedef void* ov_output_port_t;
typedef void* ov_shape_t;
typedef void* ov_element_type_e;

// Function pointer types
typedef ov_status_e (*pfn_ov_core_create)(ov_core_t**);
typedef void        (*pfn_ov_core_free)(ov_core_t*);
typedef ov_status_e (*pfn_ov_core_get_available_devices)(ov_core_t*, char***);
typedef ov_status_e (*pfn_ov_core_read_model)(ov_core_t*, const char*, const char*, ov_model_t**);
typedef ov_status_e (*pfn_ov_core_compile_model)(ov_core_t*, ov_model_t*, const char*, int, ov_compiled_model_t**);
typedef ov_status_e (*pfn_ov_compiled_model_create_infer_request)(ov_compiled_model_t*, ov_infer_request_t**);
typedef ov_status_e (*pfn_ov_infer_request_infer)(ov_infer_request_t*);
typedef void        (*pfn_ov_model_free)(ov_model_t*);
typedef void        (*pfn_ov_compiled_model_free)(ov_compiled_model_t*);
typedef void        (*pfn_ov_infer_request_free)(ov_infer_request_t*);

// ============================================================
// NPU context
// ============================================================

typedef struct {
    HMODULE             hlib;           // openvino_c.dll
    ov_core_t*          core;
    bool                has_npu;        // NPU device available
    bool                has_gpu;        // Intel GPU available (Arc)
    char                device[32];     // "NPU" or "GPU" or "CPU"

    // Function pointers
    pfn_ov_core_create                  ov_core_create;
    pfn_ov_core_free                    ov_core_free;
    pfn_ov_core_get_available_devices   ov_core_get_available_devices;
    pfn_ov_core_read_model              ov_core_read_model;
    pfn_ov_core_compile_model           ov_core_compile_model;
    pfn_ov_compiled_model_create_infer_request ov_compiled_model_create_infer_request;
    pfn_ov_infer_request_infer          ov_infer_request_infer;
    pfn_ov_model_free                   ov_model_free;
    pfn_ov_compiled_model_free          ov_compiled_model_free;
    pfn_ov_infer_request_free           ov_infer_request_free;
} npu_ctx_t;

#define LOAD_OV(ctx, name)                                              \
    ctx->name = (pfn_##name)GetProcAddress(ctx->hlib, #name);           \
    if (!ctx->name) {                                                    \
        printf("[NPU] Missing symbol: %s\n", #name);                    \
    }

// ============================================================
// Init — Runtime load OpenVINO
// ============================================================

static bool npu_init(oag_backend_t* be) {
    npu_ctx_t* ctx = (npu_ctx_t*)calloc(1, sizeof(npu_ctx_t));
    be->ctx = ctx;

    // Try to load OpenVINO C API
    const char* ov_libs[] = {
        "openvino_c.dll",
        "C:\\Program Files (x86)\\Intel\\OpenVINO\\runtime\\bin\\intel64\\Release\\openvino_c.dll",
        "C:\\Program Files\\Intel\\OpenVINO\\runtime\\bin\\intel64\\Release\\openvino_c.dll",
        NULL
    };

    for (int i = 0; ov_libs[i]; i++) {
        ctx->hlib = LoadLibraryA(ov_libs[i]);
        if (ctx->hlib) {
            printf("[NPU] Loaded: %s\n", ov_libs[i]);
            break;
        }
    }

    if (!ctx->hlib) {
        printf("[NPU] OpenVINO not found. Intel NPU acceleration unavailable.\n");
        printf("[NPU] Install: https://docs.openvino.ai/2024/get-started/install-openvino.html\n");
        free(ctx);
        be->ctx = NULL;
        return false;
    }

    // Load symbols
    LOAD_OV(ctx, ov_core_create);
    LOAD_OV(ctx, ov_core_free);
    LOAD_OV(ctx, ov_core_get_available_devices);

    if (!ctx->ov_core_create) {
        FreeLibrary(ctx->hlib);
        free(ctx);
        be->ctx = NULL;
        return false;
    }

    // Create OV core
    ov_status_e st = ctx->ov_core_create(&ctx->core);
    if (st != OV_STATUS_OK) {
        printf("[NPU] ov_core_create failed: %d\n", st);
        FreeLibrary(ctx->hlib);
        free(ctx);
        be->ctx = NULL;
        return false;
    }

    // Enumerate devices
    char** devices = NULL;
    if (ctx->ov_core_get_available_devices) {
        ctx->ov_core_get_available_devices(ctx->core, &devices);
        if (devices) {
            printf("[NPU] Available OpenVINO devices:\n");
            for (int i = 0; devices[i]; i++) {
                printf("  - %s\n", devices[i]);
                if (strcmp(devices[i], "NPU") == 0) ctx->has_npu = true;
                if (strcmp(devices[i], "GPU") == 0) ctx->has_gpu = true;
            }
        }
    }

    // Select best device
    if (ctx->has_npu) {
        strcpy(ctx->device, "NPU");
        printf("[NPU] Intel AI Boost (NPU) detected — using NPU device\n");
    } else if (ctx->has_gpu) {
        strcpy(ctx->device, "GPU");
        printf("[NPU] Using Intel GPU via OpenVINO\n");
    } else {
        strcpy(ctx->device, "CPU");
        printf("[NPU] No NPU/GPU found, using OpenVINO CPU backend\n");
    }

    // Load remaining symbols
    LOAD_OV(ctx, ov_core_read_model);
    LOAD_OV(ctx, ov_core_compile_model);
    LOAD_OV(ctx, ov_compiled_model_create_infer_request);
    LOAD_OV(ctx, ov_infer_request_infer);
    LOAD_OV(ctx, ov_model_free);
    LOAD_OV(ctx, ov_compiled_model_free);
    LOAD_OV(ctx, ov_infer_request_free);

    return true;
}

static void npu_shutdown(oag_backend_t* be) {
    npu_ctx_t* ctx = (npu_ctx_t*)be->ctx;
    if (!ctx) return;

    if (ctx->core && ctx->ov_core_free) {
        ctx->ov_core_free(ctx->core);
    }
    if (ctx->hlib) FreeLibrary(ctx->hlib);
    free(ctx);
    be->ctx = NULL;
}

// ============================================================
// Device info
// ============================================================

static bool npu_get_device_info(oag_backend_t* be, oag_device_info_t* info) {
    npu_ctx_t* ctx = (npu_ctx_t*)be->ctx;
    if (!ctx) return false;

    snprintf(info->name, sizeof(info->name), "Intel NPU (%s via OpenVINO)", ctx->device);
    info->type          = OAG_BACKEND_NPU;
    info->memory_total  = 0;
    info->memory_free   = 0;
    info->compute_units = 0;
    info->capabilities  = OAG_CAP_F32_MATMUL | OAG_CAP_F16_MATMUL;
    return true;
}

// ============================================================
// Core operations — OpenVINO model-based
// For standalone tensor ops, fall back to CPU (OpenVINO is model-level API)
// NPU excels at running compiled ONNX models, not individual ops
// ============================================================

static void npu_matmul(oag_backend_t* be, oag_tensor_t* dst,
                       const oag_tensor_t* a, const oag_tensor_t* b) {
    (void)be;
    // OpenVINO is model-level, not op-level for NPU
    // Individual matmul dispatched to CPU AVX2
    oag_tensor_matmul(dst, a, b);
}

static void npu_matmul_q4(oag_backend_t* be, oag_tensor_t* dst,
                           const void* a_quant, ggml_type_t a_type,
                           int64_t M, int64_t K,
                           const oag_tensor_t* b) {
    (void)be;
    int64_t shape_a[2] = { M, K };
    oag_tensor_t* a_f32 = oag_tensor_dequant(a_quant, a_type, 2, shape_a);
    oag_tensor_matmul(dst, a_f32, b);
    oag_tensor_free(a_f32);
}

static void npu_silu(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be; oag_tensor_silu(dst, src);
}

static void npu_gelu(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be; oag_tensor_gelu(dst, src);
}

static void npu_rms_norm(oag_backend_t* be, oag_tensor_t* dst,
                          const oag_tensor_t* src, float eps) {
    (void)be; oag_tensor_rms_norm(dst, src, eps);
}

static void npu_softmax(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src) {
    (void)be; oag_tensor_softmax(dst, src);
}

static void npu_rope(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src,
                     int n_head, int head_dim, int pos, float freq_base) {
    (void)be; oag_tensor_rope(dst, src, n_head, head_dim, pos, freq_base);
}

// ============================================================
// Vtable
// ============================================================

const oag_backend_vtable_t oag_backend_npu_vtable = {
    .name          = "NPU (Intel AI Boost / OpenVINO)",
    .type          = OAG_BACKEND_NPU,
    .init          = npu_init,
    .shutdown      = npu_shutdown,
    .get_device_info = npu_get_device_info,
    .matmul        = npu_matmul,
    .matmul_q4     = npu_matmul_q4,
    .silu          = npu_silu,
    .gelu          = npu_gelu,
    .rms_norm      = npu_rms_norm,
    .softmax       = npu_softmax,
    .rope          = npu_rope,
    .flash_attn    = NULL,
};

#else
// Non-Windows stub
static bool npu_init_stub(oag_backend_t* be) { (void)be; return false; }
static void npu_shutdown_stub(oag_backend_t* be) { (void)be; }
const oag_backend_vtable_t oag_backend_npu_vtable = {
    .name = "NPU (unavailable)", .type = OAG_BACKEND_NPU,
    .init = npu_init_stub, .shutdown = npu_shutdown_stub,
};
#endif
