// Open_Agents — Inference Pipeline
// Coordinates model loading, backend selection, and generation
#ifndef OAG_INFERENCE_H
#define OAG_INFERENCE_H

#include "core/model.h"
#include "backend/backend.h"

// Multi-device scheduling: NPU + CPU + GPU parallel
typedef struct {
    oag_backend_t* gpu;    // CUDA/Vulkan — attention + matmul
    oag_backend_t* cpu;    // Multi-CPU — fallback + embedding
    oag_backend_t* npu;    // Intel NPU — prefill acceleration
    bool           use_gpu;
    bool           use_npu;
    bool           use_multi_device;
} oag_device_scheduler_t;

// Inference engine
typedef struct {
    oag_model_t*           model;
    oag_device_scheduler_t devices;
    oag_sampler_t*         sampler;

    // Stats
    int64_t  total_tokens;
    double   total_time_ms;
    double   prompt_time_ms;
    double   gen_time_ms;
} oag_inference_t;

// Create inference engine
oag_inference_t* oag_inference_create(const char* model_path);
oag_inference_t* oag_inference_create_with_ctx(const char* model_path,
                                               int32_t context_length);
void             oag_inference_free(oag_inference_t* inf);

// Chat completion (OpenAI-compatible)
typedef struct {
    const char* role;     // "system", "user", "assistant"
    const char* content;
} oag_chat_msg_t;

typedef struct {
    oag_chat_msg_t* messages;
    int32_t         n_messages;
    oag_sampler_params_t sampler;
    int32_t         max_tokens;
    bool            stream;
    void            (*on_token)(const char* text, void* user);
    void*           user_data;
} oag_chat_params_t;

// Run chat completion
char* oag_inference_chat(oag_inference_t* inf, oag_chat_params_t params);

// Performance stats
void oag_inference_print_stats(const oag_inference_t* inf);

#endif // OAG_INFERENCE_H
