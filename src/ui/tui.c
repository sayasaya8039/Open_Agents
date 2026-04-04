// Open_Agents — Terminal UI (Windows Console API)
// Split-pane: Left=Code Editor, Right=AI Chat, Bottom=Output
#include "ui/tui.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
#include <conio.h>

// ============================================================
// TUI internals
// ============================================================

#define MAX_LINES 100000
#define MAX_LINE_LEN 4096
#define MAX_CHAT_MSGS 1024

typedef struct {
    char* text;
    int   len;
} line_t;

typedef struct {
    char role[16];  // "user" or "assistant"
    char* text;
} chat_msg_t;

struct oag_tui {
    oag_tui_config_t config;
    HANDLE           hout;
    HANDLE           hin;
    CONSOLE_SCREEN_BUFFER_INFO csbi;

    int width;
    int height;

    // Editor state
    line_t*  lines;
    int      n_lines;
    int      cursor_x;
    int      cursor_y;
    int      scroll_y;
    char     filename[512];
    bool     modified;

    // Chat state
    chat_msg_t chat[MAX_CHAT_MSGS];
    int        n_chat;
    char       chat_input[MAX_LINE_LEN];
    int        chat_input_len;
    int        chat_scroll;

    // Output panel
    char*    output_buf;
    int      output_len;

    // Layout
    int      editor_width;    // left pane width
    int      chat_width;      // right pane width
    int      output_height;   // bottom pane height

    // AI
    oag_inference_t*         inf;
    oag_provider_registry_t* providers;

    // Mode
    enum { MODE_EDITOR, MODE_CHAT, MODE_COMMAND } mode;
    bool running;
};

// ============================================================
// Console helpers
// ============================================================

static void tui_goto(oag_tui_t* t, int x, int y) {
    COORD pos = { (SHORT)x, (SHORT)y };
    SetConsoleCursorPosition(t->hout, pos);
}

static void tui_set_color(oag_tui_t* t, uint32_t fg_rgb, uint32_t bg_rgb) {
    // Windows Console Virtual Terminal Sequences (ANSI)
    int fr = (fg_rgb >> 16) & 0xFF, fg = (fg_rgb >> 8) & 0xFF, fb = fg_rgb & 0xFF;
    int br = (bg_rgb >> 16) & 0xFF, bg2 = (bg_rgb >> 8) & 0xFF, bb = bg_rgb & 0xFF;
    printf("\033[38;2;%d;%d;%dm\033[48;2;%d;%d;%dm", fr, fg, fb, br, bg2, bb);
    (void)t;
}

static void tui_reset_color(oag_tui_t* t) {
    printf("\033[0m");
    (void)t;
}

static void tui_clear(oag_tui_t* t) {
    printf("\033[2J\033[H");
    (void)t;
}

static void tui_fill_line(oag_tui_t* t, int y, uint32_t bg) {
    tui_goto(t, 0, y);
    tui_set_color(t, OAG_COLOR_FG, bg);
    for (int i = 0; i < t->width; i++) putchar(' ');
}

// ============================================================
// Drawing
// ============================================================

static void draw_status_bar(oag_tui_t* t) {
    int y = t->height - 1;
    tui_goto(t, 0, y);
    tui_set_color(t, 0x1E1E2E, OAG_COLOR_ACCENT);

    char status[256];
    const char* mode_str = t->mode == MODE_EDITOR ? "EDIT" :
                           t->mode == MODE_CHAT ? "CHAT" : "CMD";
    snprintf(status, sizeof(status),
        " [%s] %s%s  Ln %d, Col %d  |  Tab: switch  Ctrl+Q: quit  Ctrl+S: save ",
        mode_str,
        t->filename[0] ? t->filename : "[untitled]",
        t->modified ? " *" : "",
        t->cursor_y + 1, t->cursor_x + 1);

    int pad = t->width - (int)strlen(status);
    printf("%s", status);
    for (int i = 0; i < pad; i++) putchar(' ');

    tui_reset_color(t);
}

