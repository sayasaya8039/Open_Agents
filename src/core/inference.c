// Open_Agents — Inference Pipeline (Multi-device: GPU + CPU + NPU parallel)
#include "core/inference.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
static double inf_get_time_ms(void) {
    LARGE_INTEGER freq, cnt;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&cnt);
    return (double)cnt.QuadPart / (double)freq.QuadPart * 1000.0;
}
#else
#include <time.h>
static double inf_get_time_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000.0 + ts.tv_nsec / 1e6;
}
#endif

// ============================================================
// Device scheduler — NPU/CPU/GPU parallel
// ============================================================

static void init_device_scheduler(oag_device_scheduler_t* ds) {
    // Always create CPU backend
    ds->cpu = oag_backend_create(OAG_BACKEND_CPU);
    ds->gpu = NULL;
    ds->npu = NULL;
    ds->use_gpu = false;
    ds->use_npu = false;
    ds->use_multi_device = false;

    // Auto-detect GPUs via DXGI and select best backend
    oag_gpu_detect_t det = oag_gpu_detect();
    oag_gpu_detect_print(&det);

    if (det.best_gpu_idx >= 0) {
        oag_backend_type_t gpu_type = det.recommended_backend;

        // Create GPU backend (CUDA for NVIDIA, DirectML for AMD/Intel)
        ds->gpu = oag_backend_create(gpu_type);
        if (ds->gpu) {
            ds->use_gpu = true;
            oag_device_info_t info;
            if (ds->gpu->vt->get_device_info) {
                ds->gpu->vt->get_device_info(ds->gpu, &info);
                printf("[Scheduler] GPU: %s (%.1f GB) → %s\n",
                       info.name,
                       info.memory_total / (1024.0 * 1024.0 * 1024.0),
                       oag_backend_type_name(gpu_type));
            }
        } else {
            printf("[Scheduler] GPU backend %s failed to init, trying fallback...\n",
                   oag_backend_type_name(gpu_type));

            // CUDA failed → try DirectML
            if (gpu_type == OAG_BACKEND_CUDA) {
                ds->gpu = oag_backend_create(OAG_BACKEND_DIRECTML);
                if (ds->gpu) {
                    ds->use_gpu = true;
                    printf("[Scheduler] Fallback: DirectML\n");
                }
            }
        }
    }

    // Try NPU (Intel AI Boost via OpenVINO)
    ds->npu = oag_backend_create(OAG_BACKEND_NPU);
    if (ds->npu) {
        ds->use_npu = true;
        oag_device_info_t npu_info;
        if (ds->npu->vt->get_device_info) {
            ds->npu->vt->get_device_info(ds->npu, &npu_info);
            printf("[Scheduler] NPU: %s\n", npu_info.name);
        }
    }

    ds->use_multi_device = ds->use_gpu || ds->use_npu;

    if (ds->use_multi_device) {
        printf("[Scheduler] Multi-device mode: CPU");
        if (ds->use_gpu) printf(" + GPU(%s)", ds->gpu->vt->name);
        if (ds->use_npu) printf(" + NPU");
        printf("\n");
    } else {
        printf("[Scheduler] CPU-only mode (AVX2 + multi-threaded)\n");
    }
}

// ============================================================
// Create/free
// ============================================================

oag_inference_t* oag_inference_create(const char* model_path) {
    printf("[Inference] Initializing...\n");
    double t0 = inf_get_time_ms();

    oag_inference_t* inf = (oag_inference_t*)calloc(1, sizeof(oag_inference_t));

    // Initialize device scheduler
    init_device_scheduler(&inf->devices);

    // Load model with best available backend
    oag_backend_t* primary = inf->devices.use_gpu ? inf->devices.gpu : inf->devices.cpu;
    inf->model = oag_model_load(model_path, primary);
    if (!inf->model) {
        fprintf(stderr, "[Inference] Failed to load model\n");
        oag_inference_free(inf);
        return NULL;
    }

    inf->sampler = oag_sampler_create(oag_sampler_default_params());

    double t1 = inf_get_time_ms();
    printf("[Inference] Ready in %.1f ms\n", t1 - t0);
    return inf;
}

