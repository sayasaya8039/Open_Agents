// Open_Agents — Tensor Operations (AVX2 + Multi-threaded)
#include "core/tensor.h"
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
#define OAG_NUM_THREADS() ({             \
    SYSTEM_INFO si;                      \
    GetSystemInfo(&si);                  \
    (int)si.dwNumberOfProcessors;        \
})
#else
#include <unistd.h>
#define OAG_NUM_THREADS() ((int)sysconf(_SC_NPROCESSORS_ONLN))
#endif

// ============================================================
// Lifecycle
// ============================================================

static size_t calc_n_elem(int n_dims, const int64_t* shape) {
    size_t n = 1;
    for (int i = 0; i < n_dims; i++) n *= (size_t)shape[i];
    return n;
}

oag_tensor_t* oag_tensor_alloc(int n_dims, const int64_t* shape) {
    oag_tensor_t* t = (oag_tensor_t*)calloc(1, sizeof(oag_tensor_t));
    t->n_dims = n_dims;
    t->n_elem = calc_n_elem(n_dims, shape);
    memcpy(t->shape, shape, n_dims * sizeof(int64_t));

    // AVX2 aligned allocation (32-byte boundary)
    #ifdef _WIN32
    t->data = (float*)_aligned_malloc(t->n_elem * sizeof(float), 32);
    #else
    posix_memalign((void**)&t->data, 32, t->n_elem * sizeof(float));
    #endif

    t->owns_data = true;
    return t;
}

oag_tensor_t* oag_tensor_from_data(float* data, int n_dims, const int64_t* shape) {
    oag_tensor_t* t = (oag_tensor_t*)calloc(1, sizeof(oag_tensor_t));
    t->n_dims = n_dims;
    t->n_elem = calc_n_elem(n_dims, shape);
    memcpy(t->shape, shape, n_dims * sizeof(int64_t));
    t->data = data;
    t->owns_data = false;
    return t;
}

oag_tensor_t* oag_tensor_zeros(int n_dims, const int64_t* shape) {
    oag_tensor_t* t = oag_tensor_alloc(n_dims, shape);
    memset(t->data, 0, t->n_elem * sizeof(float));
    return t;
}

oag_tensor_t* oag_tensor_clone(const oag_tensor_t* src) {
    oag_tensor_t* t = oag_tensor_alloc(src->n_dims, src->shape);
    memcpy(t->data, src->data, src->n_elem * sizeof(float));
    return t;
}

void oag_tensor_free(oag_tensor_t* t) {
    if (!t) return;
    if (t->owns_data && t->data) {
        #ifdef _WIN32
        _aligned_free(t->data);
        #else
        free(t->data);
        #endif
    }
    free(t);
}

// ============================================================
// Dequantization (Q4_0, Q4_K, Q8_0 → float32)
// ============================================================

static void dequant_q4_0(const uint8_t* src, float* dst, size_t n_elem) {
    // Q4_0 block: 2 bytes scale (f16) + 16 bytes quants = 18 bytes per 32 values
    size_t n_blocks = n_elem / 32;
    for (size_t b = 0; b < n_blocks; b++) {
        const uint8_t* block = src + b * 18;
        uint16_t f16_scale;
        memcpy(&f16_scale, block, 2);

        // f16 to f32 conversion
        uint32_t sign = (f16_scale >> 15) & 1;
        uint32_t exp  = (f16_scale >> 10) & 0x1f;
        uint32_t mant = f16_scale & 0x3ff;
        float scale;
        if (exp == 0) {
            scale = (sign ? -1.0f : 1.0f) * (mant / 1024.0f) * (1.0f / 16384.0f);
        } else if (exp == 31) {
            scale = 0.0f;
        } else {
            uint32_t f32 = (sign << 31) | ((exp + 112) << 23) | (mant << 13);
            memcpy(&scale, &f32, 4);
        }

        const uint8_t* qs = block + 2;
        for (int j = 0; j < 16; j++) {
            uint8_t byte = qs[j];
            dst[b * 32 + j]      = scale * ((int)(byte & 0xF) - 8);
            dst[b * 32 + j + 16] = scale * ((int)(byte >> 4) - 8);
        }
    }
}