static void draw_editor(oag_tui_t* t) {
    int panel_h = t->height - t->output_height - 1;  // -1 for status bar

    for (int row = 0; row < panel_h; row++) {
        int line_idx = t->scroll_y + row;
        tui_goto(t, 0, row);

        if (t->config.line_numbers) {
            tui_set_color(t, OAG_COLOR_OVERLAY, OAG_COLOR_BG);
            if (line_idx < t->n_lines) {
                printf("%4d ", line_idx + 1);
            } else {
                printf("   ~ ");
            }
        }

        tui_set_color(t, OAG_COLOR_FG, OAG_COLOR_BG);

        if (line_idx < t->n_lines && t->lines[line_idx].text) {
            int max_w = t->editor_width - 5;
            int len = t->lines[line_idx].len;
            if (len > max_w) len = max_w;

            // Simple syntax highlight for keywords
            const char* text = t->lines[line_idx].text;
            for (int c = 0; c < len; c++) {
                // Highlight comments
                if (text[c] == '/' && c + 1 < len && text[c+1] == '/') {
                    tui_set_color(t, OAG_COLOR_OVERLAY, OAG_COLOR_BG);
                    printf("%.*s", len - c, text + c);
                    break;
                }
                // Highlight strings
                else if (text[c] == '"') {
                    tui_set_color(t, OAG_COLOR_GREEN, OAG_COLOR_BG);
                    putchar(text[c]);
                }
                // Highlight numbers
                else if (text[c] >= '0' && text[c] <= '9' &&
                         (c == 0 || !((text[c-1] >= 'a' && text[c-1] <= 'z') ||
                                      (text[c-1] >= 'A' && text[c-1] <= 'Z')))) {
                    tui_set_color(t, OAG_COLOR_YELLOW, OAG_COLOR_BG);
                    putchar(text[c]);
                }
                else {
                    tui_set_color(t, OAG_COLOR_FG, OAG_COLOR_BG);
                    putchar(text[c]);
                }
            }

            // Fill rest
            int printed = len + 5;
            for (int i = printed; i < t->editor_width; i++) putchar(' ');
        } else {
            for (int i = 5; i < t->editor_width; i++) putchar(' ');
        }
    }
}

static void draw_chat(oag_tui_t* t) {
    int panel_h = t->height - t->output_height - 1;
    int x_start = t->editor_width;

    // Border
    for (int row = 0; row < panel_h; row++) {
        tui_goto(t, x_start, row);
        tui_set_color(t, OAG_COLOR_SURFACE, OAG_COLOR_BG);
        putchar('|');
    }

    // Chat header
    tui_goto(t, x_start + 2, 0);
    tui_set_color(t, OAG_COLOR_ACCENT, OAG_COLOR_BG);
    printf(" AI Chat ");
    tui_set_color(t, OAG_COLOR_FG, OAG_COLOR_BG);

    // Chat messages
    int row = 2;
    int max_w = t->chat_width - 3;
    for (int i = t->chat_scroll; i < t->n_chat && row < panel_h - 2; i++) {
        tui_goto(t, x_start + 2, row);

        if (strcmp(t->chat[i].role, "user") == 0) {
            tui_set_color(t, OAG_COLOR_GREEN, OAG_COLOR_BG);
            printf("You: ");
        } else {
            tui_set_color(t, OAG_COLOR_ACCENT, OAG_COLOR_BG);
            printf("AI:  ");
        }

        tui_set_color(t, OAG_COLOR_FG, OAG_COLOR_BG);
        if (t->chat[i].text) {
            int len = (int)strlen(t->chat[i].text);
            if (len > max_w - 5) len = max_w - 5;
            printf("%.*s", len, t->chat[i].text);
        }
        row++;
    }

    // Input line
    tui_goto(t, x_start + 1, panel_h - 1);
    tui_set_color(t, OAG_COLOR_FG, OAG_COLOR_SURFACE);
    printf(" > %.*s", max_w - 3, t->chat_input);
    int pad = max_w - 3 - t->chat_input_len;
    for (int i = 0; i < pad; i++) putchar(' ');
}

