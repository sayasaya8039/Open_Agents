// Open_Agents v0.1.0 — Local AI Coding Engine
// C フルスクラッチ | GGUF + ONNX | GPU/CPU/NPU 並行 | Vulkan | Multi-NIC
//
// Usage:
//   open_agents --model <path.gguf|path.onnx> [--port 8080] [--server] [--vulkan]
//   open_agents --help
//
// Backends: CPU(AVX2+FMA), CUDA, NPU(Intel), WASM, Mojo, Julia, MLX
// Formats:  GGUF (llama.cpp compatible), ONNX (universal)
// Network:  Multi-NIC bonding (Ethernet + WiFi parallel)
// Render:   Vulkan (GPU-accelerated UI)

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>

#include "core/gguf.h"
#include "core/onnx_loader.h"
#include "core/model.h"
#include "core/inference.h"
#include "core/tokenizer.h"
#include "core/sampler.h"
#include "backend/backend.h"
#include "server/http_server.h"
#include "net/multi_nic.h"
#include "render/vulkan_render.h"
#include "ui/tui.h"

#ifndef OAG_VERSION
#define OAG_VERSION "0.1.0"
#endif

// ============================================================
// CLI argument parsing
// ============================================================

typedef struct {
    const char* model_path;
    int         port;
    bool        server_mode;
    bool        vulkan_mode;
    bool        interactive;
    bool        show_help;
    bool        list_nics;
    bool        list_gpus;
    bool        tui_mode;
    const char* edit_file;
    float       temperature;
    int         max_tokens;
    int         n_threads;
    const char* prompt;
} cli_args_t;

static cli_args_t parse_args(int argc, char** argv) {
    cli_args_t args = {
        .model_path  = NULL,
        .port        = 8080,
        .server_mode = false,
        .vulkan_mode = false,
        .interactive = true,
        .show_help   = false,
        .list_nics   = false,
        .list_gpus   = false,
        .tui_mode    = false,
        .edit_file   = NULL,
        .temperature = 0.7f,
        .max_tokens  = 512,
        .n_threads   = 0,
        .prompt      = NULL,
    };

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            args.show_help = true;
        } else if (strcmp(argv[i], "--model") == 0 || strcmp(argv[i], "-m") == 0) {
            if (i + 1 < argc) args.model_path = argv[++i];
        } else if (strcmp(argv[i], "--port") == 0 || strcmp(argv[i], "-p") == 0) {
            if (i + 1 < argc) args.port = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--server") == 0) {
            args.server_mode = true;
        } else if (strcmp(argv[i], "--vulkan") == 0) {
            args.vulkan_mode = true;
        } else if (strcmp(argv[i], "--temp") == 0) {
            if (i + 1 < argc) args.temperature = (float)atof(argv[++i]);
        } else if (strcmp(argv[i], "--max-tokens") == 0) {
            if (i + 1 < argc) args.max_tokens = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--threads") == 0 || strcmp(argv[i], "-t") == 0) {
            if (i + 1 < argc) args.n_threads = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--prompt") == 0) {
            if (i + 1 < argc) { args.prompt = argv[++i]; args.interactive = false; }
        } else if (strcmp(argv[i], "--nics") == 0) {
            args.list_nics = true;
        } else if (strcmp(argv[i], "--gpus") == 0) {
            args.list_gpus = true;
        } else if (strcmp(argv[i], "--tui") == 0 || strcmp(argv[i], "--edit") == 0) {
            args.tui_mode = true;
            if (i + 1 < argc && argv[i+1][0] != '-') args.edit_file = argv[++i];
        } else if (argv[i][0] != '-' && !args.model_path) {
            args.model_path = argv[i];
        }
    }

    return args;
}

