// Open_Agents — AI Provider (HTTP-based API calls to any AI service)
#include "provider/api_provider.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
#include <winhttp.h>
#pragma comment(lib, "winhttp.lib")
#else
// Unix: use libcurl or raw sockets
#endif

// ============================================================
// Provider base URLs
// ============================================================

static const char* DEFAULT_URLS[] = {
    [OAG_PROVIDER_LOCAL]       = "http://localhost:8080",
    [OAG_PROVIDER_OPENAI]      = "https://api.openai.com",
    [OAG_PROVIDER_ANTHROPIC]   = "https://api.anthropic.com",
    [OAG_PROVIDER_GOOGLE]      = "https://generativelanguage.googleapis.com",
    [OAG_PROVIDER_XAI]         = "https://api.x.ai",
    [OAG_PROVIDER_MISTRAL]     = "https://api.mistral.ai",
    [OAG_PROVIDER_OPENROUTER]  = "https://openrouter.ai/api",
    [OAG_PROVIDER_HUGGINGFACE] = "https://api-inference.huggingface.co",
    [OAG_PROVIDER_GROQ]        = "https://api.groq.com/openai",
    [OAG_PROVIDER_TOGETHER]    = "https://api.together.xyz",
    [OAG_PROVIDER_DEEPSEEK]    = "https://api.deepseek.com",
    [OAG_PROVIDER_OLLAMA]      = "http://localhost:11434",
};

static const char* DEFAULT_MODELS[] = {
    [OAG_PROVIDER_LOCAL]       = "local",
    [OAG_PROVIDER_OPENAI]      = "gpt-4o",
    [OAG_PROVIDER_ANTHROPIC]   = "claude-sonnet-4-6-20250514",
    [OAG_PROVIDER_GOOGLE]      = "gemini-2.5-pro",
    [OAG_PROVIDER_XAI]         = "grok-3",
    [OAG_PROVIDER_MISTRAL]     = "mistral-large-latest",
    [OAG_PROVIDER_OPENROUTER]  = "anthropic/claude-sonnet-4-6",
    [OAG_PROVIDER_HUGGINGFACE] = "meta-llama/Llama-3.3-70B-Instruct",
    [OAG_PROVIDER_GROQ]        = "llama-3.3-70b-versatile",
    [OAG_PROVIDER_TOGETHER]    = "meta-llama/Llama-3.3-70B-Instruct-Turbo",
    [OAG_PROVIDER_DEEPSEEK]    = "deepseek-chat",
    [OAG_PROVIDER_OLLAMA]      = "qwen2.5-coder:7b",
};

// ============================================================
// Environment variable names for auto-detection
// ============================================================

static const struct {
    oag_provider_type_t type;
    const char* env_var;
} ENV_KEYS[] = {
    { OAG_PROVIDER_OPENAI,      "OPENAI_API_KEY" },
    { OAG_PROVIDER_ANTHROPIC,   "ANTHROPIC_API_KEY" },
    { OAG_PROVIDER_GOOGLE,      "GOOGLE_API_KEY" },
    { OAG_PROVIDER_XAI,         "XAI_API_KEY" },
    { OAG_PROVIDER_MISTRAL,     "MISTRAL_API_KEY" },
    { OAG_PROVIDER_OPENROUTER,  "OPENROUTER_API_KEY" },
    { OAG_PROVIDER_HUGGINGFACE, "HF_API_KEY" },
    { OAG_PROVIDER_GROQ,        "GROQ_API_KEY" },
    { OAG_PROVIDER_TOGETHER,    "TOGETHER_API_KEY" },
    { OAG_PROVIDER_DEEPSEEK,    "DEEPSEEK_API_KEY" },
};

// ============================================================
// HTTP client (WinHTTP on Windows)
// ============================================================

#ifdef _WIN32
typedef struct {
    char* body;
    size_t len;
    int status_code;
} http_result_t;

