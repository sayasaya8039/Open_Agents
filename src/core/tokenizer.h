// Open_Agents — BPE Tokenizer
#ifndef OAG_TOKENIZER_H
#define OAG_TOKENIZER_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include "core/gguf.h"

#define OAG_MAX_TOKEN_LEN 256

typedef struct {
    char*   text;
    float   score;
    int32_t type;  // 0=normal, 1=unknown, 2=control, 3=user_defined, 4=unused, 5=byte
} oag_vocab_entry_t;

typedef struct {
    oag_vocab_entry_t* vocab;
    uint32_t           n_vocab;

    // Special tokens
    int32_t bos_id;  // beginning of sequence
    int32_t eos_id;  // end of sequence
    int32_t pad_id;  // padding
    int32_t nl_id;   // newline

    // BPE merge rules
    char**   merges;
    uint32_t n_merges;

    // Model type
    enum {
        OAG_TOKENIZER_BPE = 0,
        OAG_TOKENIZER_SPM = 1,  // SentencePiece
    } type;
} oag_tokenizer_t;

// Load tokenizer from GGUF metadata
oag_tokenizer_t* oag_tokenizer_load(const gguf_ctx_t* gguf);
void             oag_tokenizer_free(oag_tokenizer_t* tok);

// Encode text to token IDs
int32_t* oag_tokenizer_encode(const oag_tokenizer_t* tok,
                               const char* text,
                               size_t* out_n_tokens);

// Decode token ID to text
const char* oag_tokenizer_decode(const oag_tokenizer_t* tok, int32_t token_id);

// Decode sequence of token IDs
char* oag_tokenizer_decode_batch(const oag_tokenizer_t* tok,
                                  const int32_t* tokens,
                                  size_t n_tokens);

#endif // OAG_TOKENIZER_H