static void print_usage(void) {
    printf(
        "╔══════════════════════════════════════════════════════════╗\n"
        "║  Open_Agents v%s — Local AI Coding Engine            ║\n"
        "║  C Full-scratch | GGUF+ONNX | GPU/CPU/NPU | Vulkan     ║\n"
        "╚══════════════════════════════════════════════════════════╝\n"
        "\n"
        "Usage:\n"
        "  open_agents --model <model.gguf|model.onnx> [options]\n"
        "\n"
        "Options:\n"
        "  -m, --model <path>    Model file (GGUF or ONNX)\n"
        "  -p, --port <port>     API server port (default: 8080)\n"
        "  --server              Run as OpenAI-compatible API server\n"
        "  --vulkan              Enable Vulkan GPU rendering\n"
        "  --prompt <text>       Single prompt (non-interactive)\n"
        "  --temp <float>        Temperature (default: 0.7)\n"
        "  --max-tokens <n>      Max tokens to generate (default: 512)\n"
        "  -t, --threads <n>     CPU threads (default: auto)\n"
        "  --tui [file]          TUI code editor + AI chat (split pane)\n"
        "  --edit [file]         Same as --tui\n"
        "  --gpus                Detect GPUs (NVIDIA→CUDA, AMD→DirectML)\n"
        "  --nics                List network interfaces\n"
        "  -h, --help            Show this help\n"
        "\n"
        "Supported formats:\n"
        "  GGUF  — llama.cpp compatible (Q4_K, Q5_K, Q8_0, F16, F32)\n"
        "  ONNX  — Universal (FP32, FP16, INT8)\n"
        "\n"
        "Compute backends:\n"
        "  CPU   — AVX2 + FMA + Multi-threaded (always available)\n"
        "  CUDA  — NVIDIA GPU acceleration\n"
        "  NPU   — Intel AI Boost (OpenVINO)\n"
        "  WASM  — WebAssembly runtime\n"
        "  Mojo  — SIMD/GPU compute via Mojo FFI\n"
        "  Julia — Scientific compute via Julia FFI\n"
        "\n"
        "Examples:\n"
        "  open_agents -m qwen2.5-coder-7b-q4_k.gguf\n"
        "  open_agents -m model.onnx --server --port 8080\n"
        "  open_agents -m model.gguf --prompt \"Write a sort function\"\n"
        "  open_agents --nics\n"
        "\n",
        OAG_VERSION
    );
}

// ============================================================
// Interactive REPL
// ============================================================

static void run_interactive(oag_inference_t* inf, float temperature, int max_tokens) {
    printf("\n  Open_Agents Interactive Mode (type 'exit' to quit)\n");
    printf("  Model: %s | Backend: CPU", inf->model->arch);
    if (inf->devices.use_gpu) printf("+GPU");
    if (inf->devices.use_npu) printf("+NPU");
    printf("\n\n");

    char input[4096];
    while (1) {
        printf(">>> ");
        fflush(stdout);

        if (!fgets(input, sizeof(input), stdin)) break;

        // Remove trailing newline
        size_t len = strlen(input);
        if (len > 0 && input[len - 1] == '\n') input[--len] = '\0';
        if (len == 0) continue;

        if (strcmp(input, "exit") == 0 || strcmp(input, "quit") == 0) break;
        if (strcmp(input, "/stats") == 0) {
            oag_inference_print_stats(inf);
            continue;
        }
        if (strcmp(input, "/clear") == 0) {
            oag_kv_cache_reset(inf->model->kv_cache);
            printf("  [KV cache cleared]\n");
            continue;
        }

        // Stream callback
        oag_chat_msg_t messages[1] = {
            { .role = "user", .content = input }
        };

        oag_chat_params_t params = {
            .messages   = messages,
            .n_messages = 1,
            .sampler    = oag_sampler_default_params(),
            .max_tokens = max_tokens,
            .stream     = false,
            .on_token   = NULL,
            .user_data  = NULL,
        };
        params.sampler.temperature = temperature;

        char* response = oag_inference_chat(inf, params);
        if (response) {
            printf("\n%s\n\n", response);
            free(response);
        }
    }
}

// ============================================================
// Main
// ============================================================

