// Open_Agents — BPE Tokenizer (GGUF-based)
#include "core/tokenizer.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// ============================================================
// Load from GGUF metadata
// ============================================================

oag_tokenizer_t* oag_tokenizer_load(const gguf_ctx_t* gguf) {
    oag_tokenizer_t* tok = (oag_tokenizer_t*)calloc(1, sizeof(oag_tokenizer_t));

    // Read vocab size
    tok->n_vocab = gguf_get_u32(gguf, "tokenizer.ggml.vocab_size", 0);
    if (tok->n_vocab == 0) {
        // Try counting tokens array
        for (uint64_t i = 0; i < gguf->n_kv; i++) {
            if (strcmp(gguf->kv[i].key.data, "tokenizer.ggml.tokens") == 0 &&
                gguf->kv[i].type == GGUF_TYPE_ARRAY) {
                tok->n_vocab = (uint32_t)gguf->kv[i].value.array.count;
                break;
            }
        }
    }

    if (tok->n_vocab == 0) {
        fprintf(stderr, "[Tokenizer] Cannot determine vocab size\n");
        free(tok);
        return NULL;
    }

    printf("[Tokenizer] Vocab size: %u\n", tok->n_vocab);

    // Allocate vocab (tokens will be loaded from GGUF string arrays)
    tok->vocab = (oag_vocab_entry_t*)calloc(tok->n_vocab, sizeof(oag_vocab_entry_t));

    // For now, create placeholder entries
    // Full implementation would parse tokenizer.ggml.tokens string array
    for (uint32_t i = 0; i < tok->n_vocab; i++) {
        tok->vocab[i].text = NULL;
        tok->vocab[i].score = 0.0f;
        tok->vocab[i].type = 0;
    }

    // Special tokens
    tok->bos_id = (int32_t)gguf_get_u32(gguf, "tokenizer.ggml.bos_token_id", 1);
    tok->eos_id = (int32_t)gguf_get_u32(gguf, "tokenizer.ggml.eos_token_id", 2);
    tok->pad_id = (int32_t)gguf_get_u32(gguf, "tokenizer.ggml.padding_token_id", 0);

    // Detect type
    const char* model_type = gguf_get_str(gguf, "tokenizer.ggml.model");
    if (model_type && strcmp(model_type, "gpt2") == 0) {
        tok->type = OAG_TOKENIZER_BPE;
    } else {
        tok->type = OAG_TOKENIZER_SPM;
    }

    printf("[Tokenizer] Type: %s | BOS: %d | EOS: %d\n",
           tok->type == OAG_TOKENIZER_BPE ? "BPE" : "SPM",
           tok->bos_id, tok->eos_id);

    return tok;
}

void oag_tokenizer_free(oag_tokenizer_t* tok) {
    if (!tok) return;
    if (tok->vocab) {
        for (uint32_t i = 0; i < tok->n_vocab; i++) {
            free(tok->vocab[i].text);
        }
        free(tok->vocab);
    }
    if (tok->merges) {
        for (uint32_t i = 0; i < tok->n_merges; i++) {
            free(tok->merges[i]);
        }
        free(tok->merges);
    }
    free(tok);
}

// ============================================================
// Simple byte-level encoding (fallback)
// ============================================================

int32_t* oag_tokenizer_encode(const oag_tokenizer_t* tok,
                               const char* text,
                               size_t* out_n_tokens) {
    // Simplified: byte-level fallback encoding
    // Full BPE/SPM would do proper merge operations
    size_t text_len = strlen(text);
    size_t max_tokens = text_len + 2;  // +BOS/EOS
    int32_t* tokens = (int32_t*)malloc(max_tokens * sizeof(int32_t));
    size_t n = 0;

    // Add BOS
    tokens[n++] = tok->bos_id;

    // Byte-level encoding (each byte → token ID)
    for (size_t i = 0; i < text_len; i++) {
        uint8_t byte = (uint8_t)text[i];
        // Map byte to vocab ID (byte tokens typically start at some offset)
        tokens[n++] = (int32_t)byte + 3;  // offset past special tokens
    }

    *out_n_tokens = n;
    return tokens;
}

const char* oag_tokenizer_decode(const oag_tokenizer_t* tok, int32_t token_id) {
    if (token_id < 0 || (uint32_t)token_id >= tok->n_vocab) return "";
    if (tok->vocab[token_id].text) return tok->vocab[token_id].text;
    return "";
}

char* oag_tokenizer_decode_batch(const oag_tokenizer_t* tok,
                                  const int32_t* tokens,
                                  size_t n_tokens) {
    size_t buf_size = n_tokens * OAG_MAX_TOKEN_LEN;
    char* buf = (char*)calloc(buf_size, 1);
    size_t pos = 0;

    for (size_t i = 0; i < n_tokens; i++) {
        const char* piece = oag_tokenizer_decode(tok, tokens[i]);
        size_t piece_len = strlen(piece);
        if (pos + piece_len < buf_size) {
            memcpy(buf + pos, piece, piece_len);
            pos += piece_len;
        }
    }

    return buf;
}