static void dequant_q8_0(const uint8_t* src, float* dst, size_t n_elem) {
    // Q8_0 block: 4 bytes scale (f32) + 32 bytes quants = 36 bytes per 32 values
    // Actually: 2 bytes f16 scale + 32 bytes = 34 bytes
    size_t n_blocks = n_elem / 32;
    for (size_t b = 0; b < n_blocks; b++) {
        const uint8_t* block = src + b * 34;
        uint16_t f16_scale;
        memcpy(&f16_scale, block, 2);

        uint32_t sign = (f16_scale >> 15) & 1;
        uint32_t exp  = (f16_scale >> 10) & 0x1f;
        uint32_t mant = f16_scale & 0x3ff;
        float scale;
        if (exp == 0) {
            scale = (sign ? -1.0f : 1.0f) * (mant / 1024.0f) * (1.0f / 16384.0f);
        } else {
            uint32_t f32 = (sign << 31) | ((exp + 112) << 23) | (mant << 13);
            memcpy(&scale, &f32, 4);
        }

        const int8_t* qs = (const int8_t*)(block + 2);
        for (int j = 0; j < 32; j++) {
            dst[b * 32 + j] = scale * qs[j];
        }
    }
}

/// brain float (GGUF GGML_TYPE_BF16 = 30): uint16 を IEEE float の上位 16bit として解釈
static void dequant_bf16(const uint8_t* src, float* dst, size_t n_elem) {
    const uint16_t* bf = (const uint16_t*)src;
    for (size_t i = 0; i < n_elem; i++) {
        uint32_t u = (uint32_t)bf[i] << 16;
        memcpy(&dst[i], &u, 4);
    }
}

static void dequant_f16(const uint8_t* src, float* dst, size_t n_elem) {
    const uint16_t* f16 = (const uint16_t*)src;
    for (size_t i = 0; i < n_elem; i++) {
        uint32_t sign = (f16[i] >> 15) & 1;
        uint32_t exp  = (f16[i] >> 10) & 0x1f;
        uint32_t mant = f16[i] & 0x3ff;
        if (exp == 0) {
            dst[i] = (sign ? -1.0f : 1.0f) * (mant / 1024.0f) * (1.0f / 16384.0f);
        } else if (exp == 31) {
            dst[i] = 0.0f;
        } else {
            uint32_t f32 = (sign << 31) | ((exp + 112) << 23) | (mant << 13);
            memcpy(&dst[i], &f32, 4);
        }
    }
}

oag_tensor_t* oag_tensor_dequant(const void* quant_data, ggml_type_t type,
                                  int n_dims, const int64_t* shape) {
    oag_tensor_t* t = oag_tensor_alloc(n_dims, shape);
    const uint8_t* src = (const uint8_t*)quant_data;

    switch (type) {
        case GGML_TYPE_F32:
            memcpy(t->data, src, t->n_elem * sizeof(float));
            break;
        case GGML_TYPE_F16:
            dequant_f16(src, t->data, t->n_elem);
            break;
        case GGML_TYPE_Q4_0:
            dequant_q4_0(src, t->data, t->n_elem);
            break;
        case GGML_TYPE_Q8_0:
            dequant_q8_0(src, t->data, t->n_elem);
            break;
        case GGML_TYPE_BF16:
            dequant_bf16(src, t->data, t->n_elem);
            break;
        case GGML_TYPE_I8: {
            const int8_t* p = (const int8_t*)src;
            for (size_t i = 0; i < t->n_elem; i++) t->data[i] = (float)p[i];
            break;
        }
        case GGML_TYPE_I16: {
            const int16_t* p = (const int16_t*)src;
            for (size_t i = 0; i < t->n_elem; i++) t->data[i] = (float)p[i];
            break;
        }
        case GGML_TYPE_I32: {
            const int32_t* p = (const int32_t*)src;
            for (size_t i = 0; i < t->n_elem; i++) t->data[i] = (float)p[i];
            break;
        }
        case GGML_TYPE_I64: {
            const int64_t* p = (const int64_t*)src;
            for (size_t i = 0; i < t->n_elem; i++) t->data[i] = (float)p[i];
            break;
        }
        case GGML_TYPE_F64: {
            const double* p = (const double*)src;
            for (size_t i = 0; i < t->n_elem; i++) t->data[i] = (float)p[i];
            break;
        }
        default:
            fprintf(stderr, "[Tensor] Dequant not implemented for type: %s\n",
                    ggml_type_name(type));
            memset(t->data, 0, t->n_elem * sizeof(float));
            break;
    }
    return t;
}

// ============================================================
// Elementwise ops (AVX2 accelerated)
// ============================================================