static void draw_output(oag_tui_t* t) {
    if (t->output_height <= 0) return;
    int y_start = t->height - t->output_height - 1;

    // Border
    tui_goto(t, 0, y_start);
    tui_set_color(t, OAG_COLOR_SURFACE, OAG_COLOR_BG);
    for (int i = 0; i < t->width; i++) putchar('-');

    // Output content
    tui_set_color(t, OAG_COLOR_FG, OAG_COLOR_BG);
    for (int row = 0; row < t->output_height - 1; row++) {
        tui_goto(t, 0, y_start + 1 + row);
        for (int i = 0; i < t->width; i++) putchar(' ');
    }

    if (t->output_buf) {
        tui_goto(t, 1, y_start + 1);
        int len = t->output_len;
        if (len > t->width * (t->output_height - 1)) len = t->width * (t->output_height - 1);
        printf("%.*s", len, t->output_buf);
    }
}

static void draw_all(oag_tui_t* t) {
    draw_editor(t);
    draw_chat(t);
    draw_output(t);
    draw_status_bar(t);

    // Position cursor
    if (t->mode == MODE_EDITOR) {
        int cx = (t->config.line_numbers ? 5 : 0) + t->cursor_x;
        int cy = t->cursor_y - t->scroll_y;
        tui_goto(t, cx, cy);
    } else if (t->mode == MODE_CHAT) {
        int cx = t->editor_width + 4 + t->chat_input_len;
        int cy = t->height - t->output_height - 2;
        tui_goto(t, cx, cy);
    }

    fflush(stdout);
}

// ============================================================
// AI chat handler
// ============================================================

static void handle_chat_submit(oag_tui_t* t) {
    if (t->chat_input_len == 0) return;

    // Add user message
    if (t->n_chat < MAX_CHAT_MSGS) {
        strcpy(t->chat[t->n_chat].role, "user");
        t->chat[t->n_chat].text = strdup(t->chat_input);
        t->n_chat++;
    }

    // Get AI response
    char* response = NULL;

    if (t->providers) {
        oag_api_message_t msgs[1] = {{ .role = "user", .content = t->chat_input }};
        oag_api_request_t req = {
            .messages = msgs, .n_messages = 1,
            .temperature = -1, .max_tokens = -1, .stream = false,
        };
        oag_api_response_t resp = oag_provider_chat(t->providers, &req);
        if (resp.success && resp.content) {
            response = strdup(resp.content);
        }
        oag_api_response_free(&resp);
    } else if (t->inf) {
        oag_chat_msg_t msgs[1] = {{ .role = "user", .content = t->chat_input }};
        oag_chat_params_t params = {
            .messages = msgs, .n_messages = 1,
            .sampler = oag_sampler_default_params(),
            .max_tokens = 256, .stream = false,
        };
        response = oag_inference_chat(t->inf, params);
    }

    // Add AI response
    if (t->n_chat < MAX_CHAT_MSGS) {
        strcpy(t->chat[t->n_chat].role, "assistant");
        t->chat[t->n_chat].text = response ? response : strdup("(no response)");
        t->n_chat++;
    } else {
        // Chat full — free response to prevent leak
        free(response);
    }

    // Auto-scroll
    int panel_h = t->height - t->output_height - 4;
    if (t->n_chat > panel_h) {
        t->chat_scroll = t->n_chat - panel_h;
    }

    // Clear input
    t->chat_input[0] = '\0';
    t->chat_input_len = 0;
}

// ============================================================
// Input handling
// ============================================================

