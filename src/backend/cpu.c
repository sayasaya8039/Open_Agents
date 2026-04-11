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
// Multi-threaded matmul (row-block parallel + AVX2/FMA tiled)
// ============================================================

#define MATMUL_TILE 64
#define MATMUL_MT_THRESHOLD 4096  // M*K*N >= threshold でマルチスレッド化

typedef struct {
    const float* A;
    const float* B;
    float*       C;
    int64_t      M, K, N;
    int64_t      row_start;
    int64_t      row_end;
} matmul_arg_t;

static void matmul_tile_rows(const float* A, const float* B, float* C,
                             int64_t row_start, int64_t row_end,
                             int64_t K, int64_t N) {
    // 担当行を0初期化
    memset(C + row_start * N, 0, (size_t)(row_end - row_start) * N * sizeof(float));

    for (int64_t ii = row_start; ii < row_end; ii += MATMUL_TILE) {
        for (int64_t jj = 0; jj < N; jj += MATMUL_TILE) {
            for (int64_t kk = 0; kk < K; kk += MATMUL_TILE) {
                int64_t i_end = (ii + MATMUL_TILE < row_end) ? ii + MATMUL_TILE : row_end;
                int64_t j_end = (jj + MATMUL_TILE < N) ? jj + MATMUL_TILE : N;
                int64_t k_end = (kk + MATMUL_TILE < K) ? kk + MATMUL_TILE : K;

                for (int64_t i = ii; i < i_end; i++) {
                    for (int64_t k = kk; k < k_end; k++) {
                        float a_ik = A[i * K + k];
                        int64_t j = jj;
#ifdef __AVX2__
                        __m256 va = _mm256_set1_ps(a_ik);
                        for (; j + 8 <= j_end; j += 8) {
                            __m256 vb = _mm256_loadu_ps(&B[k * N + j]);
                            __m256 vc = _mm256_loadu_ps(&C[i * N + j]);
                            vc = _mm256_fmadd_ps(va, vb, vc);
                            _mm256_storeu_ps(&C[i * N + j], vc);
                        }
#endif
                        for (; j < j_end; j++) {
                            C[i * N + j] += a_ik * B[k * N + j];
                        }
                    }
                }
            }
        }
    }
}

#ifdef _WIN32
static DWORD WINAPI matmul_thread_fn(LPVOID param) {
    matmul_arg_t* arg = (matmul_arg_t*)param;
    matmul_tile_rows(arg->A, arg->B, arg->C,
                     arg->row_start, arg->row_end, arg->K, arg->N);
    return 0;
}
#endif

// ============================================================
// Core operations
// ============================================================

static void cpu_matmul(oag_backend_t* be, oag_tensor_t* dst,
                       const oag_tensor_t* a, const oag_tensor_t* b) {
    cpu_ctx_t* ctx = (cpu_ctx_t*)be->ctx;
    int64_t M = a->shape[0];
    int64_t K = a->shape[1];
    int64_t N = b->shape[1];

    // 小さい行列はシングルスレッドで処理
    int n_threads = (ctx && ctx->n_threads > 1) ? ctx->n_threads : 1;
    if (n_threads > 16) n_threads = 16;  // 上限キャップ
    if (M < n_threads) n_threads = (int)M;
    if ((int64_t)M * K * N < MATMUL_MT_THRESHOLD || n_threads <= 1) {
        oag_tensor_matmul(dst, a, b);
        return;
    }

#ifdef _WIN32
    // マルチスレッド matmul: M 行を n_threads ブロックに分割
    int64_t rows_per = M / n_threads;
    int64_t extra    = M % n_threads;

    // スレッド数分の引数 + ハンドル（スタック確保、ヒープ不要）
    matmul_arg_t args[16];
    HANDLE       handles[16];
    int          spawned = 0;

    int64_t row = 0;
    for (int t = 0; t < n_threads; t++) {
        args[t].A = a->data;
        args[t].B = b->data;
        args[t].C = dst->data;
        args[t].M = M;
        args[t].K = K;
        args[t].N = N;
        args[t].row_start = row;
        args[t].row_end   = row + rows_per + (t < extra ? 1 : 0);
        row = args[t].row_end;

        if (t == n_threads - 1) {
            // 最後のブロックは現スレッドで実行（スレッド生成コスト削減）
            matmul_tile_rows(a->data, b->data, dst->data,
                             args[t].row_start, args[t].row_end, K, N);
        } else {
            handles[spawned] = CreateThread(NULL, 0, matmul_thread_fn,
                                            &args[t], 0, NULL);
            if (handles[spawned]) {
                spawned++;
            } else {
                // スレッド生成失敗: 現スレッドで処理
                matmul_tile_rows(a->data, b->data, dst->data,
                                 args[t].row_start, args[t].row_end, K, N);
            }
        }
    }

    // 全ワーカースレッドの完了を待機
    if (spawned > 0) {
        WaitForMultipleObjects(spawned, handles, TRUE, INFINITE);
        for (int i = 0; i < spawned; i++) {
            CloseHandle(handles[i]);
        }
    }
#else
    // 非 Windows: シングルスレッドフォールバック
    oag_tensor_matmul(dst, a, b);
#endif
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
