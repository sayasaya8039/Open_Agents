// Open_Agents — Sampling strategies
#ifndef OAG_SAMPLER_H
#define OAG_SAMPLER_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

typedef struct {
    float    temperature;    // 0.0 = greedy, 0.7 = creative
    float    top_p;          // nucleus sampling threshold
    int32_t  top_k;          // top-k filtering
    float    repeat_penalty; // penalize repeated tokens
    int32_t  repeat_window;  // look-back window for repeat penalty
    uint64_t seed;           // RNG seed (0 = random)
    float    min_p;          // min-p sampling
} oag_sampler_params_t;

typedef struct {
    oag_sampler_params_t params;
    uint64_t             rng_state;  // xorshift64
} oag_sampler_t;

// Default params
oag_sampler_params_t oag_sampler_default_params(void);

// Create/free
oag_sampler_t* oag_sampler_create(oag_sampler_params_t params);
void           oag_sampler_free(oag_sampler_t* s);

// Sample next token from logits array [n_vocab]
int32_t oag_sampler_sample(oag_sampler_t* s,
                            float* logits,
                            int32_t n_vocab,
                            const int32_t* prev_tokens,
                            int32_t n_prev);

// Greedy (argmax)
int32_t oag_sampler_greedy(const float* logits, int32_t n_vocab);

#endif // OAG_SAMPLER_H