void oag_tensor_add(oag_tensor_t* dst, const oag_tensor_t* a, const oag_tensor_t* b) {
    size_t n = a->n_elem;
    size_t i = 0;
    #ifdef __AVX2__
    for (; i + 8 <= n; i += 8) {
        __m256 va = _mm256_load_ps(a->data + i);
        __m256 vb = _mm256_load_ps(b->data + i);
        _mm256_store_ps(dst->data + i, _mm256_add_ps(va, vb));
    }
    #endif
    for (; i < n; i++) {
        dst->data[i] = a->data[i] + b->data[i];
    }
}

void oag_tensor_mul(oag_tensor_t* dst, const oag_tensor_t* a, const oag_tensor_t* b) {
    size_t n = a->n_elem;
    size_t i = 0;
    #ifdef __AVX2__
    for (; i + 8 <= n; i += 8) {
        __m256 va = _mm256_load_ps(a->data + i);
        __m256 vb = _mm256_load_ps(b->data + i);
        _mm256_store_ps(dst->data + i, _mm256_mul_ps(va, vb));
    }
    #endif
    for (; i < n; i++) {
        dst->data[i] = a->data[i] * b->data[i];
    }
}

void oag_tensor_scale(oag_tensor_t* dst, const oag_tensor_t* src, float s) {
    size_t n = src->n_elem;
    size_t i = 0;
    #ifdef __AVX2__
    __m256 vs = _mm256_set1_ps(s);
    for (; i + 8 <= n; i += 8) {
        __m256 v = _mm256_load_ps(src->data + i);
        _mm256_store_ps(dst->data + i, _mm256_mul_ps(v, vs));
    }
    #endif
    for (; i < n; i++) {
        dst->data[i] = src->data[i] * s;
    }
}

// ============================================================
// Matrix multiplication (AVX2 + FMA tiled)
// ============================================================

void oag_tensor_matmul(oag_tensor_t* dst, const oag_tensor_t* a, const oag_tensor_t* b) {
    // A: [M, K], B: [K, N] -> C: [M, N]
    int64_t M = a->shape[0];
    int64_t K = a->shape[1];
    int64_t N = b->shape[1];

    memset(dst->data, 0, M * N * sizeof(float));

    // Tiled matmul for cache efficiency
    const int TILE = 64;

    for (int64_t ii = 0; ii < M; ii += TILE) {
        for (int64_t jj = 0; jj < N; jj += TILE) {
            for (int64_t kk = 0; kk < K; kk += TILE) {
                int64_t i_end = (ii + TILE < M) ? ii + TILE : M;
                int64_t j_end = (jj + TILE < N) ? jj + TILE : N;
                int64_t k_end = (kk + TILE < K) ? kk + TILE : K;

                for (int64_t i = ii; i < i_end; i++) {
                    for (int64_t k = kk; k < k_end; k++) {
                        float a_ik = a->data[i * K + k];
                        int64_t j = jj;
                        #ifdef __AVX2__
                        __m256 va = _mm256_set1_ps(a_ik);
                        for (; j + 8 <= j_end; j += 8) {
                            __m256 vb = _mm256_loadu_ps(&b->data[k * N + j]);
                            __m256 vc = _mm256_loadu_ps(&dst->data[i * N + j]);
                            vc = _mm256_fmadd_ps(va, vb, vc);
                            _mm256_storeu_ps(&dst->data[i * N + j], vc);
                        }
                        #endif
                        for (; j < j_end; j++) {
                            dst->data[i * N + j] += a_ik * b->data[k * N + j];
                        }
                    }
                }
            }
        }
    }
}

// ============================================================
// Activation functions
// ============================================================

void oag_tensor_silu(oag_tensor_t* dst, const oag_tensor_t* src) {
    for (size_t i = 0; i < src->n_elem; i++) {
        float x = src->data[i];
        dst->data[i] = x / (1.0f + expf(-x));  // x * sigmoid(x)
    }
}

void oag_tensor_gelu(oag_tensor_t* dst, const oag_tensor_t* src) {
    const float SQRT_2_PI = 0.7978845608028654f;
    for (size_t i = 0; i < src->n_elem; i++) {
        float x = src->data[i];
        dst->data[i] = 0.5f * x * (1.0f + tanhf(SQRT_2_PI * (x + 0.044715f * x * x * x)));
    }
}

