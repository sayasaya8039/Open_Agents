// Open_Agents — Win32 Native GUI with WebView2
// Creates a frameless window with embedded Chromium Edge for the UI
// C backend ↔ WebView2 communication via postMessage/native messaging
#include "gui/win32_app.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define COBJMACROS
#include <windows.h>
#include <shlwapi.h>
#include <shellapi.h>

#pragma comment(lib, "shlwapi.lib")

// ============================================================
// WebView2 COM interfaces (dynamically loaded)
// We define minimal COM vtables to avoid needing the WebView2 SDK
// ============================================================

// GUIDs
static const GUID IID_ICoreWebView2Environment = {
    0xb96d755e, 0x0319, 0x4e92, {0xa2, 0x96, 0x23, 0x43, 0x6f, 0x46, 0xa1, 0xfc}
};

// Callback function types
typedef HRESULT (STDMETHODCALLTYPE *CreateWebViewEnvironmentCompletedHandler)(
    void* self, HRESULT result, void* environment);
typedef HRESULT (STDMETHODCALLTYPE *CreateWebViewControllerCompletedHandler)(
    void* self, HRESULT result, void* controller);

// Loader function
typedef HRESULT (STDMETHODCALLTYPE *pfn_CreateCoreWebView2EnvironmentWithOptions)(
    PCWSTR browserExecutableFolder,
    PCWSTR userDataFolder,
    void* environmentOptions,
    void* environmentCreatedHandler);

// ============================================================
// Application state
// ============================================================

struct oag_gui {
    oag_gui_config_t config;
    HWND              hwnd;
    HINSTANCE         hinst;
    HMODULE           hlib_wv2;   // WebView2Loader.dll
    void*             webview_env;
    void*             webview_controller;
    void*             webview;
    bool              wv2_ready;
    char*             html_content;  // embedded HTML
};

// ============================================================
// Window procedure
// ============================================================

static LRESULT CALLBACK wnd_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    oag_gui_t* gui = (oag_gui_t*)GetWindowLongPtrA(hwnd, GWLP_USERDATA);

    switch (msg) {
    case WM_SIZE:
        if (gui && gui->wv2_ready && gui->webview_controller) {
            RECT rc;
            GetClientRect(hwnd, &rc);
            // Resize WebView to fill window
            // Would call ICoreWebView2Controller::put_Bounds here
        }
        return 0;

    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;

    case WM_GETMINMAXINFO: {
        MINMAXINFO* mmi = (MINMAXINFO*)lp;
        mmi->ptMinTrackSize.x = 800;
        mmi->ptMinTrackSize.y = 600;
        return 0;
    }

    default:
        return DefWindowProcA(hwnd, msg, wp, lp);
    }
}

// ============================================================
// Create the HTML content (embedded — no external files needed)
// ============================================================