static void handle_key(oag_tui_t* t, int key) {
    // Global keys
    if (key == 17) { // Ctrl+Q
        t->running = false;
        return;
    }
    if (key == 9) { // Tab — switch mode
        t->mode = (t->mode == MODE_EDITOR) ? MODE_CHAT : MODE_EDITOR;
        return;
    }

    if (t->mode == MODE_EDITOR) {
        switch (key) {
            case 224: { // Extended key
                int ext = _getch();
                switch (ext) {
                    case 72: if (t->cursor_y > 0) t->cursor_y--; break;  // Up
                    case 80: if (t->cursor_y < t->n_lines - 1) t->cursor_y++; break;  // Down
                    case 75: if (t->cursor_x > 0) t->cursor_x--; break;  // Left
                    case 77: t->cursor_x++; break;  // Right
                }
                // Scroll
                int panel_h = t->height - t->output_height - 2;
                if (t->cursor_y < t->scroll_y) t->scroll_y = t->cursor_y;
                if (t->cursor_y >= t->scroll_y + panel_h) t->scroll_y = t->cursor_y - panel_h + 1;
                break;
            }
            case 13: { // Enter
                // Insert new line
                if (t->n_lines < MAX_LINES) {
                    // Shift lines down
                    for (int i = t->n_lines; i > t->cursor_y + 1; i--) {
                        t->lines[i] = t->lines[i - 1];
                    }
                    t->lines[t->cursor_y + 1].text = strdup("");
                    t->lines[t->cursor_y + 1].len = 0;
                    t->n_lines++;
                    t->cursor_y++;
                    t->cursor_x = 0;
                    t->modified = true;
                }
                break;
            }
            case 8: { // Backspace
                if (t->cursor_x > 0 && t->cursor_y < t->n_lines) {
                    line_t* line = &t->lines[t->cursor_y];
                    if (line->text && t->cursor_x <= line->len) {
                        memmove(line->text + t->cursor_x - 1,
                                line->text + t->cursor_x,
                                line->len - t->cursor_x + 1);
                        line->len--;
                        t->cursor_x--;
                        t->modified = true;
                    }
                }
                break;
            }
            default: {
                if (key >= 32 && key < 127) {
                    // Insert character
                    if (t->cursor_y < t->n_lines) {
                        line_t* line = &t->lines[t->cursor_y];
                        if (!line->text) {
                            line->text = (char*)calloc(MAX_LINE_LEN, 1);
                            line->len = 0;
                        }
                        if (line->len < MAX_LINE_LEN - 1) {
                            memmove(line->text + t->cursor_x + 1,
                                    line->text + t->cursor_x,
                                    line->len - t->cursor_x + 1);
                            line->text[t->cursor_x] = (char)key;
                            line->len++;
                            t->cursor_x++;
                            t->modified = true;
                        }
                    }
                }
                break;
            }
        }
    } else if (t->mode == MODE_CHAT) {
        if (key == 13) { // Enter
            handle_chat_submit(t);
        } else if (key == 8) { // Backspace
            if (t->chat_input_len > 0) {
                t->chat_input[--t->chat_input_len] = '\0';
            }
        } else if (key >= 32 && key < 127 && t->chat_input_len < MAX_LINE_LEN - 1) {
            t->chat_input[t->chat_input_len++] = (char)key;
            t->chat_input[t->chat_input_len] = '\0';
        }
    }
}

// ============================================================
// Public API
// ============================================================

oag_tui_config_t oag_tui_default_config(void) {
    return (oag_tui_config_t){
        .syntax_highlight = true,
        .line_numbers     = true,
        .ai_inline        = false,
        .tab_size         = 4,
        .auto_complete    = false,
    };
}

