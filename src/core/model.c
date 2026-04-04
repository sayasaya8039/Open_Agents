// Open_Agents — Model loader + forward pass (Llama-style transformer)
#include "core/model.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

#ifdef _WIN32
#include <windows.h>
static double get_time_ms(void) {
    LARGE_INTEGER freq, cnt;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&cnt);
    return (double)cnt.QuadPart / (double)freq.QuadPart * 1000.0;
}
#else
#include <time.h>
static double get_time_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000.0 + ts.tv_nsec / 1e6;
}
#endif

// ============================================================
// KV Cache
// ============================================================

oag_kv_cache_t* oag_kv_cache_create(uint32_t n_layer, uint32_t n_ctx, uint32_t n_embd) {
    oag_kv_cache_t* cache = (oag_kv_cache_t*)calloc(1, sizeof(oag_kv_cache_t));
    cache->n_ctx = n_ctx;
    cache->pos   = 0;
    cache->k = (oag_tensor_t**)calloc(n_layer, sizeof(oag_tensor_t*));
    cache->v = (oag_tensor_t**)calloc(n_layer, sizeof(oag_tensor_t*));

    int64_t shape[2] = { n_ctx, n_embd };
    for (uint32_t l = 0; l < n_layer; l++) {
        cache->k[l] = oag_tensor_zeros(2, shape);
        cache->v[l] = oag_tensor_zeros(2, shape);
    }
    return cache;
}

void oag_kv_cache_free(oag_kv_cache_t* cache) {
    if (!cache) return;
    // n_layer not stored, but we'll free what's there
    // This is a simplification — in production, track n_layer
    free(cache->k);
    free(cache->v);
    free(cache);
}

void oag_kv_cache_reset(oag_kv_cache_t* cache) {
    cache->pos = 0;
}

// ============================================================
// Load tensor from GGUF (dequantize if needed)
// ============================================================

static oag_tensor_t* load_tensor_f32(const gguf_ctx_t* gguf, const char* name) {
    const gguf_tensor_info_t* info = gguf_find_tensor(gguf, name);
    if (!info) {
        fprintf(stderr, "[Model] Tensor not found: %s\n", name);
        return NULL;
    }

    // Find tensor index for data access
    for (uint64_t i = 0; i < gguf->n_tensors; i++) {
        if (strcmp(gguf->tensors[i].name.data, name) == 0) {
            const void* raw = gguf_tensor_data(gguf, i);
            int64_t shape[4];
            for (uint32_t d = 0; d < info->n_dims; d++) {
                shape[d] = (int64_t)info->dims[d];
            }
            return oag_tensor_dequant(raw, info->type, info->n_dims, shape);
        }
    }
    return NULL;
}

// ============================================================
// Model loading
// ============================================================

