// Open_Agents — Backend Abstraction Layer
// All compute backends implement this interface
#ifndef OAG_BACKEND_H
#define OAG_BACKEND_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include "core/tensor.h"

typedef enum {
    OAG_BACKEND_CPU      = 0,
    OAG_BACKEND_CUDA     = 1,   // NVIDIA GPU (CUDA)
    OAG_BACKEND_DIRECTML = 2,   // AMD/Intel GPU (DirectML)
    OAG_BACKEND_NPU      = 3,   // Intel NPU via OpenVINO
    OAG_BACKEND_WASM     = 4,
    OAG_BACKEND_MOJO     = 5,
    OAG_BACKEND_JULIA    = 6,
    OAG_BACKEND_MLX      = 7,   // Apple Silicon only
    OAG_BACKEND_COUNT
} oag_backend_type_t;

// GPU vendor classification
typedef enum {
    OAG_GPU_VENDOR_UNKNOWN  = 0,
    OAG_GPU_VENDOR_NVIDIA   = 1,   // → CUDA backend
    OAG_GPU_VENDOR_AMD      = 2,   // → DirectML backend
    OAG_GPU_VENDOR_INTEL    = 3,   // → DirectML or NPU backend
} oag_gpu_vendor_t;

// GPU device info (from DXGI enumeration)
typedef struct {
    char             name[128];
    oag_gpu_vendor_t vendor;
    uint64_t         vram_bytes;
    uint32_t         vendor_id;      // PCI vendor ID
    uint32_t         device_id;      // PCI device ID
    uint32_t         adapter_idx;    // DXGI adapter index
    bool             is_discrete;    // discrete vs integrated
} oag_gpu_device_t;

#define OAG_MAX_GPUS 8

// GPU detection results
typedef struct {
    oag_gpu_device_t gpus[OAG_MAX_GPUS];
    int              n_gpus;
    int              best_gpu_idx;     // highest VRAM discrete GPU
    oag_gpu_vendor_t best_vendor;
    oag_backend_type_t recommended_backend;
} oag_gpu_detect_t;

// Detect all GPUs via DXGI and choose best backend
oag_gpu_detect_t oag_gpu_detect(void);
void             oag_gpu_detect_print(const oag_gpu_detect_t* det);

// Backend capability flags
typedef enum {
    OAG_CAP_F32_MATMUL   = 1 << 0,
    OAG_CAP_F16_MATMUL   = 1 << 1,
    OAG_CAP_Q4_MATMUL    = 1 << 2,
    OAG_CAP_Q8_MATMUL    = 1 << 3,
    OAG_CAP_SIMD_AVX2    = 1 << 4,
    OAG_CAP_SIMD_AVX512  = 1 << 5,
    OAG_CAP_SIMD_NEON    = 1 << 6,
    OAG_CAP_FLASH_ATTN   = 1 << 7,
    OAG_CAP_BATCHED      = 1 << 8,
} oag_cap_flags_t;

// Backend device info
typedef struct {
    char             name[128];
    oag_backend_type_t type;
    uint64_t         memory_total;    // bytes
    uint64_t         memory_free;     // bytes
    uint32_t         compute_units;   // cores / SMs / EUs
    uint32_t         capabilities;    // OAG_CAP_* flags
} oag_device_info_t;

// Backend vtable — each backend implements these
typedef struct oag_backend oag_backend_t;

typedef struct {
    const char* name;
    oag_backend_type_t type;

    // Lifecycle
    bool (*init)(oag_backend_t* be);
    void (*shutdown)(oag_backend_t* be);

    // Device info
    bool (*get_device_info)(oag_backend_t* be, oag_device_info_t* info);

    // Core operations (on device)
    void (*matmul)(oag_backend_t* be,
                   oag_tensor_t* dst,
                   const oag_tensor_t* a,
                   const oag_tensor_t* b);

    void (*matmul_q4)(oag_backend_t* be,
                      oag_tensor_t* dst,
                      const void* a_quant, ggml_type_t a_type, int64_t M, int64_t K,
                      const oag_tensor_t* b);

    void (*silu)(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src);
    void (*gelu)(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src);
    void (*rms_norm)(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src, float eps);
    void (*softmax)(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src);
    void (*rope)(oag_backend_t* be, oag_tensor_t* dst, const oag_tensor_t* src,
                 int n_head, int head_dim, int pos, float freq_base);

    // Flash Attention (optional)
    void (*flash_attn)(oag_backend_t* be,
                       oag_tensor_t* dst,
                       const oag_tensor_t* q,
                       const oag_tensor_t* k,
                       const oag_tensor_t* v,
                       float scale, bool causal);
} oag_backend_vtable_t;

struct oag_backend {
    const oag_backend_vtable_t* vt;
    void*                       ctx;  // backend-specific context
};

// Backend registry
bool             oag_backend_register(const oag_backend_vtable_t* vt);
oag_backend_t*   oag_backend_create(oag_backend_type_t type);
void             oag_backend_destroy(oag_backend_t* be);
oag_backend_t*   oag_backend_auto_select(void);  // pick best available
const char*      oag_backend_type_name(oag_backend_type_t type);

// CPU backend (always available)
extern const oag_backend_vtable_t oag_backend_cpu_vtable;

// GPU backends
extern const oag_backend_vtable_t oag_backend_cuda_vtable;
extern const oag_backend_vtable_t oag_backend_directml_vtable;
extern const oag_backend_vtable_t oag_backend_npu_vtable;
extern const oag_backend_vtable_t oag_backend_mojo_vtable;
extern const oag_backend_vtable_t oag_backend_julia_vtable;

#endif // OAG_BACKEND_H