void oag_inference_free(oag_inference_t* inf) {
    if (!inf) return;
    oag_model_free(inf->model);
    oag_sampler_free(inf->sampler);
    if (inf->devices.cpu) oag_backend_destroy(inf->devices.cpu);
    if (inf->devices.gpu) oag_backend_destroy(inf->devices.gpu);
    if (inf->devices.npu) oag_backend_destroy(inf->devices.npu);
    free(inf);
}

// ============================================================
// Chat completion
// ============================================================

static char* build_chat_prompt(const oag_chat_msg_t* messages, int32_t n_messages) {
    // Build ChatML-style prompt
    size_t buf_size = 4096;
    for (int32_t i = 0; i < n_messages; i++) {
        buf_size += strlen(messages[i].content) + 64;
    }

    char* buf = (char*)malloc(buf_size);
    size_t pos = 0;

    for (int32_t i = 0; i < n_messages; i++) {
        pos += snprintf(buf + pos, buf_size - pos,
                       "<|im_start|>%s\n%s<|im_end|>\n",
                       messages[i].role, messages[i].content);
    }
    pos += snprintf(buf + pos, buf_size - pos, "<|im_start|>assistant\n");

    return buf;
}

char* oag_inference_chat(oag_inference_t* inf, oag_chat_params_t params) {
    if (!inf || !inf->model) return strdup("[Error: no model loaded]");

    double t0 = inf_get_time_ms();

    // Build prompt
    char* prompt_text = build_chat_prompt(params.messages, params.n_messages);

    // Tokenize
    if (!inf->model->tokenizer) {
        free(prompt_text);
        return strdup("[Error: tokenizer not loaded]");
    }

    size_t n_tokens;
    int32_t* tokens = oag_tokenizer_encode(inf->model->tokenizer, prompt_text, &n_tokens);
    free(prompt_text);

    if (!tokens || n_tokens == 0) {
        return strdup("[Error: tokenization failed]");
    }

    printf("[Chat] Prompt: %zu tokens | Max gen: %d\n", n_tokens, params.max_tokens);

    // Generate
    oag_gen_params_t gen = {
        .sampler    = params.sampler,
        .max_tokens = params.max_tokens,
        .stream     = params.stream,
        .on_token   = NULL,
        .user_data  = params.user_data,
    };

    // NOTE: gen.on_token has signature (int32_t, const char*, void*)
    // params.on_token has signature (const char*, void*)
    // We leave gen.on_token = NULL and don't stream through model_generate
    // Streaming is handled at the HTTP/TUI layer instead
    (void)params.stream;
    (void)params.on_token;

    int32_t n_gen;
    int32_t* output = oag_model_generate(inf->model, tokens, (int32_t)n_tokens, gen, &n_gen);

    // Decode generated tokens
    char* result = oag_tokenizer_decode_batch(inf->model->tokenizer,
                                              output + n_tokens, n_gen);

    double t1 = inf_get_time_ms();
    inf->total_tokens += n_gen;
    inf->total_time_ms += (t1 - t0);
    inf->prompt_time_ms = 0;  // tracked inside generate
    inf->gen_time_ms = t1 - t0;

    free(tokens);
    free(output);
    return result;
}

// ============================================================
// Stats
// ============================================================

void oag_inference_print_stats(const oag_inference_t* inf) {
    printf("╔══════════════════════════════════════╗\n");
    printf("║     Open_Agents Performance Stats    ║\n");
    printf("╠══════════════════════════════════════╣\n");
    printf("║ Total tokens:  %8lld              ║\n", (long long)inf->total_tokens);
    printf("║ Total time:    %8.1f ms           ║\n", inf->total_time_ms);
    if (inf->total_time_ms > 0) {
        printf("║ Throughput:    %8.1f tok/s        ║\n",
               inf->total_tokens / (inf->total_time_ms / 1000.0));
    }
    printf("║ Backend:       CPU");
    if (inf->devices.use_gpu) printf("+GPU");
    if (inf->devices.use_npu) printf("+NPU");
    printf("              ║\n");
    printf("╚══════════════════════════════════════╝\n");
}