static char* build_html_content(void) {
    // We embed a self-contained HTML app based on the stitch design
    static const char html[] =
"<!DOCTYPE html>\n"
"<html class=\"dark\" lang=\"en\">\n"
"<head>\n"
"<meta charset=\"utf-8\"/>\n"
"<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\"/>\n"
"<style>\n"
"* { margin:0; padding:0; box-sizing:border-box; }\n"
":root {\n"
"  --bg: #131313; --surface: #1b1b1c; --surface-high: #2a2a2a;\n"
"  --surface-highest: #353535; --surface-lowest: #0e0e0e;\n"
"  --primary: #adc6ff; --primary-dark: #4b8eff;\n"
"  --tertiary: #ffb595; --outline: #8b90a0;\n"
"  --on-surface: #e5e2e1; --on-bg: #e5e2e1;\n"
"  --error: #ff5f56; --warn: #ffbd2e; --ok: #27c93f;\n"
"}\n"
"body { font-family:'Inter','Segoe UI',system-ui,sans-serif; background:var(--bg);\n"
"  color:var(--on-surface); overflow:hidden; height:100vh; user-select:none; }\n"
"button { cursor:pointer; border:none; background:none; color:inherit; font:inherit; }\n"
"::-webkit-scrollbar { width:4px; } ::-webkit-scrollbar-track { background:transparent; }\n"
"::-webkit-scrollbar-thumb { background:rgba(255,255,255,0.1); border-radius:10px; }\n"
"\n"
"/* Layout */\n"
".app { display:flex; height:100vh; }\n"
".sidebar { width:64px; background:rgba(27,27,28,0.7); backdrop-filter:blur(30px);\n"
"  display:flex; flex-direction:column; align-items:center; padding:16px 0;\n"
"  border-right:1px solid rgba(255,255,255,0.06); z-index:50; flex-shrink:0; }\n"
".sidebar-icon { width:40px; height:40px; display:flex; align-items:center;\n"
"  justify-content:center; border-radius:10px; margin:4px 0; color:var(--outline);\n"
"  transition:all 0.2s; font-size:20px; }\n"
".sidebar-icon:hover { background:rgba(255,255,255,0.05); color:#fff; }\n"
".sidebar-icon.active { background:var(--surface-high); color:var(--primary);\n"
"  box-shadow:0 4px 12px rgba(0,0,0,0.4); }\n"
".sidebar-spacer { flex:1; }\n"
"\n"
".main-area { flex:1; display:flex; flex-direction:column; overflow:hidden; }\n"
".topbar { height:44px; display:flex; align-items:center; justify-content:space-between;\n"
"  padding:0 20px; border-bottom:1px solid rgba(255,255,255,0.06); flex-shrink:0;\n"
"  -webkit-app-region:drag; }\n"
".topbar * { -webkit-app-region:no-drag; }\n"
".topbar-title { font-weight:700; font-size:14px; letter-spacing:-0.02em; }\n"
".topbar-nav { display:flex; gap:24px; font-size:11px; font-weight:600;\n"
"  text-transform:uppercase; letter-spacing:0.1em; }\n"
".topbar-nav a { color:var(--outline); text-decoration:none; padding-bottom:4px;\n"
"  transition:color 0.2s; }\n"
".topbar-nav a.active { color:var(--primary); border-bottom:2px solid var(--primary); }\n"
".topbar-actions { display:flex; gap:8px; align-items:center; }\n"
".btn-deploy { background:linear-gradient(180deg,#adc6ff,#4b8eff); color:#001a41;\n"
"  padding:6px 16px; border-radius:8px; font-size:11px; font-weight:700;\n"
"  box-shadow:0 2px 8px rgba(75,142,255,0.3); }\n"
"\n"
".content { flex:1; display:flex; overflow:hidden; }\n"
"\n"
"/* Editor Panel */\n"
".editor-panel { flex:1; display:flex; flex-direction:column; background:var(--surface-lowest); }\n"
".editor-tabs { display:flex; height:36px; background:var(--surface); border-bottom:1px solid rgba(255,255,255,0.04); }\n"
".editor-tab { padding:0 16px; display:flex; align-items:center; gap:6px;\n"
"  font-size:12px; color:var(--outline); border-right:1px solid rgba(255,255,255,0.04); }\n"
".editor-tab.active { background:var(--surface-lowest); color:var(--primary);\n"
"  border-top:2px solid var(--primary); }\n"
".editor-content { flex:1; padding:20px; font-family:'Cascadia Code','JetBrains Mono',monospace;\n"
"  font-size:13px; line-height:1.7; overflow-y:auto; white-space:pre; }\n"
".line-num { color:rgba(139,144,160,0.3); display:inline-block; width:32px;\n"
"  text-align:right; margin-right:16px; user-select:none; }\n"
".kw { color:var(--primary); } .fn { color:#98b5f3; } .str { color:var(--tertiary); }\n"
".cm { color:var(--outline); font-style:italic; }\n"
"\n"
"/* Chat Panel */\n"
".chat-panel { width:340px; display:flex; flex-direction:column; background:var(--bg);\n"
"  border-left:1px solid rgba(255,255,255,0.04); flex-shrink:0; }\n"
".chat-header { padding:16px 20px; font-size:11px; font-weight:600;\n"
"  text-transform:uppercase; letter-spacing:0.1em; color:var(--outline);\n"
"  border-bottom:1px solid rgba(255,255,255,0.04); }\n"
".chat-messages { flex:1; overflow-y:auto; padding:16px; display:flex;\n"
"  flex-direction:column; gap:12px; }\n"
".chat-msg { padding:12px 16px; border-radius:12px; font-size:13px; line-height:1.5;\n"
"  max-width:95%; }\n"
".chat-msg.user { background:var(--surface-high); align-self:flex-end;\n"
"  border-bottom-right-radius:4px; }\n"
".chat-msg.ai { background:var(--surface); align-self:flex-start;\n"
"  border-bottom-left-radius:4px; border:1px solid rgba(255,255,255,0.04); }\n"
".chat-msg .role { font-size:10px; font-weight:600; text-transform:uppercase;\n"
"  letter-spacing:0.05em; margin-bottom:6px; }\n"
".chat-msg.user .role { color:var(--primary); }\n"
".chat-msg.ai .role { color:var(--tertiary); }\n"
".chat-input-wrap { padding:12px 16px; border-top:1px solid rgba(255,255,255,0.04); }\n"
".chat-input { width:100%; background:var(--surface-highest); border:none;\n"
"  border-radius:12px; padding:10px 16px; color:var(--on-surface); font-size:13px;\n"
"  outline:none; font-family:inherit; }\n"
".chat-input:focus { box-shadow:0 0 0 1px rgba(173,198,255,0.4); }\n"
".chat-input::placeholder { color:var(--outline); }\n"
"\n"
"/* Status Bar */\n"
".statusbar { height:28px; display:flex; align-items:center; padding:0 16px;\n"
"  font-size:11px; color:var(--outline); gap:16px;\n"
"  border-top:1px solid rgba(255,255,255,0.04); flex-shrink:0; }\n"
".status-dot { width:6px; height:6px; border-radius:50%; }\n"
".status-dot.ok { background:var(--ok); } .status-dot.warn { background:var(--warn); }\n"
"\n"
"/* Panels: Models, Knowledge, Logs */\n"
".panel-page { display:none; flex:1; overflow-y:auto; padding:32px; }\n"
".panel-page.active { display:block; }\n"
".panel-page h1 { font-size:28px; font-weight:700; letter-spacing:-0.03em; margin-bottom:8px; }\n"
".panel-page .subtitle { color:var(--outline); font-size:14px; margin-bottom:24px; }\n"
".cards-grid { display:grid; grid-template-columns:repeat(auto-fill,minmax(280px,1fr)); gap:16px; }\n"
".card { background:var(--surface); border-radius:12px; padding:20px;\n"
"  border:1px solid rgba(255,255,255,0.04); transition:all 0.2s; }\n"
".card:hover { border-color:rgba(173,198,255,0.15); transform:translateY(-1px); }\n"
".card-title { font-size:15px; font-weight:600; margin-bottom:4px; }\n"
".card-desc { font-size:12px; color:var(--outline); margin-bottom:12px; line-height:1.5; }\n"
".card-meta { display:flex; gap:12px; font-size:11px; color:var(--outline); margin-bottom:16px; }\n"
".badge { padding:2px 8px; border-radius:6px; font-size:10px; font-weight:600; }\n"
".badge-ok { background:rgba(39,201,63,0.15); color:#27c93f; }\n"
".badge-warn { background:rgba(255,189,46,0.15); color:#ffbd2e; }\n"
".stat-box { background:var(--surface); border-radius:12px; padding:20px;\n"
"  border:1px solid rgba(255,255,255,0.04); }\n"
".stat-label { font-size:10px; font-weight:600; text-transform:uppercase;\n"
"  letter-spacing:0.05em; color:var(--outline); margin-bottom:8px; }\n"
".stat-value { font-size:28px; font-weight:700; }\n"
"</style>\n"
"</head>\n"
"<body>\n"
"<div class=\"app\">\n"
"  <!-- Sidebar -->\n"
"  <div class=\"sidebar\">\n"
"    <div style=\"margin-bottom:16px;width:32px;height:32px;border-radius:8px;\n"
"      background:linear-gradient(135deg,#4b8eff,#adc6ff);display:flex;align-items:center;\n"
"      justify-content:center;font-weight:700;font-size:14px;color:#001a41;\">OA</div>\n"
"    <button class=\"sidebar-icon active\" onclick=\"showPage('editor')\" title=\"Editor\">&#x270E;</button>\n"
"    <button class=\"sidebar-icon\" onclick=\"showPage('models')\" title=\"Models\">&#x2699;</button>\n"
"    <button class=\"sidebar-icon\" onclick=\"showPage('knowledge')\" title=\"Knowledge\">&#x1F4BE;</button>\n"
"    <button class=\"sidebar-icon\" onclick=\"showPage('logs')\" title=\"Logs\">&#x2630;</button>\n"
"    <div class=\"sidebar-spacer\"></div>\n"
"    <button class=\"sidebar-icon\" title=\"Settings\">&#x2638;</button>\n"
"  </div>\n"
"\n"
"  <div class=\"main-area\">\n"
"    <!-- Top Bar -->\n"
"    <div class=\"topbar\">\n"
"      <div style=\"display:flex;align-items:center;gap:12px;\">\n"
"        <div style=\"display:flex;gap:6px;\">\n"
"          <div style=\"width:12px;height:12px;border-radius:50%;background:#ff5f56;\"></div>\n"
"          <div style=\"width:12px;height:12px;border-radius:50%;background:#ffbd2e;\"></div>\n"
"          <div style=\"width:12px;height:12px;border-radius:50%;background:#27c93f;\"></div>\n"
"        </div>\n"
"        <span class=\"topbar-title\">Open_Agents</span>\n"
"      </div>\n"
"      <div class=\"topbar-nav\">\n"
"        <a class=\"active\" href=\"#\">Project</a>\n"
"        <a href=\"#\">Build</a>\n"
"        <a href=\"#\">Debug</a>\n"
"      </div>\n"
"      <div class=\"topbar-actions\">\n"
"        <span id=\"gpu-info\" style=\"font-size:10px;color:var(--outline);\"></span>\n"
"        <button class=\"btn-deploy\">Deploy</button>\n"
"      </div>\n"
"    </div>\n"
"\n"
"    <!-- Editor Page -->\n"
"    <div class=\"content\" id=\"page-editor\">\n"
"      <div class=\"editor-panel\">\n"
"        <div class=\"editor-tabs\">\n"
"          <div class=\"editor-tab active\">main.c</div>\n"
"          <div class=\"editor-tab\">model.h</div>\n"
"        </div>\n"
"        <div class=\"editor-content\" id=\"code-area\" contenteditable=\"true\" spellcheck=\"false\">"
"<span class='line-num'> 1</span><span class='cm'>// Open_Agents — AI Coding Engine</span>\n"
"<span class='line-num'> 2</span><span class='kw'>#include</span> <span class='str'>\"core/inference.h\"</span>\n"
"<span class='line-num'> 3</span>\n"
"<span class='line-num'> 4</span><span class='kw'>int</span> <span class='fn'>main</span>(<span class='kw'>int</span> argc, <span class='kw'>char</span>** argv) {\n"
"<span class='line-num'> 5</span>    <span class='cm'>// Load GGUF model</span>\n"
"<span class='line-num'> 6</span>    oag_inference_t* inf = <span class='fn'>oag_inference_create</span>(argv[1]);\n"
"<span class='line-num'> 7</span>    <span class='kw'>if</span> (!inf) <span class='kw'>return</span> <span class='str'>1</span>;\n"
"<span class='line-num'> 8</span>\n"
"<span class='line-num'> 9</span>    <span class='cm'>// Start interactive mode</span>\n"
"<span class='line-num'>10</span>    <span class='fn'>run_interactive</span>(inf);\n"
"<span class='line-num'>11</span>    <span class='fn'>oag_inference_free</span>(inf);\n"
"<span class='line-num'>12</span>    <span class='kw'>return</span> <span class='str'>0</span>;\n"
"<span class='line-num'>13</span>}\n"
"</div>\n"
"      </div>\n"
"      <div class=\"chat-panel\">\n"
"        <div class=\"chat-header\">AI Assistant</div>\n"
"        <div class=\"chat-messages\" id=\"chat-messages\">\n"
"          <div class=\"chat-msg ai\"><div class=\"role\">AI</div>\n"
"            Welcome to Open_Agents! I can help you write, review, and debug code.\n"
"            Type a message below to get started.</div>\n"
"        </div>\n"
"        <div class=\"chat-input-wrap\">\n"
"          <input class=\"chat-input\" id=\"chat-input\" placeholder=\"Ask AI anything...\" />\n"
"        </div>\n"
"      </div>\n"
"    </div>\n"
"\n"
"    <!-- Models Page -->\n"
"    <div class=\"panel-page\" id=\"page-models\">\n"
"      <h1>Model Management</h1>\n"
"      <div class=\"subtitle\">Download and manage GGUF / ONNX models for local inference</div>\n"
"      <div style=\"display:flex;gap:16px;margin-bottom:24px;\">\n"
"        <div class=\"stat-box\" style=\"flex:1;\"><div class=\"stat-label\">GPU</div>\n"
"          <div class=\"stat-value\" id=\"stat-gpu\">Detecting...</div></div>\n"
"        <div class=\"stat-box\" style=\"flex:1;\"><div class=\"stat-label\">VRAM</div>\n"
"          <div class=\"stat-value\" id=\"stat-vram\">--</div></div>\n"
"        <div class=\"stat-box\" style=\"flex:1;\"><div class=\"stat-label\">Backend</div>\n"
"          <div class=\"stat-value\" id=\"stat-backend\">--</div></div>\n"
"      </div>\n"
"      <div class=\"cards-grid\">\n"
"        <div class=\"card\"><div class=\"badge badge-ok\">Recommended</div>\n"
"          <div class=\"card-title\">Qwen 2.5 Coder 7B</div>\n"
"          <div class=\"card-desc\">Best code generation model for its size. Supports 128K context.</div>\n"
"          <div class=\"card-meta\"><span>4.4 GB (Q4_K_M)</span><span>GGUF</span></div>\n"
"          <button class=\"btn-deploy\" style=\"width:100%;\">Download</button></div>\n"
"        <div class=\"card\"><div class=\"badge badge-ok\">Popular</div>\n"
"          <div class=\"card-title\">DeepSeek Coder V2 Lite</div>\n"
"          <div class=\"card-desc\">Lightweight yet powerful code model with MoE architecture.</div>\n"
"          <div class=\"card-meta\"><span>5.9 GB (Q4_K_M)</span><span>GGUF</span></div>\n"
"          <button class=\"btn-deploy\" style=\"width:100%;\">Download</button></div>\n"
"        <div class=\"card\">\n"
"          <div class=\"card-title\">Phi-4 Mini</div>\n"
"          <div class=\"card-desc\">Microsoft's compact model. Great for low-VRAM systems.</div>\n"
"          <div class=\"card-meta\"><span>2.3 GB (Q4_K_M)</span><span>GGUF</span></div>\n"
"          <button class=\"btn-deploy\" style=\"width:100%;\">Download</button></div>\n"
"        <div class=\"card\" style=\"border:1px dashed rgba(173,198,255,0.2);display:flex;\n"
"          align-items:center;justify-content:center;flex-direction:column;gap:8px;\">\n"
"          <div style=\"font-size:24px;\">+</div>\n"
"          <div style=\"font-size:12px;color:var(--outline);\">Import Custom GGUF/ONNX</div></div>\n"
"      </div>\n"
"    </div>\n"
"\n"
"    <!-- Knowledge Page -->\n"
"    <div class=\"panel-page\" id=\"page-knowledge\">\n"
"      <h1>Knowledge Base</h1>\n"
"      <div class=\"subtitle\">Manage and optimize RAG indices for your local context.</div>\n"
"      <div style=\"display:flex;gap:16px;margin-bottom:24px;\">\n"
"        <div class=\"stat-box\" style=\"flex:1;\"><div class=\"stat-label\">Total Tokens</div>\n"
"          <div class=\"stat-value\">0</div></div>\n"
"        <div class=\"stat-box\" style=\"flex:1;\"><div class=\"stat-label\">Indexed Files</div>\n"
"          <div class=\"stat-value\">0</div></div>\n"
"        <div class=\"stat-box\" style=\"flex:1;\"><div class=\"stat-label\">Index Health</div>\n"
"          <div class=\"stat-value\" style=\"color:var(--ok);\">Ready</div></div>\n"
"      </div>\n"
"      <div class=\"card\" style=\"padding:24px;\">\n"
"        <div class=\"card-title\">Add Folder to Index</div>\n"
"        <div class=\"card-desc\">Drag a folder here or click to browse. Supports code, docs, and text files.</div>\n"
"        <button class=\"btn-deploy\" style=\"margin-top:12px;\">+ Add Folder</button>\n"
"      </div>\n"
"    </div>\n"
"\n"
"    <!-- Logs Page -->\n"
"    <div class=\"panel-page\" id=\"page-logs\">\n"
"      <h1>Terminal &amp; Logs</h1>\n"
"      <div class=\"subtitle\">System output, build logs, and LLM interaction data.</div>\n"
"      <div style=\"display:flex;gap:16px;height:calc(100vh - 200px);\">\n"
"        <div style=\"flex:1;background:var(--surface-lowest);border-radius:12px;padding:16px;\n"
"          font-family:'Cascadia Code',monospace;font-size:12px;line-height:1.8;\n"
"          overflow-y:auto;\" id=\"log-terminal\">\n"
"<span style='color:var(--outline);'>$</span> open_agents --gpus<br/>\n"
"<span style='color:var(--ok);'>GPU detected: NVIDIA RTX 4090 (15.7 GB)</span><br/>\n"
"<span style='color:var(--outline);'>$</span> open_agents --nics<br/>\n"
"<span style='color:var(--primary);'>ETH: 192.168.1.18 (1 Gbps)</span><br/>\n"
"<span style='color:var(--primary);'>WiFi: 192.168.1.15 (1 Gbps)</span><br/>\n"
"<span style='color:var(--outline);'>$</span> <span id='log-cursor' style='animation:blink 1s infinite;'>_</span>\n"
"        </div>\n"
"        <div style=\"width:340px;background:var(--surface);border-radius:12px;padding:16px;\">\n"
"          <div style=\"font-size:11px;font-weight:600;text-transform:uppercase;\n"
"            letter-spacing:0.05em;color:var(--outline);margin-bottom:16px;\">LLM Interaction Data</div>\n"
"          <div id=\"llm-log\" style=\"font-size:12px;line-height:1.6;\n"
"            color:var(--outline);\">No interactions yet.</div>\n"
"        </div>\n"
"      </div>\n"
"    </div>\n"
"\n"
"    <!-- Status Bar -->\n"
"    <div class=\"statusbar\">\n"
"      <div style=\"display:flex;align-items:center;gap:6px;\">\n"
"        <div class=\"status-dot ok\"></div>\n"
"        <span>System Ready</span>\n"
"      </div>\n"
"      <span id=\"status-gpu\"></span>\n"
"      <span id=\"status-nic\"></span>\n"
"      <div style=\"flex:1;\"></div>\n"
"      <span>Open_Agents v0.3.5</span>\n"
"    </div>\n"
"  </div>\n"
"</div>\n"
"\n"
"<style>@keyframes blink { 0%,50% {opacity:1;} 51%,100% {opacity:0;} }</style>\n"
"\n"
"<script>\n"
"// Page navigation\n"
"function showPage(name) {\n"
"  document.querySelectorAll('.panel-page,.content').forEach(el => el.style.display='none');\n"
"  document.querySelectorAll('.sidebar-icon').forEach(el => el.classList.remove('active'));\n"
"  if (name === 'editor') {\n"
"    document.getElementById('page-editor').style.display = 'flex';\n"
"  } else {\n"
"    document.getElementById('page-'+name).style.display = 'block';\n"
"    document.getElementById('page-'+name).classList.add('active');\n"
"  }\n"
"  event.target.closest('.sidebar-icon').classList.add('active');\n"
"}\n"
"\n"
"// Chat functionality\n"
"const chatInput = document.getElementById('chat-input');\n"
"const chatMessages = document.getElementById('chat-messages');\n"
"\n"
"chatInput.addEventListener('keydown', (e) => {\n"
"  if (e.key === 'Enter' && chatInput.value.trim()) {\n"
"    const msg = chatInput.value.trim();\n"
"    addChatMessage('user', msg);\n"
"    chatInput.value = '';\n"
"    // Send to C backend via postMessage\n"
"    if (window.chrome && window.chrome.webview) {\n"
"      window.chrome.webview.postMessage(JSON.stringify({type:'chat',content:msg}));\n"
"    } else {\n"
"      // Fallback: echo\n"
"      setTimeout(() => addChatMessage('ai', 'Connect an AI model or API key to enable responses. Use --model or set OPENAI_API_KEY.'), 300);\n"
"    }\n"
"  }\n"
"});\n"
"\n"
"function addChatMessage(role, text) {\n"
"  const div = document.createElement('div');\n"
"  div.className = 'chat-msg ' + (role === 'user' ? 'user' : 'ai');\n"
"  div.innerHTML = '<div class=\"role\">' + (role === 'user' ? 'You' : 'AI') + '</div>' + escapeHtml(text);\n"
"  chatMessages.appendChild(div);\n"
"  chatMessages.scrollTop = chatMessages.scrollHeight;\n"
"}\n"
"\n"
"function escapeHtml(str) {\n"
"  return str.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;')\n"
"    .replace(/\"/g,'&quot;').replace(/\\n/g,'<br/>');\n"
"}\n"
"\n"
"// Receive messages from C backend\n"
"if (window.chrome && window.chrome.webview) {\n"
"  window.chrome.webview.addEventListener('message', (e) => {\n"
"    const data = JSON.parse(e.data);\n"
"    if (data.type === 'chat_response') addChatMessage('ai', data.content);\n"
"    if (data.type === 'gpu_info') {\n"
"      document.getElementById('stat-gpu').textContent = data.name;\n"
"      document.getElementById('stat-vram').textContent = data.vram;\n"
"      document.getElementById('stat-backend').textContent = data.backend;\n"
"      document.getElementById('status-gpu').textContent = data.name + ' | ' + data.vram;\n"
"    }\n"
"    if (data.type === 'log') {\n"
"      const term = document.getElementById('log-terminal');\n"
"      const cursor = document.getElementById('log-cursor');\n"
"      term.insertBefore(document.createTextNode(data.text + '\\n'), cursor);\n"
"    }\n"
"  });\n"
"}\n"
"</script>\n"
"</body>\n"
"</html>";

    return strdup(html);
}

