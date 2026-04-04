// Open_Agents — ONNX Model Loader
// Supports ONNX format alongside GGUF for maximum model compatibility
#ifndef OAG_ONNX_LOADER_H
#define OAG_ONNX_LOADER_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include "core/tensor.h"

// ONNX data types (from onnx.proto3)
typedef enum {
    OAG_ONNX_FLOAT   = 1,
    OAG_ONNX_UINT8   = 2,
    OAG_ONNX_INT8    = 3,
    OAG_ONNX_UINT16  = 4,
    OAG_ONNX_INT16   = 5,
    OAG_ONNX_INT32   = 6,
    OAG_ONNX_INT64   = 7,
    OAG_ONNX_FLOAT16 = 10,
    OAG_ONNX_DOUBLE  = 11,
    OAG_ONNX_UINT32  = 12,
    OAG_ONNX_UINT64  = 13,
    OAG_ONNX_BFLOAT16 = 16,
} oag_onnx_dtype_t;

// ONNX tensor
typedef struct {
    char*           name;
    oag_onnx_dtype_t dtype;
    int             n_dims;
    int64_t         dims[8];
    void*           raw_data;
    size_t          raw_size;
} oag_onnx_tensor_t;

// ONNX graph node (operator)
typedef struct {
    char*   op_type;       // e.g. "MatMul", "Add", "Relu", "LayerNormalization"
    char*   name;
    char**  inputs;
    int     n_inputs;
    char**  outputs;
    int     n_outputs;
} oag_onnx_node_t;

// ONNX model context
typedef struct {
    int64_t           ir_version;
    int64_t           opset_version;
    char*             producer;
    char*             domain;

    // Graph
    oag_onnx_node_t*   nodes;
    int                n_nodes;

    // Initializers (weights)
    oag_onnx_tensor_t* initializers;
    int                n_initializers;

    // Graph inputs/outputs
    char**             input_names;
    int                n_inputs;
    char**             output_names;
    int                n_outputs;

    // Memory-mapped file
    void*              mmap_base;
    size_t             mmap_size;
} oag_onnx_ctx_t;

// Load ONNX model (protobuf parsing)
oag_onnx_ctx_t* oag_onnx_load(const char* path);
void            oag_onnx_free(oag_onnx_ctx_t* ctx);

// Find initializer (weight) by name
const oag_onnx_tensor_t* oag_onnx_find_initializer(const oag_onnx_ctx_t* ctx, const char* name);

// Convert ONNX tensor to float32 oag_tensor
oag_tensor_t* oag_onnx_to_tensor(const oag_onnx_tensor_t* onnx_t);

// Print model summary
void oag_onnx_print_summary(const oag_onnx_ctx_t* ctx);

// ============================================================
// Unified model format detection
// ============================================================

typedef enum {
    OAG_FORMAT_UNKNOWN = 0,
    OAG_FORMAT_GGUF    = 1,
    OAG_FORMAT_ONNX    = 2,
} oag_model_format_t;

// Detect format from file header
oag_model_format_t oag_detect_model_format(const char* path);

#endif // OAG_ONNX_LOADER_H