oag_model_t* oag_model_load(const char* path, oag_backend_t* backend) {
    printf("[Model] Loading: %s\n", path);
    double t0 = get_time_ms();

    gguf_ctx_t* gguf = gguf_load(path);
    if (!gguf) return NULL;

    oag_model_t* m = (oag_model_t*)calloc(1, sizeof(oag_model_t));
    m->gguf    = gguf;
    m->backend = backend;

    // Copy arch params
    m->arch           = gguf->arch ? strdup(gguf->arch) : strdup("llama");
    m->n_vocab        = gguf->n_vocab;
    m->n_embd         = gguf->n_embd;
    m->n_head         = gguf->n_head;
    m->n_head_kv      = gguf->n_head_kv;
    m->n_layer        = gguf->n_layer;
    m->n_ctx          = gguf->n_ctx;
    m->n_ff           = gguf->n_ff;
    m->head_dim       = m->n_embd / m->n_head;
    m->rope_freq_base = gguf->rope_freq_base;
    m->norm_eps       = gguf->norm_eps;

    printf("[Model] Arch: %s | %uL %uH %uKV | embd=%u ff=%u ctx=%u\n",
           m->arch, m->n_layer, m->n_head, m->n_head_kv,
           m->n_embd, m->n_ff, m->n_ctx);

    // Load tokenizer
    m->tokenizer = oag_tokenizer_load(gguf);

    // Load embedding + output weights
    m->tok_embd    = load_tensor_f32(gguf, "token_embd.weight");
    m->output_norm = load_tensor_f32(gguf, "output_norm.weight");
    m->output      = load_tensor_f32(gguf, "output.weight");

    // If output shares weights with embedding
    if (!m->output && m->tok_embd) {
        m->output = oag_tensor_clone(m->tok_embd);
        printf("[Model] Using tied embeddings for output\n");
    }

    // Load layers
    m->layers = (oag_layer_t*)calloc(m->n_layer, sizeof(oag_layer_t));
    for (uint32_t l = 0; l < m->n_layer; l++) {
        char name[256];

        snprintf(name, sizeof(name), "blk.%u.attn_norm.weight", l);
        m->layers[l].attn_norm = load_tensor_f32(gguf, name);

        snprintf(name, sizeof(name), "blk.%u.attn_q.weight", l);
        m->layers[l].wq = load_tensor_f32(gguf, name);

        snprintf(name, sizeof(name), "blk.%u.attn_k.weight", l);
        m->layers[l].wk = load_tensor_f32(gguf, name);

        snprintf(name, sizeof(name), "blk.%u.attn_v.weight", l);
        m->layers[l].wv = load_tensor_f32(gguf, name);

        snprintf(name, sizeof(name), "blk.%u.attn_output.weight", l);
        m->layers[l].wo = load_tensor_f32(gguf, name);

        snprintf(name, sizeof(name), "blk.%u.ffn_norm.weight", l);
        m->layers[l].ffn_norm = load_tensor_f32(gguf, name);

        snprintf(name, sizeof(name), "blk.%u.ffn_gate.weight", l);
        m->layers[l].w1 = load_tensor_f32(gguf, name);

        snprintf(name, sizeof(name), "blk.%u.ffn_down.weight", l);
        m->layers[l].w2 = load_tensor_f32(gguf, name);

        snprintf(name, sizeof(name), "blk.%u.ffn_up.weight", l);
        m->layers[l].w3 = load_tensor_f32(gguf, name);

        if (l == 0) {
            printf("[Model] Layer 0 loaded: attn_norm=%s wq=%s\n",
                   m->layers[l].attn_norm ? "OK" : "MISS",
                   m->layers[l].wq ? "OK" : "MISS");
        }
    }

    // Create KV cache
    m->kv_cache = oag_kv_cache_create(m->n_layer, m->n_ctx, m->n_embd);

    double t1 = get_time_ms();
    printf("[Model] Loaded in %.1f ms\n", t1 - t0);

    return m;
}

void oag_model_free(oag_model_t* model) {
    if (!model) return;

    oag_tensor_free(model->tok_embd);
    oag_tensor_free(model->output_norm);
    oag_tensor_free(model->output);

    if (model->layers) {
        for (uint32_t l = 0; l < model->n_layer; l++) {
            oag_tensor_free(model->layers[l].attn_norm);
            oag_tensor_free(model->layers[l].wq);
            oag_tensor_free(model->layers[l].wk);
            oag_tensor_free(model->layers[l].wv);
            oag_tensor_free(model->layers[l].wo);
            oag_tensor_free(model->layers[l].ffn_norm);
            oag_tensor_free(model->layers[l].w1);
            oag_tensor_free(model->layers[l].w2);
            oag_tensor_free(model->layers[l].w3);
        }
        free(model->layers);
    }

    oag_kv_cache_free(model->kv_cache);
    oag_tokenizer_free(model->tokenizer);
    gguf_free(model->gguf);
    free(model->arch);
    free(model);
}

// ============================================================
// Forward pass — single-token transformer decoder
// ============================================================

