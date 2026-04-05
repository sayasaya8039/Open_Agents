// Open_Agents — ONNX Model Loader
// Minimal protobuf parser for ONNX format (no external deps)
#include "core/onnx_loader.h"
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
// Protobuf wire types
// ============================================================

#define PB_VARINT  0
#define PB_64BIT   1
#define PB_LEN     2
#define PB_32BIT   5

typedef struct {
    const uint8_t* data;
    size_t         pos;
    size_t         end;
} pb_reader_t;

static uint64_t pb_read_varint(pb_reader_t* r) {
    uint64_t val = 0;
    int shift = 0;
    while (r->pos < r->end) {
        uint8_t b = r->data[r->pos++];
        val |= (uint64_t)(b & 0x7F) << shift;
        if (!(b & 0x80)) break;
        shift += 7;
    }
    return val;
}

static void pb_read_bytes(pb_reader_t* r, size_t len, const uint8_t** out, size_t* out_len) {
    *out = r->data + r->pos;
    *out_len = len;
    r->pos += len;
}

static char* pb_read_string(pb_reader_t* r, size_t len) {
    char* s = (char*)malloc(len + 1);
    memcpy(s, r->data + r->pos, len);
    s[len] = '\0';
    r->pos += len;
    return s;
}

// ============================================================
// Memory mapping (same as gguf.c)
// ============================================================

#ifdef _WIN32
static void* onnx_mmap(const char* path, size_t* out_size) {
    HANDLE file = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ,
                              NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return NULL;

    LARGE_INTEGER file_size;
    GetFileSizeEx(file, &file_size);
    *out_size = (size_t)file_size.QuadPart;

    HANDLE mapping = CreateFileMappingA(file, NULL, PAGE_READONLY, 0, 0, NULL);
    if (!mapping) { CloseHandle(file); return NULL; }

    void* base = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0);
    CloseHandle(mapping);
    CloseHandle(file);
    return base;
}

