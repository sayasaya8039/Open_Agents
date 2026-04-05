// Open_Agents — GGUF Format Parser
// Spec: https://github.com/ggerganov/ggml/blob/master/docs/gguf.md
#ifndef OAG_GGUF_H
#define OAG_GGUF_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

/* 先頭 4 バイトが G,G,U,F (0x47,0x47,0x55,0x46)。memcpy で LE uint32 として読むと 0x46554747 */
#define GGUF_MAGIC 0x46554747U
#define GGUF_VERSION_3 3

// GGUF value types
typedef enum {
    GGUF_TYPE_UINT8   = 0,
    GGUF_TYPE_INT8    = 1,
    GGUF_TYPE_UINT16  = 2,
    GGUF_TYPE_INT16   = 3,
    GGUF_TYPE_UINT32  = 4,
    GGUF_TYPE_INT32   = 5,
    GGUF_TYPE_FLOAT32 = 6,
    GGUF_TYPE_BOOL    = 7,
    GGUF_TYPE_STRING  = 8,
    GGUF_TYPE_ARRAY   = 9,
    GGUF_TYPE_UINT64  = 10,
    GGUF_TYPE_INT64   = 11,
    GGUF_TYPE_FLOAT64 = 12,
} gguf_type_t;

// Tensor quantization types
typedef enum {
    GGML_TYPE_F32     = 0,
    GGML_TYPE_F16     = 1,
    GGML_TYPE_Q4_0    = 2,
    GGML_TYPE_Q4_1    = 3,
    GGML_TYPE_Q5_0    = 6,
    GGML_TYPE_Q5_1    = 7,
    GGML_TYPE_Q8_0    = 8,
    GGML_TYPE_Q8_1    = 9,
    GGML_TYPE_Q2_K    = 10,
    GGML_TYPE_Q3_K    = 11,
    GGML_TYPE_Q4_K    = 12,
    GGML_TYPE_Q5_K    = 13,
    GGML_TYPE_Q6_K    = 14,
    GGML_TYPE_Q8_K    = 15,
    GGML_TYPE_IQ2_XXS = 16,
    GGML_TYPE_IQ2_XS  = 17,
    GGML_TYPE_IQ3_XXS = 18,
    GGML_TYPE_IQ1_S   = 19,
    GGML_TYPE_IQ4_NL  = 20,
    GGML_TYPE_IQ3_S   = 21,
    GGML_TYPE_IQ2_S   = 22,
    GGML_TYPE_IQ4_XS  = 23,
    GGML_TYPE_I8      = 24,
    GGML_TYPE_I16     = 25,
    GGML_TYPE_I32     = 26,
    GGML_TYPE_I64     = 27,
    GGML_TYPE_F64     = 28,
    GGML_TYPE_IQ1_M   = 29,
    GGML_TYPE_BF16    = 30,
} ggml_type_t;

// GGUF string
typedef struct {
    uint64_t len;
    char*    data;
} gguf_string_t;

// GGUF metadata key-value
typedef struct {
    gguf_string_t key;
    gguf_type_t   type;
    union {
        uint8_t   u8;
        int8_t    i8;
        uint16_t  u16;
        int16_t   i16;
        uint32_t  u32;
        int32_t   i32;
        float     f32;
        bool      b;
        uint64_t  u64;
        int64_t   i64;
        double    f64;
        gguf_string_t str;
        struct {
            gguf_type_t elem_type;
            uint64_t    count;
            void*       data;
        } array;
    } value;
} gguf_kv_t;

// GGUF tensor info
typedef struct {
    gguf_string_t name;
    uint32_t      n_dims;
    uint64_t      dims[4];
    ggml_type_t   type;
    uint64_t      offset;  // offset from start of tensor data
} gguf_tensor_info_t;

// GGUF file context
typedef struct {
    uint32_t version;
    uint64_t n_tensors;
    uint64_t n_kv;

    gguf_kv_t*          kv;
    gguf_tensor_info_t* tensors;

    // Memory-mapped file data
    void*    mmap_base;
    size_t   mmap_size;
    /* Windows: CreateFileMapping の HANDLE。MapView 後に直ちに Close すると未定義動作になるため gguf_free まで保持 */
    void*    mmap_mapping_handle;
    uint8_t* tensor_data_start;

    // Parsed metadata cache
    char*    arch;           // e.g. "llama", "qwen2"
    uint32_t n_vocab;
    uint32_t n_embd;
    uint32_t n_head;
    uint32_t n_head_kv;
    uint32_t n_layer;
    uint32_t n_ctx;          // context length
    uint32_t n_ff;
    float    rope_freq_base;
    float    norm_eps;
} gguf_ctx_t;

// Load/free
gguf_ctx_t* gguf_load(const char* path);
void        gguf_free(gguf_ctx_t* ctx);
/** 直近の gguf_load 失敗理由（スレッド非安全・GUI 表示用） */
const char* gguf_get_last_error(void);

// Metadata accessors
const char*   gguf_get_str(const gguf_ctx_t* ctx, const char* key);
uint32_t      gguf_get_u32(const gguf_ctx_t* ctx, const char* key, uint32_t def);
float         gguf_get_f32(const gguf_ctx_t* ctx, const char* key, float def);

// Tensor data access
const void*   gguf_tensor_data(const gguf_ctx_t* ctx, uint64_t tensor_idx);
size_t        gguf_tensor_size(const gguf_tensor_info_t* info);
const gguf_tensor_info_t* gguf_find_tensor(const gguf_ctx_t* ctx, const char* name);

// Type helpers
size_t ggml_type_size(ggml_type_t type);
size_t ggml_type_block_size(ggml_type_t type);
const char* ggml_type_name(ggml_type_t type);

#endif // OAG_GGUF_H
