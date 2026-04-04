// Open_Agents — GGUF Format Parser
// Memory-maps GGUF files and parses metadata + tensor info
#include "core/gguf.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
#else
#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
#endif

// ============================================================
// Type size tables
// ============================================================

static const size_t GGML_TYPE_SIZES[] = {
    [GGML_TYPE_F32]     = 4,
    [GGML_TYPE_F16]     = 2,
    [GGML_TYPE_Q4_0]    = 18,   // 32 values per block: 2 + 16
    [GGML_TYPE_Q4_1]    = 20,
    [GGML_TYPE_Q5_0]    = 22,
    [GGML_TYPE_Q5_1]    = 24,
    [GGML_TYPE_Q8_0]    = 34,
    [GGML_TYPE_Q8_1]    = 40,
    [GGML_TYPE_Q2_K]    = 84,
    [GGML_TYPE_Q3_K]    = 110,
    [GGML_TYPE_Q4_K]    = 144,
    [GGML_TYPE_Q5_K]    = 176,
    [GGML_TYPE_Q6_K]    = 210,
    [GGML_TYPE_Q8_K]    = 292,
};

static const size_t GGML_BLOCK_SIZES[] = {
    [GGML_TYPE_F32]     = 1,
    [GGML_TYPE_F16]     = 1,
    [GGML_TYPE_Q4_0]    = 32,
    [GGML_TYPE_Q4_1]    = 32,
    [GGML_TYPE_Q5_0]    = 32,
    [GGML_TYPE_Q5_1]    = 32,
    [GGML_TYPE_Q8_0]    = 32,
    [GGML_TYPE_Q8_1]    = 32,
    [GGML_TYPE_Q2_K]    = 256,
    [GGML_TYPE_Q3_K]    = 256,
    [GGML_TYPE_Q4_K]    = 256,
    [GGML_TYPE_Q5_K]    = 256,
    [GGML_TYPE_Q6_K]    = 256,
    [GGML_TYPE_Q8_K]    = 256,
};

size_t ggml_type_size(ggml_type_t type) {
    if (type <= GGML_TYPE_Q8_K) return GGML_TYPE_SIZES[type];
    return 0;
}

size_t ggml_type_block_size(ggml_type_t type) {
    if (type <= GGML_TYPE_Q8_K) return GGML_BLOCK_SIZES[type];
    return 0;
}

const char* ggml_type_name(ggml_type_t type) {
    switch (type) {
        case GGML_TYPE_F32:   return "F32";
        case GGML_TYPE_F16:   return "F16";
        case GGML_TYPE_Q4_0:  return "Q4_0";
        case GGML_TYPE_Q4_1:  return "Q4_1";
        case GGML_TYPE_Q5_0:  return "Q5_0";
        case GGML_TYPE_Q5_1:  return "Q5_1";
        case GGML_TYPE_Q8_0:  return "Q8_0";
        case GGML_TYPE_Q8_1:  return "Q8_1";
        case GGML_TYPE_Q2_K:  return "Q2_K";
        case GGML_TYPE_Q3_K:  return "Q3_K";
        case GGML_TYPE_Q4_K:  return "Q4_K";
        case GGML_TYPE_Q5_K:  return "Q5_K";
        case GGML_TYPE_Q6_K:  return "Q6_K";
        case GGML_TYPE_Q8_K:  return "Q8_K";
        default:              return "UNKNOWN";
    }
}

// ============================================================
// Binary reader helpers
// ============================================================

typedef struct {
    const uint8_t* data;
    size_t         pos;
    size_t         size;
} reader_t;

static bool reader_has(const reader_t* r, size_t n) {
    return r->pos + n <= r->size;
}

static uint8_t read_u8(reader_t* r) {
    uint8_t v = r->data[r->pos];
    r->pos += 1;
    return v;
}

static uint32_t read_u32(reader_t* r) {
    uint32_t v;
    memcpy(&v, r->data + r->pos, 4);
    r->pos += 4;
    return v;
}

static uint64_t read_u64(reader_t* r) {
    uint64_t v;
    memcpy(&v, r->data + r->pos, 8);
    r->pos += 8;
    return v;
}

static int32_t read_i32(reader_t* r) {
    int32_t v;
    memcpy(&v, r->data + r->pos, 4);
    r->pos += 4;
    return v;
}