oag_tui_t* oag_tui_create(oag_tui_config_t config) {
    oag_tui_t* t = (oag_tui_t*)calloc(1, sizeof(oag_tui_t));
    t->config = config;

    t->hout = GetStdHandle(STD_OUTPUT_HANDLE);
    t->hin  = GetStdHandle(STD_INPUT_HANDLE);

    // Enable virtual terminal processing (ANSI colors)
    DWORD mode;
    GetConsoleMode(t->hout, &mode);
    SetConsoleMode(t->hout, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);

    // Get console size
    GetConsoleScreenBufferInfo(t->hout, &t->csbi);
    t->width  = t->csbi.srWindow.Right - t->csbi.srWindow.Left + 1;
    t->height = t->csbi.srWindow.Bottom - t->csbi.srWindow.Top + 1;

    // Layout: 60% editor, 40% chat, 5 lines output
    t->editor_width  = (t->width * 60) / 100;
    t->chat_width    = t->width - t->editor_width;
    t->output_height = 5;

    // Init editor
    t->lines = (line_t*)calloc(MAX_LINES, sizeof(line_t));
    t->lines[0].text = strdup("");
    t->lines[0].len = 0;
    t->n_lines = 1;

    t->mode = MODE_EDITOR;
    t->running = true;

    // Hide cursor blinking
    CONSOLE_CURSOR_INFO cci = { .dwSize = 25, .bVisible = TRUE };
    SetConsoleCursorInfo(t->hout, &cci);

    return t;
}

void oag_tui_free(oag_tui_t* t) {
    if (!t) return;
    for (int i = 0; i < t->n_lines; i++) free(t->lines[i].text);
    free(t->lines);
    for (int i = 0; i < t->n_chat; i++) free(t->chat[i].text);
    free(t->output_buf);
    tui_reset_color(t);
    free(t);
}

void oag_tui_set_inference(oag_tui_t* tui, oag_inference_t* inf) {
    tui->inf = inf;
}

void oag_tui_set_provider(oag_tui_t* tui, oag_provider_registry_t* reg) {
    tui->providers = reg;
}

bool oag_tui_open_file(oag_tui_t* t, const char* path) {
    FILE* f = fopen(path, "r");
    if (!f) return false;

    strncpy(t->filename, path, sizeof(t->filename) - 1);

    // Free existing lines
    for (int i = 0; i < t->n_lines; i++) free(t->lines[i].text);
    t->n_lines = 0;

    char buf[MAX_LINE_LEN];
    while (fgets(buf, sizeof(buf), f) && t->n_lines < MAX_LINES) {
        size_t len = strlen(buf);
        if (len > 0 && buf[len-1] == '\n') buf[--len] = '\0';
        if (len > 0 && buf[len-1] == '\r') buf[--len] = '\0';
        t->lines[t->n_lines].text = strdup(buf);
        t->lines[t->n_lines].len = (int)len;
        t->n_lines++;
    }

    fclose(f);
    t->cursor_x = 0;
    t->cursor_y = 0;
    t->scroll_y = 0;
    t->modified = false;

    return true;
}

void oag_tui_run(oag_tui_t* t) {
    tui_clear(t);
    draw_all(t);

    while (t->running) {
        if (_kbhit()) {
            int key = _getch();
            handle_key(t, key);
            draw_all(t);
        } else {
            Sleep(16);  // ~60fps
        }
    }

    tui_clear(t);
    tui_reset_color(t);
}

#else
// Non-Windows stubs
oag_tui_config_t oag_tui_default_config(void) { return (oag_tui_config_t){0}; }
oag_tui_t* oag_tui_create(oag_tui_config_t c) { (void)c; return NULL; }
void oag_tui_free(oag_tui_t* t) { (void)t; }
void oag_tui_set_inference(oag_tui_t* t, oag_inference_t* i) { (void)t; (void)i; }
void oag_tui_set_provider(oag_tui_t* t, oag_provider_registry_t* r) { (void)t; (void)r; }
bool oag_tui_open_file(oag_tui_t* t, const char* p) { (void)t; (void)p; return false; }
void oag_tui_run(oag_tui_t* t) { (void)t; }
#endif