void oag_tensor_relu(oag_tensor_t* dst, const oag_tensor_t* src) {
    for (size_t i = 0; i < src->n_elem; i++) {
        dst->data[i] = src->data[i] > 0 ? src->data[i] : 0;
    }
}

// ============================================================
// Normalization
// ============================================================

void oag_tensor_rms_norm(oag_tensor_t* dst, const oag_tensor_t* src, float eps) {
    size_t n = src->n_elem;
    float sum_sq = 0.0f;

    #ifdef __AVX2__
    __m256 vsum = _mm256_setzero_ps();
    size_t i = 0;
    for (; i + 8 <= n; i += 8) {
        __m256 v = _mm256_load_ps(src->data + i);
        vsum = _mm256_fmadd_ps(v, v, vsum);
    }
    float tmp[8];
    _mm256_store_ps(tmp, vsum);
    for (int j = 0; j < 8; j++) sum_sq += tmp[j];
    for (; i < n; i++) sum_sq += src->data[i] * src->data[i];
    #else
    for (size_t i = 0; i < n; i++) sum_sq += src->data[i] * src->data[i];
    #endif

    float scale = 1.0f / sqrtf(sum_sq / n + eps);
    oag_tensor_scale(dst, src, scale);
}

void oag_tensor_layer_norm(oag_tensor_t* dst, const oag_tensor_t* src,
                            const oag_tensor_t* weight, const oag_tensor_t* bias, float eps) {
    size_t n = src->n_elem;
    float mean = 0.0f;
    for (size_t i = 0; i < n; i++) mean += src->data[i];
    mean /= n;

    float var = 0.0f;
    for (size_t i = 0; i < n; i++) {
        float d = src->data[i] - mean;
        var += d * d;
    }
    var /= n;

    float inv_std = 1.0f / sqrtf(var + eps);
    for (size_t i = 0; i < n; i++) {
        float norm = (src->data[i] - mean) * inv_std;
        dst->data[i] = norm * weight->data[i] + (bias ? bias->data[i] : 0.0f);
    }
}

// ============================================================
// Softmax
// ============================================================

void oag_tensor_softmax(oag_tensor_t* dst, const oag_tensor_t* src) {
    size_t n = src->n_elem;
    float max_val = src->data[0];
    for (size_t i = 1; i < n; i++) {
        if (src->data[i] > max_val) max_val = src->data[i];
    }
    float sum = 0.0f;
    for (size_t i = 0; i < n; i++) {
        dst->data[i] = expf(src->data[i] - max_val);
        sum += dst->data[i];
    }
    float inv_sum = 1.0f / sum;
    for (size_t i = 0; i < n; i++) {
        dst->data[i] *= inv_sum;
    }
}

// ============================================================
// RoPE
// ============================================================

void oag_tensor_rope(oag_tensor_t* dst, const oag_tensor_t* src,
                     int n_head, int head_dim, int pos, float freq_base) {
    for (int h = 0; h < n_head; h++) {
        float* head_data = src->data + h * head_dim;
        float* head_dst  = dst->data + h * head_dim;
        for (int d = 0; d < head_dim; d += 2) {
            float theta = pos * powf(freq_base, -(float)d / head_dim);
            float cos_t = cosf(theta);
            float sin_t = sinf(theta);
            float x0 = head_data[d];
            float x1 = head_data[d + 1];
            head_dst[d]     = x0 * cos_t - x1 * sin_t;
            head_dst[d + 1] = x0 * sin_t + x1 * cos_t;
        }
    }
}

// ============================================================
// Utility
// ============================================================

void oag_tensor_fill(oag_tensor_t* t, float val) {
    for (size_t i = 0; i < t->n_elem; i++) t->data[i] = val;
}

void oag_tensor_copy(oag_tensor_t* dst, const oag_tensor_t* src) {
    memcpy(dst->data, src->data, src->n_elem * sizeof(float));
}

void oag_tensor_print(const oag_tensor_t* t, const char* label) {
    printf("[%s] shape=[", label);
    for (int i = 0; i < t->n_dims; i++) {
        printf("%lld%s", (long long)t->shape[i], i < t->n_dims - 1 ? "," : "");
    }
    printf("] n_elem=%zu first=[", t->n_elem);
    size_t show = t->n_elem < 8 ? t->n_elem : 8;
    for (size_t i = 0; i < show; i++) {
        printf("%.4f%s", t->data[i], i < show - 1 ? ", " : "");
    }
    printf("...]\n");
}