static float read_f32(reader_t* r) {
    float v;
    memcpy(&v, r->data + r->pos, 4);
    r->pos += 4;
    return v;
}

static double read_f64(reader_t* r) {
    double v;
    memcpy(&v, r->data + r->pos, 8);
    r->pos += 8;
    return v;
}

static gguf_string_t read_string(reader_t* r) {
    gguf_string_t s;
    s.len = read_u64(r);
    s.data = (char*)malloc(s.len + 1);
    memcpy(s.data, r->data + r->pos, s.len);
    s.data[s.len] = '\0';
    r->pos += s.len;
    return s;
}

// ============================================================
// Memory mapping
// ============================================================

#ifdef _WIN32
static void* mmap_file(const char* path, size_t* out_size) {
    HANDLE file = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ,
                              NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) {
        fprintf(stderr, "[GGUF] Cannot open: %s\n", path);
        return NULL;
    }

    LARGE_INTEGER file_size;
    GetFileSizeEx(file, &file_size);
    *out_size = (size_t)file_size.QuadPart;

    HANDLE mapping = CreateFileMappingA(file, NULL, PAGE_READONLY, 0, 0, NULL);
    if (!mapping) {
        CloseHandle(file);
        return NULL;
    }

    void* base = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0);
    CloseHandle(mapping);
    CloseHandle(file);
    return base;
}

