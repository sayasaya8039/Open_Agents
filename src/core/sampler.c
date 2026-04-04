// Open_Agents — Sampling (top-k, top-p, min-p, temperature, repeat penalty)
#include "core/sampler.h"
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <float.h>

#ifdef _WIN32
#include <windows.h>
#else
#include <time.h>
#include <unistd.h>
#endif

// ============================================================
// RNG (xorshift64*)
// ============================================================

static uint64_t xorshift64(uint64_t* state) {
    uint64_t x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    return x * 0x2545F4914F6CDD1DULL;
}

static float rand_f32(uint64_t* state) {
    return (float)(xorshift64(state) >> 11) / (float)(1ULL << 53);
}

// ============================================================
// Default params
// ============================================================

oag_sampler_params_t oag_sampler_default_params(void) {
    return (oag_sampler_params_t){
        .temperature    = 0.7f,
        .top_p          = 0.9f,
        .top_k          = 40,
        .repeat_penalty = 1.1f,
        .repeat_window  = 64,
        .seed           = 0,
        .min_p          = 0.05f,
    };
}

// ============================================================
// Create/free
// ============================================================

oag_sampler_t* oag_sampler_create(oag_sampler_params_t params) {
    oag_sampler_t* s = (oag_sampler_t*)calloc(1, sizeof(oag_sampler_t));
    s->params = params;
    if (params.seed == 0) {
        // Use time-based seed
        #ifdef _WIN32
        LARGE_INTEGER li;
        QueryPerformanceCounter(&li);
        s->rng_state = (uint64_t)li.QuadPart;
        #else
        s->rng_state = (uint64_t)time(NULL) ^ ((uint64_t)getpid() << 16);
        #endif
    } else {
        s->rng_state = params.seed;
    }
    if (s->rng_state == 0) s->rng_state = 1;
    return s;
}

void oag_sampler_free(oag_sampler_t* s) {
    free(s);
}

// ============================================================
// Greedy (argmax)
// ============================================================

int32_t oag_sampler_greedy(const float* logits, int32_t n_vocab) {
    int32_t max_idx = 0;
    float max_val = logits[0];
    for (int32_t i = 1; i < n_vocab; i++) {
        if (logits[i] > max_val) {
            max_val = logits[i];
            max_idx = i;
        }
    }
    return max_idx;
}

// ============================================================
// Candidate sorting helper
// ============================================================

typedef struct {
    int32_t id;
    float   logit;
} candidate_t;

static int cmp_candidates_desc(const void* a, const void* b) {
    float da = ((const candidate_t*)a)->logit;
    float db = ((const candidate_t*)b)->logit;
    return (da < db) - (da > db);
}

// ============================================================
// Full sampling pipeline
// ============================================================

int32_t oag_sampler_sample(oag_sampler_t* s,
                            float* logits,
                            int32_t n_vocab,
                            const int32_t* prev_tokens,
                            int32_t n_prev) {
    // 1. Repeat penalty
    if (s->params.repeat_penalty != 1.0f && prev_tokens && n_prev > 0) {
        int32_t window = (n_prev < s->params.repeat_window) ? n_prev : s->params.repeat_window;
        for (int32_t i = n_prev - window; i < n_prev; i++) {
            int32_t tid = prev_tokens[i];
            if (tid >= 0 && tid < n_vocab) {
                if (logits[tid] > 0) {
                    logits[tid] /= s->params.repeat_penalty;
                } else {
                    logits[tid] *= s->params.repeat_penalty;
                }
            }
        }
    }

    // 2. Temperature
    if (s->params.temperature <= 0.0f) {
        return oag_sampler_greedy(logits, n_vocab);
    }

    float inv_temp = 1.0f / s->params.temperature;
    for (int32_t i = 0; i < n_vocab; i++) {
        logits[i] *= inv_temp;
    }

    // 3. Build candidates and sort
    candidate_t* candidates = (candidate_t*)malloc(n_vocab * sizeof(candidate_t));
    for (int32_t i = 0; i < n_vocab; i++) {
        candidates[i].id    = i;
        candidates[i].logit = logits[i];
    }
    qsort(candidates, n_vocab, sizeof(candidate_t), cmp_candidates_desc);

    int32_t n_candidates = n_vocab;

    // 4. Top-k
    if (s->params.top_k > 0 && s->params.top_k < n_candidates) {
        n_candidates = s->params.top_k;
    }

    // 5. Min-p filtering
    if (s->params.min_p > 0.0f) {
        float max_logit = candidates[0].logit;
        float threshold = max_logit + logf(s->params.min_p);
        for (int32_t i = 1; i < n_candidates; i++) {
            if (candidates[i].logit < threshold) {
                n_candidates = i;
                break;
            }
        }
    }

    // 6. Softmax over candidates
    float max_val = candidates[0].logit;
    float sum = 0.0f;
    for (int32_t i = 0; i < n_candidates; i++) {
        candidates[i].logit = expf(candidates[i].logit - max_val);
        sum += candidates[i].logit;
    }
    for (int32_t i = 0; i < n_candidates; i++) {
        candidates[i].logit /= sum;
    }

    // 7. Top-p (nucleus)
    if (s->params.top_p < 1.0f) {
        float cumsum = 0.0f;
        for (int32_t i = 0; i < n_candidates; i++) {
            cumsum += candidates[i].logit;
            if (cumsum >= s->params.top_p) {
                n_candidates = i + 1;
                break;
            }
        }
        // Re-normalize
        sum = 0.0f;
        for (int32_t i = 0; i < n_candidates; i++) sum += candidates[i].logit;
        for (int32_t i = 0; i < n_candidates; i++) candidates[i].logit /= sum;
    }

    // 8. Random sample
    float r = rand_f32(&s->rng_state);
    float cumsum = 0.0f;
    int32_t selected = candidates[0].id;
    for (int32_t i = 0; i < n_candidates; i++) {
        cumsum += candidates[i].logit;
        if (r <= cumsum) {
            selected = candidates[i].id;
            break;
        }
    }

    free(candidates);
    return selected;
}
