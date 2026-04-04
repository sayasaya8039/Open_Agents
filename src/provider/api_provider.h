// Open_Agents — AI Provider Abstraction (Any API Key → Any AI)
// Supports: OpenAI, Anthropic, Google, xAI, Mistral, OpenRouter, HuggingFace,
//           Groq, Together, DeepSeek, local (llama.cpp/ollama), custom
#ifndef OAG_API_PROVIDER_H
#define OAG_API_PROVIDER_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#define OAG_MAX_API_KEY_LEN  256
#define OAG_MAX_PROVIDERS    16

typedef enum {
    OAG_PROVIDER_LOCAL       = 0,   // Built-in GGUF/ONNX inference
    OAG_PROVIDER_OPENAI      = 1,   // OpenAI (GPT-4o, o3, etc.)
    OAG_PROVIDER_ANTHROPIC   = 2,   // Anthropic (Claude)
    OAG_PROVIDER_GOOGLE      = 3,   // Google (Gemini)
    OAG_PROVIDER_XAI         = 4,   // xAI (Grok)
    OAG_PROVIDER_MISTRAL     = 5,   // Mistral
    OAG_PROVIDER_OPENROUTER  = 6,   // OpenRouter (multi-model gateway)
    OAG_PROVIDER_HUGGINGFACE = 7,   // HuggingFace Inference API
    OAG_PROVIDER_GROQ        = 8,   // Groq (ultra-fast)
    OAG_PROVIDER_TOGETHER    = 9,   // Together AI
    OAG_PROVIDER_DEEPSEEK    = 10,  // DeepSeek
    OAG_PROVIDER_OLLAMA      = 11,  // Ollama (local server)
    OAG_PROVIDER_CUSTOM      = 12,  // Custom OpenAI-compatible endpoint
    OAG_PROVIDER_COUNT
} oag_provider_type_t;

// Provider configuration
typedef struct {
    oag_provider_type_t type;
    char                api_key[OAG_MAX_API_KEY_LEN];
    char                base_url[256];
    char                model[128];      // e.g. "gpt-4o", "claude-sonnet-4-6"
    char                org_id[128];     // optional org ID
    float               temperature;
    int32_t             max_tokens;
    float               top_p;
    bool                stream;
    int32_t             timeout_ms;      // request timeout
} oag_provider_config_t;

// Chat message
typedef struct {
    const char* role;     // "system", "user", "assistant"
    const char* content;
} oag_api_message_t;

// Chat completion request
typedef struct {
    oag_api_message_t* messages;
    int32_t            n_messages;
    const char*        model;       // override config model
    float              temperature; // -1 = use config default
    int32_t            max_tokens;  // -1 = use config default
    bool               stream;
    void               (*on_chunk)(const char* text, void* user);
    void*              user_data;
} oag_api_request_t;

// Chat completion response
typedef struct {
    char*    content;         // response text (caller frees)
    char*    model;           // actual model used
    int32_t  prompt_tokens;
    int32_t  completion_tokens;
    int32_t  total_tokens;
    double   latency_ms;
    bool     success;
    char*    error;           // error message if !success
} oag_api_response_t;

// Provider vtable
typedef struct oag_provider oag_provider_t;

typedef struct {
    const char*         name;
    oag_provider_type_t type;

    bool (*init)(oag_provider_t* p, const oag_provider_config_t* config);
    void (*shutdown)(oag_provider_t* p);

    // Synchronous chat completion
    oag_api_response_t (*chat)(oag_provider_t* p, const oag_api_request_t* req);

    // List available models
    char** (*list_models)(oag_provider_t* p, int32_t* out_count);
} oag_provider_vtable_t;

struct oag_provider {
    const oag_provider_vtable_t* vt;
    oag_provider_config_t        config;
    void*                        ctx;   // provider-specific context
};

// ============================================================
// Provider registry (multi-provider support)
// ============================================================

typedef struct {
    oag_provider_t* providers[OAG_MAX_PROVIDERS];
    int32_t         n_providers;
    int32_t         active_idx;    // currently selected provider
} oag_provider_registry_t;

// Create registry
oag_provider_registry_t* oag_provider_registry_create(void);
void                     oag_provider_registry_free(oag_provider_registry_t* reg);

// Add provider
bool oag_provider_add(oag_provider_registry_t* reg, oag_provider_config_t config);

// Set active provider
bool oag_provider_set_active(oag_provider_registry_t* reg, int32_t idx);
bool oag_provider_set_active_by_type(oag_provider_registry_t* reg, oag_provider_type_t type);

// Get active provider
oag_provider_t* oag_provider_get_active(oag_provider_registry_t* reg);

// Chat with active provider
oag_api_response_t oag_provider_chat(oag_provider_registry_t* reg,
                                      const oag_api_request_t* req);

// Convenience: create provider from env var
// Reads OPENAI_API_KEY, ANTHROPIC_API_KEY, etc.
int32_t oag_provider_auto_detect(oag_provider_registry_t* reg);

// Provider config helpers
oag_provider_config_t oag_provider_config_openai(const char* api_key, const char* model);
oag_provider_config_t oag_provider_config_anthropic(const char* api_key, const char* model);
oag_provider_config_t oag_provider_config_google(const char* api_key, const char* model);
oag_provider_config_t oag_provider_config_xai(const char* api_key, const char* model);
oag_provider_config_t oag_provider_config_openrouter(const char* api_key, const char* model);
oag_provider_config_t oag_provider_config_groq(const char* api_key, const char* model);
oag_provider_config_t oag_provider_config_deepseek(const char* api_key, const char* model);
oag_provider_config_t oag_provider_config_ollama(const char* model);
oag_provider_config_t oag_provider_config_custom(const char* base_url, const char* api_key, const char* model);

// Free response
void oag_api_response_free(oag_api_response_t* resp);

// Provider name
const char* oag_provider_type_name(oag_provider_type_t type);

#endif // OAG_API_PROVIDER_H