int main(int argc, char** argv) {
    cli_args_t args = parse_args(argc, argv);

    if (args.show_help || (!args.model_path && !args.list_nics && !args.list_gpus && !args.tui_mode)) {
        print_usage();
        return 0;
    }

    // List GPUs
    if (args.list_gpus) {
        oag_gpu_detect_t det = oag_gpu_detect();
        oag_gpu_detect_print(&det);
        if (!args.model_path && !args.list_nics) return 0;
    }

    // List NICs
    if (args.list_nics) {
        oag_multi_nic_t* mn = oag_multi_nic_create();
        oag_multi_nic_print(mn);
        oag_multi_nic_free(mn);
        if (!args.model_path) return 0;
    }

    // Detect model format
    printf("[Open_Agents] Loading model: %s\n", args.model_path);
    oag_model_format_t fmt = oag_detect_model_format(args.model_path);

    switch (fmt) {
        case OAG_FORMAT_GGUF:
            printf("[Open_Agents] Format: GGUF\n");
            break;
        case OAG_FORMAT_ONNX:
            printf("[Open_Agents] Format: ONNX\n");
            break;
        default:
            fprintf(stderr, "[Open_Agents] Unknown model format\n");
            return 1;
    }

    // Initialize Vulkan (optional)
    oag_vk_ctx_t* vk = NULL;
    if (args.vulkan_mode) {
        oag_vk_config_t vk_config = oag_vk_default_config();
        vk = oag_vk_create(vk_config);
    }

    // Create inference engine
    oag_inference_t* inf = NULL;

    if (fmt == OAG_FORMAT_GGUF) {
        inf = oag_inference_create(args.model_path);
    } else if (fmt == OAG_FORMAT_ONNX) {
        // ONNX path: load separately and wrap
        oag_onnx_ctx_t* onnx = oag_onnx_load(args.model_path);
        if (onnx) {
            oag_onnx_print_summary(onnx);
            // TODO: Create inference engine from ONNX graph
            // For now, print info and exit
            printf("[Open_Agents] ONNX inference pipeline coming soon\n");
            oag_onnx_free(onnx);
        }
        // Fallback message
        if (!inf) {
            printf("[Open_Agents] ONNX runtime not yet complete. Use GGUF format.\n");
            if (vk) oag_vk_destroy(vk);
            return 1;
        }
    }

    if (!inf) {
        fprintf(stderr, "[Open_Agents] Failed to initialize inference engine\n");
        if (vk) oag_vk_destroy(vk);
        return 1;
    }

    // TUI mode (code editor + AI chat) — works with or without model
    if (args.tui_mode) {
        oag_tui_t* tui = oag_tui_create(oag_tui_default_config());
        if (tui) {
            if (inf) oag_tui_set_inference(tui, inf);

            oag_provider_registry_t* providers = oag_provider_registry_create();
            oag_provider_auto_detect(providers);
            if (providers->n_providers > 0) {
                oag_tui_set_provider(tui, providers);
            }

            if (args.edit_file) {
                oag_tui_open_file(tui, args.edit_file);
            }

            oag_tui_run(tui);
            oag_tui_free(tui);
            oag_provider_registry_free(providers);
        }
        oag_inference_free(inf);
        if (vk) oag_vk_destroy(vk);
        return 0;
    }
    // Server mode
    if (args.server_mode) {
        oag_server_config_t srv_config = oag_server_default_config();
        srv_config.port = args.port;
        oag_server_t* srv = oag_server_create(inf, srv_config);
        if (srv) {
            printf("[Open_Agents] Server mode — Ctrl+C to stop\n");
            oag_server_run(srv);  // blocking
            oag_server_free(srv);
        }
    }
    // Single prompt mode
    else if (args.prompt) {
        oag_chat_msg_t messages[1] = {
            { .role = "user", .content = args.prompt }
        };

        oag_chat_params_t params = {
            .messages   = messages,
            .n_messages = 1,
            .sampler    = oag_sampler_default_params(),
            .max_tokens = args.max_tokens,
            .stream     = false,
        };
        params.sampler.temperature = args.temperature;

        char* response = oag_inference_chat(inf, params);
        if (response) {
            printf("%s\n", response);
            free(response);
        }
        oag_inference_print_stats(inf);
    }
    // Interactive REPL
    else {
        run_interactive(inf, args.temperature, args.max_tokens);
        oag_inference_print_stats(inf);
    }

    // Cleanup
    oag_inference_free(inf);
    if (vk) oag_vk_destroy(vk);

    return 0;
}
