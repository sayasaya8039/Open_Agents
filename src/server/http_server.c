// Open_Agents — Minimal HTTP Server (OpenAI-compatible API)
// Endpoints: POST /v1/chat/completions, GET /v1/models, GET /health
#include "server/http_server.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
#pragma comment(lib, "ws2_32.lib")
typedef SOCKET socket_t;
#define CLOSE_SOCKET closesocket
#define SOCKET_ERROR_VAL SOCKET_ERROR
#else
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <unistd.h>
typedef int socket_t;
#define CLOSE_SOCKET close
#define INVALID_SOCKET -1
#define SOCKET_ERROR_VAL -1
#endif

struct oag_server {
    oag_inference_t*   inf;
    oag_server_config_t config;
    socket_t           listen_fd;
    volatile bool      running;
};

oag_server_config_t oag_server_default_config(void) {
    oag_server_config_t c;
    c.port = 8080;
    strncpy(c.host, "0.0.0.0", sizeof(c.host));
    c.max_connections = 64;
    c.cors_enabled = true;
    return c;
}

// ============================================================
// Simple HTTP parser (minimal)
// ============================================================

typedef struct {
    char method[16];
    char path[256];
    char* body;
    size_t body_len;
    size_t content_length;
} http_request_t;

static bool parse_http_request(const char* raw, size_t raw_len, http_request_t* req) {
    memset(req, 0, sizeof(*req));

    // Parse request line
    const char* line_end = strstr(raw, "\r\n");
    if (!line_end) return false;

    sscanf(raw, "%15s %255s", req->method, req->path);

    // Find Content-Length
    const char* cl = strstr(raw, "Content-Length:");
    if (!cl) cl = strstr(raw, "content-length:");
    if (cl) {
        req->content_length = (size_t)atol(cl + 15);
    }

    // Find body (after \r\n\r\n)
    const char* body_start = strstr(raw, "\r\n\r\n");
    if (body_start) {
        body_start += 4;
        req->body = (char*)body_start;
        req->body_len = raw_len - (body_start - raw);
    }

    return true;
}

// ============================================================
// JSON helpers (minimal, no external deps)
// ============================================================

static const char* json_find_string(const char* json, const char* key) {
    char pattern[128];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char* pos = strstr(json, pattern);
    if (!pos) return NULL;
    pos = strchr(pos + strlen(pattern), ':');
    if (!pos) return NULL;
    pos++;
    while (*pos == ' ' || *pos == '\t') pos++;
    if (*pos == '"') return pos + 1;
    return pos;
}

static int json_get_int(const char* json, const char* key, int def) {
    const char* val = json_find_string(json, key);
    if (!val) return def;
    // Back up to before the quote if we're past one
    while (val > json && *(val-1) != ':' && *(val-1) != ' ') val--;
    return atoi(val);
}

// ============================================================
// HTTP response helpers
// ============================================================

static void send_response(socket_t fd, int status, const char* status_text,
                          const char* content_type, const char* body) {
    char header[1024];
    int body_len = body ? (int)strlen(body) : 0;
    int header_len = snprintf(header, sizeof(header),
        "HTTP/1.1 %d %s\r\n"
        "Content-Type: %s\r\n"
        "Content-Length: %d\r\n"
        "Access-Control-Allow-Origin: *\r\n"
        "Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n"
        "Access-Control-Allow-Headers: Content-Type, Authorization\r\n"
        "Connection: close\r\n"
        "\r\n",
        status, status_text, content_type, body_len);

    send(fd, header, header_len, 0);
    if (body && body_len > 0) {
        send(fd, body, body_len, 0);
    }
}

static void send_json(socket_t fd, int status, const char* json) {
    send_response(fd, status, status == 200 ? "OK" : "Error",
                  "application/json", json);
}

// ============================================================
// SSE streaming helper
// ============================================================

typedef struct {
    socket_t fd;
    bool     header_sent;
} sse_ctx_t;

