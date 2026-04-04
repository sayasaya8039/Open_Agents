// Open_Agents — Tensor Operations
#ifndef OAG_TENSOR_H
#define OAG_TENSOR_H

#include <stdint.h>
#include <stddef.h>
#include "core/gguf.h"

// Tensor: contiguous multi-dimensional array
typedef struct {
    float*   data;
    int64_t  shape[4];
    int      n_dims;
    size_t   n_elem;     // total elements
    bool     owns_data;  // if true, free on destroy
} oag_tensor_t;

// Lifecycle
oag_tensor_t* oag_tensor_alloc(int n_dims, const int64_t* shape);
oag_tensor_t* oag_tensor_from_data(float* data, int n_dims, const int64_t* shape);
oag_tensor_t* oag_tensor_zeros(int n_dims, const int64_t* shape);
oag_tensor_t* oag_tensor_clone(const oag_tensor_t* src);
void          oag_tensor_free(oag_tensor_t* t);

// Dequantize GGUF tensor to float32
oag_tensor_t* oag_tensor_dequant(const void* quant_data, ggml_type_t type,
                                  int n_dims, const int64_t* shape);

// Elementwise ops
void oag_tensor_add(oag_tensor_t* dst, const oag_tensor_t* a, const oag_tensor_t* b);
void oag_tensor_mul(oag_tensor_t* dst, const oag_tensor_t* a, const oag_tensor_t* b);
void oag_tensor_scale(oag_tensor_t* dst, const oag_tensor_t* src, float s);

// Linear algebra
// C = A @ B  (matmul: [M,K] x [K,N] -> [M,N])
void oag_tensor_matmul(oag_tensor_t* dst, const oag_tensor_t* a, const oag_tensor_t* b);

// Activation functions
void oag_tensor_silu(oag_tensor_t* dst, const oag_tensor_t* src);
void oag_tensor_gelu(oag_tensor_t* dst, const oag_tensor_t* src);
void oag_tensor_relu(oag_tensor_t* dst, const oag_tensor_t* src);

// Normalization
void oag_tensor_rms_norm(oag_tensor_t* dst, const oag_tensor_t* src, float eps);
void oag_tensor_layer_norm(oag_tensor_t* dst, const oag_tensor_t* src,
                            const oag_tensor_t* weight, const oag_tensor_t* bias, float eps);

// Softmax
void oag_tensor_softmax(oag_tensor_t* dst, const oag_tensor_t* src);

// RoPE (Rotary Position Embedding)
void oag_tensor_rope(oag_tensor_t* dst, const oag_tensor_t* src,
                     int n_head, int head_dim, int pos, float freq_base);

// Utility
void oag_tensor_print(const oag_tensor_t* t, const char* label);
void oag_tensor_fill(oag_tensor_t* t, float val);
void oag_tensor_copy(oag_tensor_t* dst, const oag_tensor_t* src);

#endif // OAG_TENSOR_H