oag_tensor_t* oag_model_forward(oag_model_t* model,
                                 const int32_t* tokens,
                                 int32_t n_tokens) {
    uint32_t n_embd = model->n_embd;
    uint32_t n_head = model->n_head;
    uint32_t n_head_kv = model->n_head_kv;
    uint32_t head_dim = model->head_dim;
    uint32_t n_layer = model->n_layer;
    int32_t  kv_pos = model->kv_cache->pos;

    // Current hidden state: [n_tokens, n_embd]
    int64_t shape_h[2] = { n_tokens, n_embd };
    oag_tensor_t* hidden = oag_tensor_alloc(2, shape_h);

    // Lookup token embeddings
    for (int32_t t = 0; t < n_tokens; t++) {
        int32_t tid = tokens[t];
        if (tid >= 0 && (uint32_t)tid < model->n_vocab && model->tok_embd) {
            memcpy(hidden->data + t * n_embd,
                   model->tok_embd->data + tid * n_embd,
                   n_embd * sizeof(float));
        }
    }

    // Transformer layers
    int64_t shape_1d[1] = { n_embd };
    oag_tensor_t* norm_out = oag_tensor_alloc(1, shape_1d);
    oag_tensor_t* q_out    = oag_tensor_alloc(1, shape_1d);
    oag_tensor_t* k_out    = oag_tensor_alloc(1, shape_1d);
    oag_tensor_t* v_out    = oag_tensor_alloc(1, shape_1d);
    oag_tensor_t* attn_out = oag_tensor_alloc(1, shape_1d);

    for (uint32_t l = 0; l < n_layer; l++) {
        oag_layer_t* layer = &model->layers[l];

        for (int32_t t = 0; t < n_tokens; t++) {
            float* h = hidden->data + t * n_embd;

            // Create temp tensor view for this token
            oag_tensor_t h_view = { .data = h, .shape = { n_embd }, .n_dims = 1, .n_elem = n_embd, .owns_data = false };

            // 1. Attention norm
            if (layer->attn_norm) {
                oag_tensor_rms_norm(norm_out, &h_view, model->norm_eps);
                // Apply norm weight
                oag_tensor_mul(norm_out, norm_out, layer->attn_norm);
            } else {
                oag_tensor_copy(norm_out, &h_view);
            }

            // 2. Q/K/V projections (simplified: skip if weights missing)
            if (layer->wq && layer->wk && layer->wv) {
                // Q = norm_out @ Wq^T (simplified matmul for single vector)
                for (uint32_t i = 0; i < n_embd; i++) {
                    float sum = 0.0f;
                    for (uint32_t j = 0; j < n_embd; j++) {
                        sum += norm_out->data[j] * layer->wq->data[i * n_embd + j];
                    }
                    q_out->data[i] = sum;
                }

                // K, V (use n_head_kv * head_dim for GQA)
                uint32_t kv_dim = n_head_kv * head_dim;
                for (uint32_t i = 0; i < kv_dim; i++) {
                    float sum_k = 0.0f, sum_v = 0.0f;
                    for (uint32_t j = 0; j < n_embd; j++) {
                        sum_k += norm_out->data[j] * layer->wk->data[i * n_embd + j];
                        sum_v += norm_out->data[j] * layer->wv->data[i * n_embd + j];
                    }
                    k_out->data[i] = sum_k;
                    v_out->data[i] = sum_v;
                }

                // Apply RoPE to Q and K
                oag_tensor_rope(q_out, q_out, n_head, head_dim, kv_pos + t, model->rope_freq_base);
                oag_tensor_rope(k_out, k_out, n_head_kv, head_dim, kv_pos + t, model->rope_freq_base);

                // Store K/V in cache (simplified)
                // ... KV cache update would go here ...

                // Simplified attention: just use Q @ K^T for current position
                // Full implementation needs KV cache iteration
                // For now: attn_out = Wo @ Q (placeholder)
                for (uint32_t i = 0; i < n_embd; i++) {
                    float sum = 0.0f;
                    for (uint32_t j = 0; j < n_embd; j++) {
                        sum += q_out->data[j] * layer->wo->data[i * n_embd + j];
                    }
                    attn_out->data[i] = sum;
                }

                // Residual connection
                for (uint32_t i = 0; i < n_embd; i++) {
                    h[i] += attn_out->data[i];
                }
            }

            // 3. FFN
            if (layer->ffn_norm && layer->w1 && layer->w2 && layer->w3) {
                oag_tensor_rms_norm(norm_out, &h_view, model->norm_eps);
                oag_tensor_mul(norm_out, norm_out, layer->ffn_norm);

                // SwiGLU: out = W2 @ (silu(W1 @ x) * (W3 @ x))
                uint32_t ff = model->n_ff;
                float* gate = (float*)malloc(ff * sizeof(float));
                float* up   = (float*)malloc(ff * sizeof(float));

                for (uint32_t i = 0; i < ff; i++) {
                    float g = 0.0f, u = 0.0f;
                    for (uint32_t j = 0; j < n_embd; j++) {
                        g += norm_out->data[j] * layer->w1->data[i * n_embd + j];
                        u += norm_out->data[j] * layer->w3->data[i * n_embd + j];
                    }
                    // SiLU activation on gate
                    gate[i] = g / (1.0f + expf(-g));
                    gate[i] *= u;
                }

                // Down projection
                for (uint32_t i = 0; i < n_embd; i++) {
                    float sum = 0.0f;
                    for (uint32_t j = 0; j < ff; j++) {
                        sum += gate[j] * layer->w2->data[i * ff + j];
                    }
                    h[i] += sum;
                }

                free(gate);
                free(up);
            }
        }
    }

    // Final norm
    if (model->output_norm) {
        for (int32_t t = 0; t < n_tokens; t++) {
            oag_tensor_t h_view = {
                .data = hidden->data + t * n_embd,
                .shape = { n_embd }, .n_dims = 1, .n_elem = n_embd, .owns_data = false
            };
            oag_tensor_rms_norm(&h_view, &h_view, model->norm_eps);
            oag_tensor_mul(&h_view, &h_view, model->output_norm);
        }
    }

    // LM head: last token hidden state → logits [n_vocab]
    float* last_h = hidden->data + (n_tokens - 1) * n_embd;
    int64_t shape_logits[1] = { model->n_vocab };
    oag_tensor_t* logits = oag_tensor_alloc(1, shape_logits);

    if (model->output) {
        for (uint32_t i = 0; i < model->n_vocab; i++) {
            float sum = 0.0f;
            for (uint32_t j = 0; j < n_embd; j++) {
                sum += last_h[j] * model->output->data[i * n_embd + j];
            }
            logits->data[i] = sum;
        }
    }

    oag_tensor_free(norm_out);
    oag_tensor_free(q_out);
    oag_tensor_free(k_out);
    oag_tensor_free(v_out);
    oag_tensor_free(attn_out);
    oag_tensor_free(hidden);

    model->kv_cache->pos += n_tokens;
    return logits;
}