// ============================================================
// Create
// ============================================================

oag_gui_config_t oag_gui_default_config(void) {
    oag_gui_config_t c;
    c.width = 1400;
    c.height = 900;
    c.maximized = false;
    c.frameless = false;
    strncpy(c.title, "Open_Agents — The Intelligent Workspace", sizeof(c.title) - 1);
    return c;
}

oag_gui_t* oag_gui_create(oag_gui_config_t config) {
    oag_gui_t* gui = (oag_gui_t*)calloc(1, sizeof(oag_gui_t));
    gui->config = config;
    gui->hinst = GetModuleHandleA(NULL);

    // Register window class
    WNDCLASSEXA wc = {0};
    wc.cbSize = sizeof(wc);
    wc.style = CS_HREDRAW | CS_VREDRAW;
    wc.lpfnWndProc = wnd_proc;
    wc.hInstance = gui->hinst;
    wc.hCursor = LoadCursorA(NULL, IDC_ARROW);
    wc.hbrBackground = CreateSolidBrush(RGB(0x13, 0x13, 0x13));
    wc.lpszClassName = "OpenAgentsWindow";
    wc.hIcon = LoadIconA(NULL, IDI_APPLICATION);
    RegisterClassExA(&wc);

    // Create window
    DWORD style = WS_OVERLAPPEDWINDOW;
    RECT rc = {0, 0, config.width, config.height};
    AdjustWindowRect(&rc, style, FALSE);

    gui->hwnd = CreateWindowExA(
        0, "OpenAgentsWindow", config.title,
        style, CW_USEDEFAULT, CW_USEDEFAULT,
        rc.right - rc.left, rc.bottom - rc.top,
        NULL, NULL, gui->hinst, NULL);

    SetWindowLongPtrA(gui->hwnd, GWLP_USERDATA, (LONG_PTR)gui);

    // Build HTML
    gui->html_content = build_html_content();

    // Try to load WebView2
    gui->hlib_wv2 = LoadLibraryA("WebView2Loader.dll");
    if (!gui->hlib_wv2) {
        // Try system path
        char path[512];
        GetSystemDirectoryA(path, sizeof(path));
        strcat(path, "\\WebView2Loader.dll");
        gui->hlib_wv2 = LoadLibraryA(path);
    }

    if (gui->hlib_wv2) {
        printf("[GUI] WebView2 found — full GUI mode\n");
        // In full implementation: call CreateCoreWebView2EnvironmentWithOptions
        // and navigate to data:text/html or use NavigateToString
    } else {
        printf("[GUI] WebView2 not found — writing HTML to temp file\n");
        printf("[GUI] Install Edge WebView2 Runtime for embedded GUI\n");

        // Fallback: write HTML to temp file and open in default browser
        char temp_path[MAX_PATH];
        GetTempPathA(MAX_PATH, temp_path);
        strcat(temp_path, "open_agents_ui.html");

        FILE* f = fopen(temp_path, "w");
        if (f) {
            fprintf(f, "%s", gui->html_content);
            fclose(f);

            // Open in browser
            ShellExecuteA(NULL, "open", temp_path, NULL, NULL, SW_SHOWNORMAL);
            printf("[GUI] Opened UI in browser: %s\n", temp_path);
        }
    }

    return gui;
}