static void sse_send_header(sse_ctx_t* ctx) {
    if (ctx->header_sent) return;
    const char* header =
        "HTTP/1.1 200 OK\r\n"
        "Content-Type: text/event-stream\r\n"
        "Cache-Control: no-cache\r\n"
        "Connection: keep-alive\r\n"
        "Access-Control-Allow-Origin: *\r\n"
        "\r\n";
    send(ctx->fd, header, (int)strlen(header), 0);
    ctx->header_sent = true;
}

static void sse_send_token(const char* text, void* user) {
    sse_ctx_t* ctx = (sse_ctx_t*)user;
    sse_send_header(ctx);

    // Escape text for JSON
    char escaped[1024];
    size_t j = 0;
    for (size_t i = 0; text[i] && j < sizeof(escaped) - 2; i++) {
        if (text[i] == '"' || text[i] == '\\') escaped[j++] = '\\';
        if (text[i] == '\n') { escaped[j++] = '\\'; escaped[j++] = 'n'; continue; }
        escaped[j++] = text[i];
    }
    escaped[j] = '\0';

    char chunk[2048];
    int len = snprintf(chunk, sizeof(chunk),
        "data: {\"choices\":[{\"delta\":{\"content\":\"%s\"}}]}\n\n",
        escaped);
    send(ctx->fd, chunk, len, 0);
}

// ============================================================
// Route handlers
// ============================================================

static void handle_health(socket_t fd) {
    send_json(fd, 200, "{\"status\":\"ok\",\"engine\":\"Open_Agents\",\"version\":\"0.1.0\"}");
}

static void handle_models(socket_t fd, oag_inference_t* inf) {
    char json[1024];
    const char* model_name = inf->model ? inf->model->arch : "unknown";
    snprintf(json, sizeof(json),
        "{\"object\":\"list\",\"data\":[{"
        "\"id\":\"local-%s\","
        "\"object\":\"model\","
        "\"owned_by\":\"local\","
        "\"permission\":[]}]}",
        model_name);
    send_json(fd, 200, json);
}

static void handle_chat_completions(socket_t fd, oag_inference_t* inf,
                                     http_request_t* req) {
    if (!req->body || req->body_len == 0) {
        send_json(fd, 400, "{\"error\":\"Empty request body\"}");
        return;
    }

    // Parse messages from JSON (simplified)
    // In production, use a proper JSON parser
    bool stream = (strstr(req->body, "\"stream\":true") != NULL ||
                   strstr(req->body, "\"stream\": true") != NULL);
    int max_tokens = json_get_int(req->body, "max_tokens", 512);

    // Extract last user message (simplified)
    const char* content_start = NULL;
    const char* search = req->body;
    while ((search = strstr(search, "\"content\"")) != NULL) {
        content_start = search;
        search += 9;
    }

    char user_msg[4096] = "Hello";
    if (content_start) {
        const char* val = strchr(content_start + 9, '"');
        if (val) {
            val++;
            const char* end = strchr(val, '"');
            if (end) {
                size_t len = end - val;
                if (len > sizeof(user_msg) - 1) len = sizeof(user_msg) - 1;
                memcpy(user_msg, val, len);
                user_msg[len] = '\0';
            }
        }
    }

    // Build chat params
    oag_chat_msg_t messages[1] = {
        { .role = "user", .content = user_msg }
    };

    oag_chat_params_t params = {
        .messages    = messages,
        .n_messages  = 1,
        .sampler     = oag_sampler_default_params(),
        .max_tokens  = max_tokens,
        .stream      = stream,
        .on_token    = NULL,
        .user_data   = NULL,
    };

    if (stream) {
        sse_ctx_t sse = { .fd = fd, .header_sent = false };
        params.on_token = sse_send_token;
        params.user_data = &sse;

        char* result = oag_inference_chat(inf, params);
        free(result);

        // Send done marker
        const char* done = "data: [DONE]\n\n";
        send(fd, done, (int)strlen(done), 0);
    } else {
        char* result = oag_inference_chat(inf, params);

        // Build response JSON
        size_t result_len = result ? strlen(result) : 0;
        size_t json_size = result_len + 512;
        char* json = (char*)malloc(json_size);

        snprintf(json, json_size,
            "{\"id\":\"chatcmpl-local\","
            "\"object\":\"chat.completion\","
            "\"choices\":[{\"index\":0,"
            "\"message\":{\"role\":\"assistant\",\"content\":\"%s\"},"
            "\"finish_reason\":\"stop\"}],"
            "\"usage\":{\"prompt_tokens\":0,\"completion_tokens\":0,\"total_tokens\":0}}",
            result ? result : "");

        send_json(fd, 200, json);
        free(json);
        free(result);
    }
}

