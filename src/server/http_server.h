// Open_Agents — HTTP Server (OpenAI-compatible API)
#ifndef OAG_HTTP_SERVER_H
#define OAG_HTTP_SERVER_H

#include "core/inference.h"

typedef struct {
    int      port;
    char     host[64];
    int      max_connections;
    bool     cors_enabled;
} oag_server_config_t;

typedef struct oag_server oag_server_t;

oag_server_config_t oag_server_default_config(void);

// Create and run server
oag_server_t* oag_server_create(oag_inference_t* inf, oag_server_config_t config);
void          oag_server_run(oag_server_t* srv);   // blocking
void          oag_server_stop(oag_server_t* srv);
void          oag_server_free(oag_server_t* srv);

#endif // OAG_HTTP_SERVER_H