static http_result_t http_post(const char* url, const char* auth_header,
                                const char* extra_headers,
                                const char* body, int timeout_ms) {
    http_result_t result = { .body = NULL, .len = 0, .status_code = 0 };

    // Parse URL
    wchar_t wurl[512];
    MultiByteToWideChar(CP_UTF8, 0, url, -1, wurl, 512);

    URL_COMPONENTS uc;
    memset(&uc, 0, sizeof(uc));
    uc.dwStructSize = sizeof(uc);
    wchar_t host[256], path[256];
    uc.lpszHostName = host;
    uc.dwHostNameLength = 256;
    uc.lpszUrlPath = path;
    uc.dwUrlPathLength = 256;
    WinHttpCrackUrl(wurl, 0, 0, &uc);

    HINTERNET session = WinHttpOpen(L"Open_Agents/0.1",
                                     WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                                     WINHTTP_NO_PROXY_NAME,
                                     WINHTTP_NO_PROXY_BYPASS, 0);
    if (!session) return result;

    if (timeout_ms > 0) {
        WinHttpSetTimeouts(session, timeout_ms, timeout_ms, timeout_ms, timeout_ms);
    }

    HINTERNET connect = WinHttpConnect(session, host, uc.nPort, 0);
    if (!connect) { WinHttpCloseHandle(session); return result; }

    DWORD flags = (uc.nScheme == INTERNET_SCHEME_HTTPS) ? WINHTTP_FLAG_SECURE : 0;
    HINTERNET request = WinHttpOpenRequest(connect, L"POST", path,
                                            NULL, WINHTTP_NO_REFERER,
                                            WINHTTP_DEFAULT_ACCEPT_TYPES, flags);
    if (!request) {
        WinHttpCloseHandle(connect);
        WinHttpCloseHandle(session);
        return result;
    }

    // Add headers
    wchar_t wheaders[1024];
    swprintf(wheaders, 1024, L"Content-Type: application/json\r\n%hs",
             auth_header ? auth_header : "");
    WinHttpAddRequestHeaders(request, wheaders, -1, WINHTTP_ADDREQ_FLAG_ADD);

    if (extra_headers) {
        wchar_t wextra[512];
        MultiByteToWideChar(CP_UTF8, 0, extra_headers, -1, wextra, 512);
        WinHttpAddRequestHeaders(request, wextra, -1, WINHTTP_ADDREQ_FLAG_ADD);
    }

    // Send
    DWORD body_len = body ? (DWORD)strlen(body) : 0;
    BOOL sent = WinHttpSendRequest(request, WINHTTP_NO_ADDITIONAL_HEADERS, 0,
                                    (LPVOID)body, body_len, body_len, 0);
    if (!sent || !WinHttpReceiveResponse(request, NULL)) {
        WinHttpCloseHandle(request);
        WinHttpCloseHandle(connect);
        WinHttpCloseHandle(session);
        return result;
    }

    // Get status code
    DWORD status_code = 0;
    DWORD sc_size = sizeof(status_code);
    WinHttpQueryHeaders(request, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                        WINHTTP_HEADER_NAME_BY_INDEX, &status_code, &sc_size,
                        WINHTTP_NO_HEADER_INDEX);
    result.status_code = (int)status_code;

    // Read response body
    size_t buf_size = 65536;
    result.body = (char*)malloc(buf_size);
    result.len = 0;

    DWORD bytes_read;
    while (WinHttpReadData(request, result.body + result.len,
                           (DWORD)(buf_size - result.len - 1), &bytes_read) && bytes_read > 0) {
        result.len += bytes_read;
        if (result.len + 8192 >= buf_size) {
            buf_size *= 2;
            result.body = (char*)realloc(result.body, buf_size);
        }
    }
    result.body[result.len] = '\0';

    WinHttpCloseHandle(request);
    WinHttpCloseHandle(connect);
    WinHttpCloseHandle(session);
    return result;
}
#endif

// ============================================================
// Build JSON request body
// ============================================================

static char* build_openai_request(const oag_api_request_t* req,
                                   const oag_provider_config_t* config) {
    const char* model = req->model ? req->model : config->model;
    float temp = (req->temperature >= 0) ? req->temperature : config->temperature;
    int max_tok = (req->max_tokens > 0) ? req->max_tokens : config->max_tokens;

    // Calculate needed buffer
    size_t buf_size = 4096;
    for (int i = 0; i < req->n_messages; i++) {
        buf_size += strlen(req->messages[i].content) + 128;
    }

    char* buf = (char*)malloc(buf_size);
    size_t pos = 0;

    pos += snprintf(buf + pos, buf_size - pos,
        "{\"model\":\"%s\",\"temperature\":%.2f,\"max_tokens\":%d,"
        "\"stream\":%s,\"messages\":[",
        model, temp, max_tok, req->stream ? "true" : "false");

    for (int i = 0; i < req->n_messages; i++) {
        if (i > 0) buf[pos++] = ',';

        // Escape content
        const char* content = req->messages[i].content;
        pos += snprintf(buf + pos, buf_size - pos,
            "{\"role\":\"%s\",\"content\":\"", req->messages[i].role);

        for (size_t j = 0; content[j] && pos < buf_size - 10; j++) {
            if (content[j] == '"')       { buf[pos++] = '\\'; buf[pos++] = '"'; }
            else if (content[j] == '\\') { buf[pos++] = '\\'; buf[pos++] = '\\'; }
            else if (content[j] == '\n') { buf[pos++] = '\\'; buf[pos++] = 'n'; }
            else if (content[j] == '\r') { buf[pos++] = '\\'; buf[pos++] = 'r'; }
            else if (content[j] == '\t') { buf[pos++] = '\\'; buf[pos++] = 't'; }
            else buf[pos++] = content[j];
        }

        pos += snprintf(buf + pos, buf_size - pos, "\"}");
    }

    pos += snprintf(buf + pos, buf_size - pos, "]}");
    return buf;
}