void oag_gui_run(oag_gui_t* gui) {
    if (!gui || !gui->hwnd) return;

    if (gui->hlib_wv2) {
        // Show window and run message loop
        ShowWindow(gui->hwnd, SW_SHOW);
        UpdateWindow(gui->hwnd);

        MSG msg;
        while (GetMessageA(&msg, NULL, 0, 0)) {
            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
    } else {
        // Browser fallback — wait for user to close
        printf("[GUI] Running in browser mode. Press Enter to exit.\n");
        getchar();
    }
}

void oag_gui_free(oag_gui_t* gui) {
    if (!gui) return;
    free(gui->html_content);
    if (gui->hlib_wv2) FreeLibrary(gui->hlib_wv2);
    if (gui->hwnd) DestroyWindow(gui->hwnd);
    free(gui);
}

void oag_gui_post_message(oag_gui_t* gui, const char* json) {
    (void)gui; (void)json;
    // In full WebView2 implementation:
    // ICoreWebView2::PostWebMessageAsJson(json)
}

#else
oag_gui_config_t oag_gui_default_config(void) {
    return (oag_gui_config_t){ .width = 1400, .height = 900 };
}
oag_gui_t* oag_gui_create(oag_gui_config_t c) { (void)c; return NULL; }
void oag_gui_run(oag_gui_t* g) { (void)g; }
void oag_gui_free(oag_gui_t* g) { free(g); }
void oag_gui_post_message(oag_gui_t* g, const char* j) { (void)g; (void)j; }
#endif
