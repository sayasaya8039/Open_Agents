// Open_Agents — Win32 Native GUI Host (WebView2 embedded)
#ifndef OAG_WIN32_APP_H
#define OAG_WIN32_APP_H

#include <stdbool.h>
#include <stdint.h>

typedef struct {
    int      width;
    int      height;
    bool     maximized;
    bool     frameless;     // custom title bar
    char     title[128];
} oag_gui_config_t;

typedef struct oag_gui oag_gui_t;

oag_gui_config_t oag_gui_default_config(void);

// Create and run the GUI application
oag_gui_t* oag_gui_create(oag_gui_config_t config);
void       oag_gui_run(oag_gui_t* gui);     // blocking message loop
void       oag_gui_free(oag_gui_t* gui);

// Send message from C backend to WebView (JS)
void oag_gui_post_message(oag_gui_t* gui, const char* json);

#endif // OAG_WIN32_APP_H