// ============================================================
// Text generation
// ============================================================

oag_gen_params_t oag_gen_default_params(void) {
    return (oag_gen_params_t){
        .sampler    = oag_sampler_default_params(),
        .max_tokens = 512,
        .stream     = false,
        .on_token   = NULL,
        .user_data  = NULL,
    };
}

int32_t* oag_model_generate(oag_model_t* model,
                             const int32_t* prompt_tokens,
                             int32_t n_prompt,
                             oag_gen_params_t params,
                             int32_t* out_n_generated) {
    oag_sampler_t* sampler = oag_sampler_create(params.sampler);
    int32_t max_gen = params.max_tokens;
    int32_t* output = (int32_t*)malloc((n_prompt + max_gen) * sizeof(int32_t));
    memcpy(output, prompt_tokens, n_prompt * sizeof(int32_t));

    // Reset KV cache
    oag_kv_cache_reset(model->kv_cache);

    // Prefill: process all prompt tokens
    printf("[Gen] Prefill %d tokens...\n", n_prompt);
    double t0 = get_time_ms();
    oag_tensor_t* logits = oag_model_forward(model, prompt_tokens, n_prompt);
    double t1 = get_time_ms();
    printf("[Gen] Prefill: %.1f ms (%.1f tok/s)\n", t1 - t0, n_prompt / ((t1 - t0) / 1000.0));

    int32_t n_gen = 0;
    double gen_start = get_time_ms();

    for (int32_t i = 0; i < max_gen; i++) {
        // Sample
        int32_t next = oag_sampler_sample(sampler, logits->data,
                                           (int32_t)model->n_vocab,
                                           output, n_prompt + n_gen);
        oag_tensor_free(logits);

        // Check EOS
        if (model->tokenizer && next == model->tokenizer->eos_id) break;

        output[n_prompt + n_gen] = next;
        n_gen++;

        // Stream callback
        if (params.stream && params.on_token && model->tokenizer) {
            const char* text = oag_tokenizer_decode(model->tokenizer, next);
            params.on_token(next, text, params.user_data);
        }

        // Forward single token
        logits = oag_model_forward(model, &next, 1);
    }

    double gen_end = get_time_ms();
    if (n_gen > 0) {
        printf("[Gen] Generated %d tokens in %.1f ms (%.1f tok/s)\n",
               n_gen, gen_end - gen_start,
               n_gen / ((gen_end - gen_start) / 1000.0));
    }

    if (logits) oag_tensor_free(logits);
    oag_sampler_free(sampler);
    *out_n_generated = n_gen;
    return output;
}