static char* build_anthropic_request(const oag_api_request_t* req,
                                      const oag_provider_config_t* config) {
    const char* model = req->model ? req->model : config->model;
    int max_tok = (req->max_tokens > 0) ? req->max_tokens : config->max_tokens;

    size_t buf_size = 4096;
    for (int i = 0; i < req->n_messages; i++) {
        buf_size += strlen(req->messages[i].content) + 128;
    }

    char* buf = (char*)malloc(buf_size);
    size_t pos = 0;

    // Anthropic Messages API format
    pos += snprintf(buf + pos, buf_size - pos,
        "{\"model\":\"%s\",\"max_tokens\":%d,\"messages\":[", model, max_tok);

    for (int i = 0; i < req->n_messages; i++) {
        if (strcmp(req->messages[i].role, "system") == 0) continue;  // system handled separately
        if (i > 0) buf[pos++] = ',';

        pos += snprintf(buf + pos, buf_size - pos,
            "{\"role\":\"%s\",\"content\":\"%s\"}",
            req->messages[i].role, req->messages[i].content);
    }

    pos += snprintf(buf + pos, buf_size - pos, "]");

    // Add system message if present
    for (int i = 0; i < req->n_messages; i++) {
        if (strcmp(req->messages[i].role, "system") == 0) {
            pos += snprintf(buf + pos, buf_size - pos,
                ",\"system\":\"%s\"", req->messages[i].content);
            break;
        }
    }

    pos += snprintf(buf + pos, buf_size - pos, "}");
    return buf;
}

// ============================================================
// Parse JSON response (minimal parser)
// ============================================================

static char* extract_json_string(const char* json, const char* key) {
    char pattern[128];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char* pos = strstr(json, pattern);
    if (!pos) return NULL;

    pos = strchr(pos + strlen(pattern), ':');
    if (!pos) return NULL;
    pos++;
    while (*pos == ' ' || *pos == '\t' || *pos == '\n') pos++;
    if (*pos != '"') return NULL;
    pos++;

    // Find closing quote (handle escapes)
    size_t len = 0;
    const char* start = pos;
    while (pos[len] && !(pos[len] == '"' && (len == 0 || pos[len-1] != '\\'))) {
        len++;
    }

    char* result = (char*)malloc(len + 1);
    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

// ============================================================
// OpenAI-compatible provider (covers most providers)
// ============================================================

static bool openai_init(oag_provider_t* p, const oag_provider_config_t* config) {
    p->config = *config;
    printf("[Provider] %s initialized (model: %s)\n",
           oag_provider_type_name(config->type), config->model);
    return true;
}

static void openai_shutdown(oag_provider_t* p) {
    (void)p;
}

static oag_api_response_t openai_chat(oag_provider_t* p, const oag_api_request_t* req) {
    oag_api_response_t resp = { 0 };

    #ifdef _WIN32
    double t0;
    LARGE_INTEGER freq, cnt;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&cnt);
    t0 = (double)cnt.QuadPart / (double)freq.QuadPart * 1000.0;

    // Build request
    char* body;
    char auth[512];
    char extra[256] = "";
    char url[512];

    if (p->config.type == OAG_PROVIDER_ANTHROPIC) {
        body = build_anthropic_request(req, &p->config);
        snprintf(auth, sizeof(auth), "x-api-key: %s", p->config.api_key);
        snprintf(extra, sizeof(extra), "anthropic-version: 2023-06-01");
        snprintf(url, sizeof(url), "%s/v1/messages", p->config.base_url);
    } else {
        body = build_openai_request(req, &p->config);
        snprintf(auth, sizeof(auth), "Authorization: Bearer %s", p->config.api_key);
        snprintf(url, sizeof(url), "%s/v1/chat/completions", p->config.base_url);
    }

    // Send HTTP request
    http_result_t http = http_post(url, auth, extra[0] ? extra : NULL,
                                    body, p->config.timeout_ms);
    free(body);

    QueryPerformanceCounter(&cnt);
    resp.latency_ms = (double)cnt.QuadPart / (double)freq.QuadPart * 1000.0 - t0;

    if (http.status_code == 200 && http.body) {
        resp.success = true;

        // Extract content from response
        if (p->config.type == OAG_PROVIDER_ANTHROPIC) {
            resp.content = extract_json_string(http.body, "text");
        } else {
            resp.content = extract_json_string(http.body, "content");
        }

        if (!resp.content) {
            resp.content = strdup(http.body);  // fallback: return raw
        }

        resp.model = extract_json_string(http.body, "model");
    } else {
        resp.success = false;
        resp.error = http.body ? strdup(http.body) : strdup("HTTP request failed");
    }

    if (http.body && !resp.error) free(http.body);
    #else
    resp.success = false;
    resp.error = strdup("HTTP client not implemented for this platform");
    #endif

    return resp;
}

