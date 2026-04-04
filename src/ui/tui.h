// Open_Agents — Terminal UI (TUI Code Editor + AI Chat)
// Windows Console API で高速 TUI を描画
#ifndef OAG_TUI_H
#define OAG_TUI_H

#include <stdint.h>
#include <stdbool.h>
#include "core/inference.h"
#include "provider/api_provider.h"

// Color scheme
#define OAG_COLOR_BG        0x1E1E2E  // Catppuccin Mocha base
#define OAG_COLOR_FG        0xCDD6F4  // text
#define OAG_COLOR_ACCENT    0x89B4FA  // blue
#define OAG_COLOR_GREEN     0xA6E3A1
#define OAG_COLOR_RED       0xF38BA8
#define OAG_COLOR_YELLOW    0xF9E2AF
#define OAG_COLOR_SURFACE   0x313244
#define OAG_COLOR_OVERLAY   0x6C7086

// Panel types
typedef enum {
    OAG_PANEL_EDITOR  = 0,  // Code editor
    OAG_PANEL_CHAT    = 1,  // AI chat
    OAG_PANEL_OUTPUT  = 2,  // Compilation / run output
    OAG_PANEL_STATUS  = 3,  // Status bar
} oag_panel_type_t;

// TUI config
typedef struct {
    bool   syntax_highlight;
    bool   line_numbers;
    bool   ai_inline;          // show AI suggestions inline
    int    tab_size;
    bool   auto_complete;
    char   font_name[64];
} oag_tui_config_t;

// TUI context
typedef struct oag_tui oag_tui_t;

// Create TUI
oag_tui_config_t oag_tui_default_config(void);
oag_tui_t* oag_tui_create(oag_tui_config_t config);
void       oag_tui_free(oag_tui_t* tui);

// Set inference engine (for local model) and/or API provider
void oag_tui_set_inference(oag_tui_t* tui, oag_inference_t* inf);
void oag_tui_set_provider(oag_tui_t* tui, oag_provider_registry_t* reg);

// Main loop (blocking)
void oag_tui_run(oag_tui_t* tui);

// Load file into editor
bool oag_tui_open_file(oag_tui_t* tui, const char* path);

#endif // OAG_TUI_H