static void onnx_munmap(void* base, size_t size) {
    (void)size;
    UnmapViewOfFile(base);
}
#else
static void* onnx_mmap(const char* path, size_t* out_size) {
    int fd = open(path, O_RDONLY);
    if (fd < 0) return NULL;
    struct stat st;
    fstat(fd, &st);
    *out_size = st.st_size;
    void* base = mmap(NULL, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    close(fd);
    return (base == MAP_FAILED) ? NULL : base;
}

static void onnx_munmap(void* base, size_t size) {
    munmap(base, size);
}
#endif

// ============================================================
// ONNX protobuf field IDs (from onnx.proto3)
// ============================================================

// ModelProto fields
#define ONNX_MODEL_IR_VERSION    1
#define ONNX_MODEL_OPSET_IMPORT  8
#define ONNX_MODEL_PRODUCER      2
#define ONNX_MODEL_DOMAIN        4
#define ONNX_MODEL_GRAPH         7

// GraphProto fields
#define ONNX_GRAPH_NODE         1
#define ONNX_GRAPH_INITIALIZER  5
#define ONNX_GRAPH_INPUT        11
#define ONNX_GRAPH_OUTPUT       12

// NodeProto fields
#define ONNX_NODE_INPUT   1
#define ONNX_NODE_OUTPUT  2
#define ONNX_NODE_NAME    3
#define ONNX_NODE_OP_TYPE 4

// TensorProto fields
#define ONNX_TENSOR_DIMS      1
#define ONNX_TENSOR_DATA_TYPE 2
#define ONNX_TENSOR_NAME      8
#define ONNX_TENSOR_RAW_DATA  13

// ============================================================
// Parse ONNX tensor (initializer/weight)
// ============================================================

static void parse_tensor(pb_reader_t* r, oag_onnx_tensor_t* t) {
    memset(t, 0, sizeof(*t));
    while (r->pos < r->end) {
        uint64_t tag = pb_read_varint(r);
        uint32_t field = (uint32_t)(tag >> 3);
        uint32_t wire  = (uint32_t)(tag & 7);

        switch (field) {
            case ONNX_TENSOR_DIMS:
                if (wire == PB_VARINT) {
                    if (t->n_dims < 8) {
                        t->dims[t->n_dims++] = (int64_t)pb_read_varint(r);
                    }
                } else if (wire == PB_LEN) {
                    // packed repeated int64
                    size_t len = (size_t)pb_read_varint(r);
                    size_t end = r->pos + len;
                    while (r->pos < end && t->n_dims < 8) {
                        t->dims[t->n_dims++] = (int64_t)pb_read_varint(r);
                    }
                }
                break;

            case ONNX_TENSOR_DATA_TYPE:
                t->dtype = (oag_onnx_dtype_t)pb_read_varint(r);
                break;

            case ONNX_TENSOR_NAME: {
                size_t len = (size_t)pb_read_varint(r);
                t->name = pb_read_string(r, len);
                break;
            }

            case ONNX_TENSOR_RAW_DATA: {
                size_t len = (size_t)pb_read_varint(r);
                t->raw_data = (void*)(r->data + r->pos);
                t->raw_size = len;
                r->pos += len;
                break;
            }

            default:
                // Skip unknown fields
                if (wire == PB_VARINT) pb_read_varint(r);
                else if (wire == PB_64BIT) r->pos += 8;
                else if (wire == PB_32BIT) r->pos += 4;
                else if (wire == PB_LEN) {
                    size_t len = (size_t)pb_read_varint(r);
                    r->pos += len;
                }
                break;
        }
    }
}

// ============================================================
// Parse ONNX graph node
// ============================================================

static void parse_node(pb_reader_t* r, oag_onnx_node_t* node) {
    memset(node, 0, sizeof(*node));
    // Pre-allocate
    node->inputs  = (char**)calloc(16, sizeof(char*));
    node->outputs = (char**)calloc(16, sizeof(char*));

    while (r->pos < r->end) {
        uint64_t tag = pb_read_varint(r);
        uint32_t field = (uint32_t)(tag >> 3);
        uint32_t wire  = (uint32_t)(tag & 7);

        if (wire == PB_LEN) {
            size_t len = (size_t)pb_read_varint(r);
            switch (field) {
                case ONNX_NODE_INPUT:
                    if (node->n_inputs < 16)
                        node->inputs[node->n_inputs++] = pb_read_string(r, len);
                    else r->pos += len;
                    break;
                case ONNX_NODE_OUTPUT:
                    if (node->n_outputs < 16)
                        node->outputs[node->n_outputs++] = pb_read_string(r, len);
                    else r->pos += len;
                    break;
                case ONNX_NODE_NAME:
                    node->name = pb_read_string(r, len);
                    break;
                case ONNX_NODE_OP_TYPE:
                    node->op_type = pb_read_string(r, len);
                    break;
                default:
                    r->pos += len;
                    break;
            }
        } else if (wire == PB_VARINT) {
            pb_read_varint(r);
        } else if (wire == PB_64BIT) {
            r->pos += 8;
        } else if (wire == PB_32BIT) {
            r->pos += 4;
        }
    }
}

// ============================================================
// Load ONNX model
// ============================================================

oag_onnx_ctx_t* oag_onnx_load(const char* path) {
    size_t file_size;
    void* base = onnx_mmap(path, &file_size);
    if (!base) {
        fprintf(stderr, "[ONNX] Cannot open: %s\n", path);
        return NULL;
    }

    oag_onnx_ctx_t* ctx = (oag_onnx_ctx_t*)calloc(1, sizeof(oag_onnx_ctx_t));
    ctx->mmap_base = base;
    ctx->mmap_size = file_size;

    // Pre-allocate
    ctx->nodes        = (oag_onnx_node_t*)calloc(4096, sizeof(oag_onnx_node_t));
    ctx->initializers = (oag_onnx_tensor_t*)calloc(4096, sizeof(oag_onnx_tensor_t));
    ctx->input_names  = (char**)calloc(32, sizeof(char*));
    ctx->output_names = (char**)calloc(32, sizeof(char*));

    pb_reader_t model_r = { .data = (const uint8_t*)base, .pos = 0, .end = file_size };

    while (model_r.pos < model_r.end) {
        uint64_t tag = pb_read_varint(&model_r);
        uint32_t field = (uint32_t)(tag >> 3);
        uint32_t wire  = (uint32_t)(tag & 7);

        if (field == ONNX_MODEL_IR_VERSION && wire == PB_VARINT) {
            ctx->ir_version = (int64_t)pb_read_varint(&model_r);
        } else if (field == ONNX_MODEL_PRODUCER && wire == PB_LEN) {
            size_t len = (size_t)pb_read_varint(&model_r);
            ctx->producer = pb_read_string(&model_r, len);
        } else if (field == ONNX_MODEL_DOMAIN && wire == PB_LEN) {
            size_t len = (size_t)pb_read_varint(&model_r);
            ctx->domain = pb_read_string(&model_r, len);
        } else if (field == ONNX_MODEL_GRAPH && wire == PB_LEN) {
            // Parse graph
            size_t graph_len = (size_t)pb_read_varint(&model_r);
            size_t graph_end = model_r.pos + graph_len;

            while (model_r.pos < graph_end) {
                uint64_t gtag = pb_read_varint(&model_r);
                uint32_t gfield = (uint32_t)(gtag >> 3);
                uint32_t gwire  = (uint32_t)(gtag & 7);

                if (gwire == PB_LEN) {
                    size_t len = (size_t)pb_read_varint(&model_r);
                    size_t sub_end = model_r.pos + len;

                    if (gfield == ONNX_GRAPH_NODE && ctx->n_nodes < 4096) {
                        pb_reader_t sub = { .data = model_r.data, .pos = model_r.pos, .end = sub_end };
                        parse_node(&sub, &ctx->nodes[ctx->n_nodes++]);
                        model_r.pos = sub_end;
                    } else if (gfield == ONNX_GRAPH_INITIALIZER && ctx->n_initializers < 4096) {
                        pb_reader_t sub = { .data = model_r.data, .pos = model_r.pos, .end = sub_end };
                        parse_tensor(&sub, &ctx->initializers[ctx->n_initializers++]);
                        model_r.pos = sub_end;
                    } else {
                        model_r.pos = sub_end;
                    }
                } else if (gwire == PB_VARINT) {
                    pb_read_varint(&model_r);
                } else if (gwire == PB_64BIT) {
                    model_r.pos += 8;
                } else if (gwire == PB_32BIT) {
                    model_r.pos += 4;
                }
            }
        } else if (field == ONNX_MODEL_OPSET_IMPORT && wire == PB_LEN) {
            size_t len = (size_t)pb_read_varint(&model_r);
            size_t sub_end = model_r.pos + len;
            // Parse opset version
            while (model_r.pos < sub_end) {
                uint64_t otag = pb_read_varint(&model_r);
                uint32_t owire = (uint32_t)(otag & 7);
                if (owire == PB_VARINT) {
                    ctx->opset_version = (int64_t)pb_read_varint(&model_r);
                } else if (owire == PB_LEN) {
                    size_t slen = (size_t)pb_read_varint(&model_r);
                    model_r.pos += slen;
                } else {
                    break;
                }
            }
            model_r.pos = sub_end;
        } else {
            // Skip
            if (wire == PB_VARINT) pb_read_varint(&model_r);
            else if (wire == PB_64BIT) model_r.pos += 8;
            else if (wire == PB_32BIT) model_r.pos += 4;
            else if (wire == PB_LEN) {
                size_t len = (size_t)pb_read_varint(&model_r);
                model_r.pos += len;
            } else break;
        }
    }

    printf("[ONNX] Loaded: IR v%lld | Opset %lld | Producer: %s\n",
           (long long)ctx->ir_version,
           (long long)ctx->opset_version,
           ctx->producer ? ctx->producer : "unknown");
    printf("[ONNX] Nodes: %d | Initializers: %d\n",
           ctx->n_nodes, ctx->n_initializers);

    return ctx;
}

void oag_onnx_free(oag_onnx_ctx_t* ctx) {
    if (!ctx) return;

    for (int i = 0; i < ctx->n_nodes; i++) {
        free(ctx->nodes[i].op_type);
        free(ctx->nodes[i].name);
        for (int j = 0; j < ctx->nodes[i].n_inputs; j++) free(ctx->nodes[i].inputs[j]);
        for (int j = 0; j < ctx->nodes[i].n_outputs; j++) free(ctx->nodes[i].outputs[j]);
        free(ctx->nodes[i].inputs);
        free(ctx->nodes[i].outputs);
    }
    free(ctx->nodes);

    for (int i = 0; i < ctx->n_initializers; i++) {
        free(ctx->initializers[i].name);
    }
    free(ctx->initializers);

    free(ctx->input_names);
    free(ctx->output_names);
    free(ctx->producer);
    free(ctx->domain);

    if (ctx->mmap_base) onnx_munmap(ctx->mmap_base, ctx->mmap_size);
    free(ctx);
}

const oag_onnx_tensor_t* oag_onnx_find_initializer(const oag_onnx_ctx_t* ctx, const char* name) {
    for (int i = 0; i < ctx->n_initializers; i++) {
        if (ctx->initializers[i].name && strcmp(ctx->initializers[i].name, name) == 0) {
            return &ctx->initializers[i];
        }
    }
    return NULL;
}

oag_tensor_t* oag_onnx_to_tensor(const oag_onnx_tensor_t* onnx_t) {
    if (!onnx_t || !onnx_t->raw_data) return NULL;

    // Clamp to max 4 dims (oag_tensor_t supports max 4)
    int n_dims = onnx_t->n_dims;
    if (n_dims > 4) n_dims = 4;

    int64_t shape[4] = {1, 1, 1, 1};
    for (int i = 0; i < n_dims; i++) {
        shape[i] = onnx_t->dims[i];
    }

    size_t n_elem = 1;
    for (int i = 0; i < onnx_t->n_dims; i++) n_elem *= (size_t)onnx_t->dims[i];

    oag_tensor_t* t = oag_tensor_alloc(onnx_t->n_dims, shape);

    switch (onnx_t->dtype) {
        case OAG_ONNX_FLOAT:
            memcpy(t->data, onnx_t->raw_data, n_elem * sizeof(float));
            break;
        case OAG_ONNX_FLOAT16: {
            const uint16_t* f16 = (const uint16_t*)onnx_t->raw_data;
            for (size_t i = 0; i < n_elem; i++) {
                uint32_t sign = (f16[i] >> 15) & 1;
                uint32_t exp  = (f16[i] >> 10) & 0x1f;
                uint32_t mant = f16[i] & 0x3ff;
                uint32_t f32;
                if (exp == 0) f32 = sign << 31;
                else if (exp == 31) f32 = (sign << 31) | 0x7F800000;
                else f32 = (sign << 31) | ((exp + 112) << 23) | (mant << 13);
                memcpy(&t->data[i], &f32, 4);
            }
            break;
        }
        default:
            fprintf(stderr, "[ONNX] Unsupported dtype: %d\n", onnx_t->dtype);
            memset(t->data, 0, n_elem * sizeof(float));
            break;
    }

    return t;
}

void oag_onnx_print_summary(const oag_onnx_ctx_t* ctx) {
    printf("╔══════════════════════════════════════╗\n");
    printf("║       ONNX Model Summary             ║\n");
    printf("╠══════════════════════════════════════╣\n");
    printf("║ IR Version:   %lld                    ║\n", (long long)ctx->ir_version);
    printf("║ Opset:        %lld                    ║\n", (long long)ctx->opset_version);
    printf("║ Producer:     %-20s  ║\n", ctx->producer ? ctx->producer : "unknown");
    printf("║ Nodes:        %d                      ║\n", ctx->n_nodes);
    printf("║ Weights:      %d                      ║\n", ctx->n_initializers);
    printf("╚══════════════════════════════════════╝\n");

    // Op type histogram
    printf("\nOperator distribution:\n");
    // Count top ops
    typedef struct { const char* op; int count; } op_count_t;
    op_count_t ops[64];
    int n_ops = 0;

    for (int i = 0; i < ctx->n_nodes; i++) {
        if (!ctx->nodes[i].op_type) continue;
        bool found = false;
        for (int j = 0; j < n_ops; j++) {
            if (strcmp(ops[j].op, ctx->nodes[i].op_type) == 0) {
                ops[j].count++;
                found = true;
                break;
            }
        }
        if (!found && n_ops < 64) {
            ops[n_ops].op = ctx->nodes[i].op_type;
            ops[n_ops].count = 1;
            n_ops++;
        }
    }

    for (int i = 0; i < n_ops; i++) {
        printf("  %-24s %d\n", ops[i].op, ops[i].count);
    }
}

// ============================================================
// Format detection
// ============================================================

oag_model_format_t oag_detect_model_format(const char* path) {
    FILE* f = fopen(path, "rb");
    if (!f) return OAG_FORMAT_UNKNOWN;

    uint8_t header[4];
    size_t n = fread(header, 1, 4, f);
    fclose(f);

    if (n < 4) return OAG_FORMAT_UNKNOWN;

    // GGUF magic: 'G','G','U','F' as LE uint32 = 0x46554747
    if (header[0] == 'G' && header[1] == 'G' && header[2] == 'U' && header[3] == 'F') {
        return OAG_FORMAT_GGUF;
    }

    // ONNX (protobuf): starts with 0x08 (field 1, varint = ir_version)
    // or 0x12 (field 2, length-delimited = producer_name)
    if (header[0] == 0x08 || header[0] == 0x12) {
        // Check file extension for confirmation
        const char* ext = strrchr(path, '.');
        if (ext && strcmp(ext, ".onnx") == 0) {
            return OAG_FORMAT_ONNX;
        }
        // Heuristic: if field 1 varint is small, likely ONNX
        if (header[0] == 0x08 && header[1] < 20) {
            return OAG_FORMAT_ONNX;
        }
    }

    return OAG_FORMAT_UNKNOWN;
}