static const oag_provider_vtable_t openai_vtable = {
    .name     = "OpenAI-compatible",
    .type     = OAG_PROVIDER_OPENAI,
    .init     = openai_init,
    .shutdown = openai_shutdown,
    .chat     = openai_chat,
    .list_models = NULL,
};

// ============================================================
// Provider registry
// ============================================================

oag_provider_registry_t* oag_provider_registry_create(void) {
    oag_provider_registry_t* reg = (oag_provider_registry_t*)calloc(1, sizeof(*reg));
    reg->active_idx = -1;
    return reg;
}

void oag_provider_registry_free(oag_provider_registry_t* reg) {
    if (!reg) return;
    for (int i = 0; i < reg->n_providers; i++) {
        if (reg->providers[i]) {
            if (reg->providers[i]->vt->shutdown) {
                reg->providers[i]->vt->shutdown(reg->providers[i]);
            }
            free(reg->providers[i]);
        }
    }
    free(reg);
}

bool oag_provider_add(oag_provider_registry_t* reg, oag_provider_config_t config) {
    if (reg->n_providers >= OAG_MAX_PROVIDERS) return false;

    oag_provider_t* p = (oag_provider_t*)calloc(1, sizeof(oag_provider_t));
    p->vt = &openai_vtable;  // All providers use OpenAI-compatible HTTP

    // Set default base URL if not specified
    if (config.base_url[0] == '\0' && config.type < OAG_PROVIDER_COUNT) {
        strncpy(config.base_url, DEFAULT_URLS[config.type], sizeof(config.base_url) - 1);
    }
    if (config.model[0] == '\0' && config.type < OAG_PROVIDER_COUNT) {
        strncpy(config.model, DEFAULT_MODELS[config.type], sizeof(config.model) - 1);
    }
    if (config.temperature <= 0) config.temperature = 0.7f;
    if (config.max_tokens <= 0) config.max_tokens = 2048;
    if (config.timeout_ms <= 0) config.timeout_ms = 30000;

    if (!p->vt->init(p, &config)) {
        free(p);
        return false;
    }

    reg->providers[reg->n_providers] = p;
    if (reg->active_idx < 0) reg->active_idx = reg->n_providers;
    reg->n_providers++;
    return true;
}

bool oag_provider_set_active(oag_provider_registry_t* reg, int32_t idx) {
    if (idx < 0 || idx >= reg->n_providers) return false;
    reg->active_idx = idx;
    return true;
}

bool oag_provider_set_active_by_type(oag_provider_registry_t* reg, oag_provider_type_t type) {
    for (int i = 0; i < reg->n_providers; i++) {
        if (reg->providers[i]->config.type == type) {
            reg->active_idx = i;
            return true;
        }
    }
    return false;
}

oag_provider_t* oag_provider_get_active(oag_provider_registry_t* reg) {
    if (reg->active_idx < 0 || reg->active_idx >= reg->n_providers) return NULL;
    return reg->providers[reg->active_idx];
}

oag_api_response_t oag_provider_chat(oag_provider_registry_t* reg,
                                      const oag_api_request_t* req) {
    oag_provider_t* p = oag_provider_get_active(reg);
    if (!p) {
        oag_api_response_t resp = { 0 };
        resp.success = false;
        resp.error = strdup("No active provider");
        return resp;
    }
    return p->vt->chat(p, req);
}

// ============================================================
// Auto-detect providers from environment variables
// ============================================================

