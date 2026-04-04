// Open_Agents — Model abstraction (Transformer decoder)
#ifndef OAG_MODEL_H
#define OAG_MODEL_H

#include "core/gguf.h"
#include "core/tensor.h"
#include "core/tokenizer.h"
#include "core/sampler.h"
#include "backend/backend.h"

// KV cache for autoregressive generation
typedef struct {
    oag_tensor_t** k;  // [n_layer] tensors of shape [n_ctx, n_embd]
    oag_tensor_t** v;  // [n_layer] tensors
    int32_t        pos; // current position
    int32_t        n_ctx;
    uint32_t       n_layer;
} oag_kv_cache_t;

// Layer weights (Llama-style transformer)
typedef struct {
    oag_tensor_t* attn_norm;     // RMSNorm weight
    oag_tensor_t* wq;            // Query projection
    oag_tensor_t* wk;            // Key projection
    oag_tensor_t* wv;            // Value projection
    oag_tensor_t* wo;            // Output projection
    oag_tensor_t* ffn_norm;      // FFN RMSNorm weight
    oag_tensor_t* w1;            // FFN gate
    oag_tensor_t* w2;            // FFN down
    oag_tensor_t* w3;            // FFN up

    // Quantized raw pointers (for Q4/Q8 direct matmul)
    const void*   wq_raw;
    const void*   wk_raw;
    const void*   wv_raw;
    const void*   wo_raw;
    const void*   w1_raw;
    const void*   w2_raw;
    const void*   w3_raw;
    ggml_type_t   weight_type;
} oag_layer_t;

// Full model
typedef struct {
    gguf_ctx_t*      gguf;
    oag_tokenizer_t* tokenizer;
    oag_backend_t*   backend;

    // Architecture params
    char*    arch;
    uint32_t n_vocab;
    uint32_t n_embd;
    uint32_t n_head;
    uint32_t n_head_kv;
    uint32_t n_layer;
    uint32_t n_ctx;
    uint32_t n_ff;
    uint32_t head_dim;
    float    rope_freq_base;
    float    norm_eps;

    // Weights
    oag_tensor_t* tok_embd;      // Token embeddings [n_vocab, n_embd]
    oag_layer_t*  layers;        // [n_layer]
    oag_tensor_t* output_norm;   // Final RMSNorm
    oag_tensor_t* output;        // LM head [n_vocab, n_embd]

    // KV cache
    oag_kv_cache_t* kv_cache;
} oag_model_t;

// Load model from GGUF file
oag_model_t* oag_model_load(const char* path, oag_backend_t* backend);
void         oag_model_free(oag_model_t* model);

// Forward pass: token IDs → logits
// tokens: input token IDs, n_tokens: count
// returns logits tensor [n_vocab] (caller frees)
oag_tensor_t* oag_model_forward(oag_model_t* model,
                                 const int32_t* tokens,
                                 int32_t n_tokens);

// Generate text
typedef struct {
    oag_sampler_params_t sampler;
    int32_t max_tokens;       // max tokens to generate
    bool    stream;           // call callback per token
    void    (*on_token)(int32_t token_id, const char* text, void* user);
    void*   user_data;
} oag_gen_params_t;

oag_gen_params_t oag_gen_default_params(void);

// Generate: returns array of generated token IDs
int32_t* oag_model_generate(oag_model_t* model,
                             const int32_t* prompt_tokens,
                             int32_t n_prompt,
                             oag_gen_params_t params,
                             int32_t* out_n_generated);

// KV cache management
oag_kv_cache_t* oag_kv_cache_create(uint32_t n_layer, uint32_t n_ctx, uint32_t n_embd);
void            oag_kv_cache_free(oag_kv_cache_t* cache);
void            oag_kv_cache_reset(oag_kv_cache_t* cache);

#endif // OAG_MODEL_H