static void munmap_file(void* base, size_t size) {
    (void)size;
    UnmapViewOfFile(base);
}
#else
static void* mmap_file(const char* path, size_t* out_size) {
    int fd = open(path, O_RDONLY);
    if (fd < 0) {
        fprintf(stderr, "[GGUF] Cannot open: %s\n", path);
        return NULL;
    }
    struct stat st;
    fstat(fd, &st);
    *out_size = st.st_size;
    void* base = mmap(NULL, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    close(fd);
    if (base == MAP_FAILED) return NULL;
    return base;
}

static void munmap_file(void* base, size_t size) {
    munmap(base, size);
}
#endif

// ============================================================
// GGUF loader
// ============================================================

static bool read_kv_value(reader_t* r, gguf_kv_t* kv) {
    kv->type = (gguf_type_t)read_u32(r);
    switch (kv->type) {
        case GGUF_TYPE_UINT8:   kv->value.u8  = read_u8(r);     break;
        case GGUF_TYPE_INT8:    kv->value.i8  = (int8_t)read_u8(r); break;
        case GGUF_TYPE_UINT32:  kv->value.u32 = read_u32(r);    break;
        case GGUF_TYPE_INT32:   kv->value.i32 = read_i32(r);    break;
        case GGUF_TYPE_FLOAT32: kv->value.f32 = read_f32(r);    break;
        case GGUF_TYPE_BOOL:    kv->value.b   = read_u8(r) != 0; break;
        case GGUF_TYPE_STRING:  kv->value.str = read_string(r);  break;
        case GGUF_TYPE_UINT64:  kv->value.u64 = read_u64(r);    break;
        case GGUF_TYPE_INT64:   kv->value.i64 = (int64_t)read_u64(r); break;
        case GGUF_TYPE_FLOAT64: kv->value.f64 = read_f64(r);    break;
        case GGUF_TYPE_ARRAY: {
            kv->value.array.elem_type = (gguf_type_t)read_u32(r);
            kv->value.array.count     = read_u64(r);
            // Skip array data for now (store position)
            kv->value.array.data = NULL;
            for (uint64_t i = 0; i < kv->value.array.count; i++) {
                switch (kv->value.array.elem_type) {
                    case GGUF_TYPE_UINT8:
                    case GGUF_TYPE_INT8:
                    case GGUF_TYPE_BOOL:   r->pos += 1; break;
                    case GGUF_TYPE_UINT16:
                    case GGUF_TYPE_INT16:  r->pos += 2; break;
                    case GGUF_TYPE_UINT32:
                    case GGUF_TYPE_INT32:
                    case GGUF_TYPE_FLOAT32: r->pos += 4; break;
                    case GGUF_TYPE_UINT64:
                    case GGUF_TYPE_INT64:
                    case GGUF_TYPE_FLOAT64: r->pos += 8; break;
                    case GGUF_TYPE_STRING: {
                        uint64_t slen = read_u64(r);
                        r->pos += slen;
                        break;
                    }
                    default:
                        fprintf(stderr, "[GGUF] Unknown array elem type: %d\n",
                                kv->value.array.elem_type);
                        return false;
                }
            }
            break;
        }
        default:
            fprintf(stderr, "[GGUF] Unknown KV type: %d\n", kv->type);
            return false;
    }
    return true;
}

gguf_ctx_t* gguf_load(const char* path) {
    size_t file_size;
    void* base = mmap_file(path, &file_size);
    if (!base) return NULL;

    reader_t r = { .data = (const uint8_t*)base, .pos = 0, .size = file_size };

    // Verify magic
    uint32_t magic = read_u32(&r);
    if (magic != GGUF_MAGIC) {
        fprintf(stderr, "[GGUF] Invalid magic: 0x%08X (expected 0x%08X)\n", magic, GGUF_MAGIC);
        munmap_file(base, file_size);
        return NULL;
    }

    uint32_t version = read_u32(&r);
    if (version < 2 || version > 3) {
        fprintf(stderr, "[GGUF] Unsupported version: %u\n", version);
        munmap_file(base, file_size);
        return NULL;
    }

    gguf_ctx_t* ctx = (gguf_ctx_t*)calloc(1, sizeof(gguf_ctx_t));
    ctx->version    = version;
    ctx->n_tensors  = read_u64(&r);
    ctx->n_kv       = read_u64(&r);
    ctx->mmap_base  = base;
    ctx->mmap_size  = file_size;

    printf("[GGUF] Version: %u, Tensors: %llu, KV pairs: %llu\n",
           ctx->version, (unsigned long long)ctx->n_tensors,
           (unsigned long long)ctx->n_kv);

    // Read metadata KV pairs
    ctx->kv = (gguf_kv_t*)calloc(ctx->n_kv, sizeof(gguf_kv_t));
    for (uint64_t i = 0; i < ctx->n_kv; i++) {
        ctx->kv[i].key = read_string(&r);
        if (!read_kv_value(&r, &ctx->kv[i])) {
            fprintf(stderr, "[GGUF] Failed reading KV #%llu\n", (unsigned long long)i);
            gguf_free(ctx);
            return NULL;
        }
    }

    // Read tensor infos
    ctx->tensors = (gguf_tensor_info_t*)calloc(ctx->n_tensors, sizeof(gguf_tensor_info_t));
    for (uint64_t i = 0; i < ctx->n_tensors; i++) {
        ctx->tensors[i].name   = read_string(&r);
        ctx->tensors[i].n_dims = read_u32(&r);
        for (uint32_t d = 0; d < ctx->tensors[i].n_dims; d++) {
            ctx->tensors[i].dims[d] = read_u64(&r);
        }
        ctx->tensors[i].type   = (ggml_type_t)read_u32(&r);
        ctx->tensors[i].offset = read_u64(&r);
    }

    // Tensor data starts at aligned position after header
    size_t alignment = 32;  // GGUF default alignment
    const char* align_str = gguf_get_str(ctx, "general.alignment");
    if (align_str) alignment = (size_t)atol(align_str);

    size_t header_end = r.pos;
    size_t data_start = (header_end + alignment - 1) & ~(alignment - 1);
    ctx->tensor_data_start = (uint8_t*)base + data_start;

    // Parse common metadata
    ctx->arch = NULL;
    const char* arch = gguf_get_str(ctx, "general.architecture");
    if (arch) {
        ctx->arch = strdup(arch);
        char key[256];

        snprintf(key, sizeof(key), "%s.embedding_length", arch);
        ctx->n_embd = gguf_get_u32(ctx, key, 0);

        snprintf(key, sizeof(key), "%s.attention.head_count", arch);
        ctx->n_head = gguf_get_u32(ctx, key, 0);

        snprintf(key, sizeof(key), "%s.attention.head_count_kv", arch);
        ctx->n_head_kv = gguf_get_u32(ctx, key, ctx->n_head);

        snprintf(key, sizeof(key), "%s.block_count", arch);
        ctx->n_layer = gguf_get_u32(ctx, key, 0);

        snprintf(key, sizeof(key), "%s.context_length", arch);
        ctx->n_ctx = gguf_get_u32(ctx, key, 2048);

        snprintf(key, sizeof(key), "%s.feed_forward_length", arch);
        ctx->n_ff = gguf_get_u32(ctx, key, 0);

        snprintf(key, sizeof(key), "%s.rope.freq_base", arch);
        ctx->rope_freq_base = gguf_get_f32(ctx, key, 10000.0f);

        snprintf(key, sizeof(key), "%s.attention.layer_norm_rms_epsilon", arch);
        ctx->norm_eps = gguf_get_f32(ctx, key, 1e-5f);

        // Vocab from tokenizer
        ctx->n_vocab = gguf_get_u32(ctx, "tokenizer.ggml.vocab_size", 0);
    }

    printf("[GGUF] Arch: %s | Embd: %u | Heads: %u/%u | Layers: %u | Ctx: %u | FF: %u\n",
           ctx->arch ? ctx->arch : "unknown",
           ctx->n_embd, ctx->n_head, ctx->n_head_kv,
           ctx->n_layer, ctx->n_ctx, ctx->n_ff);

    return ctx;
}

void gguf_free(gguf_ctx_t* ctx) {
    if (!ctx) return;

    for (uint64_t i = 0; i < ctx->n_kv; i++) {
        free(ctx->kv[i].key.data);
        if (ctx->kv[i].type == GGUF_TYPE_STRING) {
            free(ctx->kv[i].value.str.data);
        }
    }
    free(ctx->kv);

    for (uint64_t i = 0; i < ctx->n_tensors; i++) {
        free(ctx->tensors[i].name.data);
    }
    free(ctx->tensors);

    free(ctx->arch);

    if (ctx->mmap_base) {
        munmap_file(ctx->mmap_base, ctx->mmap_size);
    }

    free(ctx);
}

// ============================================================
// Metadata accessors
// ============================================================

const char* gguf_get_str(const gguf_ctx_t* ctx, const char* key) {
    for (uint64_t i = 0; i < ctx->n_kv; i++) {
        if (strcmp(ctx->kv[i].key.data, key) == 0) {
            if (ctx->kv[i].type == GGUF_TYPE_STRING) {
                return ctx->kv[i].value.str.data;
            }
            return NULL;
        }
    }
    return NULL;
}

uint32_t gguf_get_u32(const gguf_ctx_t* ctx, const char* key, uint32_t def) {
    for (uint64_t i = 0; i < ctx->n_kv; i++) {
        if (strcmp(ctx->kv[i].key.data, key) == 0) {
            switch (ctx->kv[i].type) {
                case GGUF_TYPE_UINT32: return ctx->kv[i].value.u32;
                case GGUF_TYPE_INT32:  return (uint32_t)ctx->kv[i].value.i32;
                case GGUF_TYPE_UINT64: return (uint32_t)ctx->kv[i].value.u64;
                default: return def;
            }
        }
    }
    return def;
}

float gguf_get_f32(const gguf_ctx_t* ctx, const char* key, float def) {
    for (uint64_t i = 0; i < ctx->n_kv; i++) {
        if (strcmp(ctx->kv[i].key.data, key) == 0) {
            switch (ctx->kv[i].type) {
                case GGUF_TYPE_FLOAT32: return ctx->kv[i].value.f32;
                case GGUF_TYPE_FLOAT64: return (float)ctx->kv[i].value.f64;
                default: return def;
            }
        }
    }
    return def;
}

// ============================================================
// Tensor data access
// ============================================================

const void* gguf_tensor_data(const gguf_ctx_t* ctx, uint64_t tensor_idx) {
    if (tensor_idx >= ctx->n_tensors) return NULL;
    return ctx->tensor_data_start + ctx->tensors[tensor_idx].offset;
}

size_t gguf_tensor_size(const gguf_tensor_info_t* info) {
    uint64_t n_elem = 1;
    for (uint32_t d = 0; d < info->n_dims; d++) {
        n_elem *= info->dims[d];
    }
    size_t block_sz = ggml_type_block_size(info->type);
    size_t type_sz  = ggml_type_size(info->type);
    if (block_sz == 0) return 0;
    return (n_elem / block_sz) * type_sz;
}

const gguf_tensor_info_t* gguf_find_tensor(const gguf_ctx_t* ctx, const char* name) {
    for (uint64_t i = 0; i < ctx->n_tensors; i++) {
        if (strcmp(ctx->tensors[i].name.data, name) == 0) {
            return &ctx->tensors[i];
        }
    }
    return NULL;
}