int32_t oag_provider_auto_detect(oag_provider_registry_t* reg) {
    int32_t count = 0;

    for (size_t i = 0; i < sizeof(ENV_KEYS) / sizeof(ENV_KEYS[0]); i++) {
        const char* key = getenv(ENV_KEYS[i].env_var);
        if (key && key[0] != '\0') {
            oag_provider_config_t config = { 0 };
            config.type = ENV_KEYS[i].type;
            strncpy(config.api_key, key, OAG_MAX_API_KEY_LEN - 1);

            if (oag_provider_add(reg, config)) {
                printf("[Provider] Auto-detected: %s (%s)\n",
                       oag_provider_type_name(config.type), ENV_KEYS[i].env_var);
                count++;
            }
        }
    }

    if (count == 0) {
        printf("[Provider] No API keys found in environment\n");
    }

    return count;
}

// ============================================================
// Config helpers
// ============================================================

#define MAKE_CONFIG(TYPE, KEY, MODEL_STR)          \
    oag_provider_config_t config = { 0 };          \
    config.type = TYPE;                             \
    if (KEY) strncpy(config.api_key, KEY, OAG_MAX_API_KEY_LEN - 1);  \
    if (MODEL_STR) strncpy(config.model, MODEL_STR, sizeof(config.model) - 1);  \
    return config;

oag_provider_config_t oag_provider_config_openai(const char* api_key, const char* model) {
    MAKE_CONFIG(OAG_PROVIDER_OPENAI, api_key, model);
}

oag_provider_config_t oag_provider_config_anthropic(const char* api_key, const char* model) {
    MAKE_CONFIG(OAG_PROVIDER_ANTHROPIC, api_key, model);
}

oag_provider_config_t oag_provider_config_google(const char* api_key, const char* model) {
    MAKE_CONFIG(OAG_PROVIDER_GOOGLE, api_key, model);
}

oag_provider_config_t oag_provider_config_xai(const char* api_key, const char* model) {
    MAKE_CONFIG(OAG_PROVIDER_XAI, api_key, model);
}

oag_provider_config_t oag_provider_config_openrouter(const char* api_key, const char* model) {
    MAKE_CONFIG(OAG_PROVIDER_OPENROUTER, api_key, model);
}

oag_provider_config_t oag_provider_config_groq(const char* api_key, const char* model) {
    MAKE_CONFIG(OAG_PROVIDER_GROQ, api_key, model);
}

oag_provider_config_t oag_provider_config_deepseek(const char* api_key, const char* model) {
    MAKE_CONFIG(OAG_PROVIDER_DEEPSEEK, api_key, model);
}

oag_provider_config_t oag_provider_config_ollama(const char* model) {
    MAKE_CONFIG(OAG_PROVIDER_OLLAMA, NULL, model);
}

oag_provider_config_t oag_provider_config_custom(const char* base_url, const char* api_key, const char* model) {
    oag_provider_config_t config = { 0 };
    config.type = OAG_PROVIDER_CUSTOM;
    if (api_key) strncpy(config.api_key, api_key, OAG_MAX_API_KEY_LEN - 1);
    if (model) strncpy(config.model, model, sizeof(config.model) - 1);
    if (base_url) strncpy(config.base_url, base_url, sizeof(config.base_url) - 1);
    return config;
}

// ============================================================
// Utility
// ============================================================

void oag_api_response_free(oag_api_response_t* resp) {
    free(resp->content);
    free(resp->model);
    free(resp->error);
    memset(resp, 0, sizeof(*resp));
}

const char* oag_provider_type_name(oag_provider_type_t type) {
    switch (type) {
        case OAG_PROVIDER_LOCAL:       return "Local";
        case OAG_PROVIDER_OPENAI:      return "OpenAI";
        case OAG_PROVIDER_ANTHROPIC:   return "Anthropic";
        case OAG_PROVIDER_GOOGLE:      return "Google";
        case OAG_PROVIDER_XAI:         return "xAI";
        case OAG_PROVIDER_MISTRAL:     return "Mistral";
        case OAG_PROVIDER_OPENROUTER:  return "OpenRouter";
        case OAG_PROVIDER_HUGGINGFACE: return "HuggingFace";
        case OAG_PROVIDER_GROQ:        return "Groq";
        case OAG_PROVIDER_TOGETHER:    return "Together";
        case OAG_PROVIDER_DEEPSEEK:    return "DeepSeek";
        case OAG_PROVIDER_OLLAMA:      return "Ollama";
        case OAG_PROVIDER_CUSTOM:      return "Custom";
        default:                       return "Unknown";
    }
}