// ============================================================
// Server lifecycle
// ============================================================

oag_server_t* oag_server_create(oag_inference_t* inf, oag_server_config_t config) {
    #ifdef _WIN32
    WSADATA wsa;
    WSAStartup(MAKEWORD(2, 2), &wsa);
    #endif

    oag_server_t* srv = (oag_server_t*)calloc(1, sizeof(oag_server_t));
    srv->inf    = inf;
    srv->config = config;
    srv->running = true;

    srv->listen_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (srv->listen_fd == INVALID_SOCKET) {
        fprintf(stderr, "[Server] Failed to create socket\n");
        free(srv);
        return NULL;
    }

    int opt = 1;
    setsockopt(srv->listen_fd, SOL_SOCKET, SO_REUSEADDR, (const char*)&opt, sizeof(opt));

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons(config.port);
    addr.sin_addr.s_addr = INADDR_ANY;

    if (bind(srv->listen_fd, (struct sockaddr*)&addr, sizeof(addr)) == SOCKET_ERROR_VAL) {
        fprintf(stderr, "[Server] Failed to bind port %d\n", config.port);
        CLOSE_SOCKET(srv->listen_fd);
        free(srv);
        return NULL;
    }

    listen(srv->listen_fd, config.max_connections);
    printf("[Server] Listening on http://%s:%d\n", config.host, config.port);
    printf("[Server] Endpoints:\n");
    printf("  POST /v1/chat/completions  — Chat (OpenAI compatible)\n");
    printf("  GET  /v1/models            — List models\n");
    printf("  GET  /health               — Health check\n");

    return srv;
}

void oag_server_run(oag_server_t* srv) {
    while (srv->running) {
        struct sockaddr_in client_addr;
        int addr_len = sizeof(client_addr);
        socket_t client = accept(srv->listen_fd, (struct sockaddr*)&client_addr, &addr_len);

        if (client == INVALID_SOCKET) continue;

        // Read request (simplified: single recv)
        char buf[65536];
        int n = recv(client, buf, sizeof(buf) - 1, 0);
        if (n <= 0) {
            CLOSE_SOCKET(client);
            continue;
        }
        buf[n] = '\0';

        http_request_t req;
        if (!parse_http_request(buf, n, &req)) {
            send_json(client, 400, "{\"error\":\"Invalid request\"}");
            CLOSE_SOCKET(client);
            continue;
        }

        // Route
        if (strcmp(req.method, "OPTIONS") == 0) {
            send_response(client, 204, "No Content", "text/plain", NULL);
        } else if (strcmp(req.path, "/health") == 0) {
            handle_health(client);
        } else if (strcmp(req.path, "/v1/models") == 0) {
            handle_models(client, srv->inf);
        } else if (strcmp(req.path, "/v1/chat/completions") == 0) {
            handle_chat_completions(client, srv->inf, &req);
        } else {
            send_json(client, 404, "{\"error\":\"Not found\"}");
        }

        CLOSE_SOCKET(client);
    }
}

void oag_server_stop(oag_server_t* srv) {
    srv->running = false;
    CLOSE_SOCKET(srv->listen_fd);
}

void oag_server_free(oag_server_t* srv) {
    if (!srv) return;
    CLOSE_SOCKET(srv->listen_fd);
    #ifdef _WIN32
    WSACleanup();
    #endif
    free(srv);
}
