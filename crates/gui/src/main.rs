use gpui::*;

// ============================================================
// Figma Design Colors (VS Code Dark / macOS style)
// From: figma.com/make/R9zPYmdxXBKlNsptbTt6dF
// ============================================================

const BG: u32 = 0x1e1e1e;          // Main background
const SIDEBAR_BG: u32 = 0x252526;   // Sidebar
const TITLEBAR_BG: u32 = 0x2d2d2d;  // Title bar
const BORDER: u32 = 0x3d3d3d;       // Borders
const HOVER_BG: u32 = 0x37373d;     // Active/hover state
const PANEL_BG: u32 = 0x252526;     // Panel headers
const TEXT_PRIMARY: u32 = 0xe5e5e5;  // Primary text (gray-200)
const TEXT_SECONDARY: u32 = 0x9ca3af; // Secondary text (gray-400)
const TEXT_MUTED: u32 = 0x6b7280;    // Muted text (gray-500)
const TEXT_DIM: u32 = 0x4b5563;      // Dim text (gray-600)
const ACCENT_BLUE: u32 = 0x2563eb;   // Blue accent (blue-600)
const ACCENT_ORANGE: u32 = 0xfb923c; // Orange accent
const ACCENT_PINK: u32 = 0xdb2777;   // Pink accent
const STATUSBAR_BG: u32 = 0x007acc;  // VS Code blue status bar
const TRAFFIC_RED: u32 = 0xff5f57;
const TRAFFIC_YELLOW: u32 = 0xfebc2e;
const TRAFFIC_GREEN: u32 = 0x28c840;
const PURPLE: u32 = 0xa855f7;

fn hex(c: u32) -> Hsla {
    let r = ((c >> 16) & 0xFF) as f32 / 255.0;
    let g = ((c >> 8) & 0xFF) as f32 / 255.0;
    let b = (c & 0xFF) as f32 / 255.0;
    Rgba { r, g, b, a: 1.0 }.into()
}

fn hex_a(c: u32, a: f32) -> Hsla {
    let r = ((c >> 16) & 0xFF) as f32 / 255.0;
    let g = ((c >> 8) & 0xFF) as f32 / 255.0;
    let b = (c & 0xFF) as f32 / 255.0;
    Rgba { r, g, b, a }.into()
}

// ============================================================
// State
// ============================================================

#[derive(Clone, Copy, PartialEq)]
enum Page { Editor, Chat, Settings, Terminal }

struct ChatMsg { role: String, content: String }

struct FileEntry { name: &'static str, is_folder: bool }

struct AppView {
    page: Page,
    chat_messages: Vec<ChatMsg>,
    code: String,
    files: Vec<FileEntry>,
}

// ============================================================
// Render
// ============================================================

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let content: AnyElement = match self.page {
            Page::Editor => self.render_editor().into_any_element(),
            Page::Chat => self.render_chat_page().into_any_element(),
            Page::Settings => self.render_settings().into_any_element(),
            Page::Terminal => self.render_terminal().into_any_element(),
        };

        // Root: rounded window with macOS feel
        div()
            .size_full()
            .bg(hex(BG))
            .flex()
            .flex_col()
            .rounded(px(20.))
            .overflow_hidden()
            .child(self.render_titlebar())
            .child(
                div()
                    .flex_1()
                    .flex()
                    .overflow_hidden()
                    .child(self.render_sidebar())
                    // Separator
                    .child(div().w(px(1.)).h_full().bg(hex(BORDER)))
                    .child(
                        div().flex_1().flex().flex_col().child(content),
                    ),
            )
    }
}

impl AppView {
    // ============================================================
    // Title Bar (macOS traffic lights)
    // ============================================================

    fn render_titlebar(&self) -> impl IntoElement {
        // The entire titlebar is a canvas that registers window control hitboxes
        // for proper WM_NCHITTEST handling (drag, close, min, max)
        div()
            .h(px(44.))
            .bg(hex(TITLEBAR_BG))
            .border_b_1()
            .border_color(hex(BORDER))
            .child(
                canvas(
                    // Prepaint: allocate hitboxes
                    |bounds, window, _cx| {
                        // Drag region = entire titlebar (minus button area on left)
                        let drag_bounds = Bounds {
                            origin: point(bounds.origin.x + px(80.), bounds.origin.y),
                            size: size(bounds.size.width - px(80.), bounds.size.height),
                        };
                        let drag_hitbox = window.insert_hitbox(drag_bounds, HitboxBehavior::Normal);

                        // Button hitboxes (x=16, y=16, 12x12 each, 8px gap)
                        let close_bounds = Bounds {
                            origin: point(bounds.origin.x + px(16.), bounds.origin.y + px(16.)),
                            size: size(px(12.), px(12.)),
                        };
                        let close_hitbox = window.insert_hitbox(close_bounds, HitboxBehavior::Normal);

                        let min_bounds = Bounds {
                            origin: point(bounds.origin.x + px(36.), bounds.origin.y + px(16.)),
                            size: size(px(12.), px(12.)),
                        };
                        let min_hitbox = window.insert_hitbox(min_bounds, HitboxBehavior::Normal);

                        let max_bounds = Bounds {
                            origin: point(bounds.origin.x + px(56.), bounds.origin.y + px(16.)),
                            size: size(px(12.), px(12.)),
                        };
                        let max_hitbox = window.insert_hitbox(max_bounds, HitboxBehavior::Normal);

                        (drag_hitbox, close_hitbox, min_hitbox, max_hitbox)
                    },
                    // Paint: register hitboxes as window controls + draw UI
                    |bounds, (drag_hb, close_hb, min_hb, max_hb), window, _cx| {
                        // Register window control areas for WM_NCHITTEST
                        window.insert_window_control_hitbox(WindowControlArea::Drag, drag_hb);
                        window.insert_window_control_hitbox(WindowControlArea::Close, close_hb);
                        window.insert_window_control_hitbox(WindowControlArea::Min, min_hb);
                        window.insert_window_control_hitbox(WindowControlArea::Max, max_hb);

                        // Draw traffic light buttons
                        let btn_y = bounds.origin.y + px(16.);
                        let btn_r = px(6.);

                        // Close (red)
                        let close_center = point(bounds.origin.x + px(22.), btn_y + btn_r);
                        window.paint_quad(fill(
                            Bounds::centered_at(close_center, size(px(12.), px(12.))),
                            hex(TRAFFIC_RED),
                        ).corner_radii(px(6.)));

                        // Minimize (yellow)
                        let min_center = point(bounds.origin.x + px(42.), btn_y + btn_r);
                        window.paint_quad(fill(
                            Bounds::centered_at(min_center, size(px(12.), px(12.))),
                            hex(TRAFFIC_YELLOW),
                        ).corner_radii(px(6.)));

                        // Maximize (green)
                        let max_center = point(bounds.origin.x + px(62.), btn_y + btn_r);
                        window.paint_quad(fill(
                            Bounds::centered_at(max_center, size(px(12.), px(12.))),
                            hex(TRAFFIC_GREEN),
                        ).corner_radii(px(6.)));

                        // Title text (centered)
                        let title = "AI Coding Assistant";
                        let title_origin = point(
                            bounds.origin.x + bounds.size.width / 2.0 - px(60.),
                            bounds.origin.y + px(14.),
                        );
                        window.paint_quad(fill(
                            Bounds { origin: title_origin, size: size(px(0.), px(0.)) },
                            hex_a(0x000000, 0.0),
                        ));
                    },
                )
                .size_full(),
            )
            // Overlay: title text via regular div (canvas can't easily do text)
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(80.))
                    .right(px(60.))
                    .h(px(44.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .text_color(hex(TEXT_SECONDARY))
                    .child("AI Coding Assistant"),
            )
    }

    // ============================================================
    // Sidebar (Figma: Editor / Chat / Settings + Explorer + Terminal)
    // ============================================================

    fn render_sidebar(&self) -> impl IntoElement {
        div()
            .w(px(220.))
            .h_full()
            .bg(hex(SIDEBAR_BG))
            .flex()
            .flex_col()
            .overflow_hidden()
            // Navigation
            .child(
                div()
                    .p(px(12.))
                    .pt(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(self.nav_item("Editor", Page::Editor))
                    .child(self.nav_item("Chat", Page::Chat))
                    .child(self.nav_item("Settings", Page::Settings)),
            )
            // File Explorer
            .child(
                div()
                    .flex_1()
                    .border_t_1()
                    .border_color(hex(BORDER))
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    // EXPLORER header
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .mb(px(4.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .child("📂"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("EXPLORER"),
                            ),
                    )
                    // Action buttons row (Figma: FilePlus, FolderPlus, RefreshCw, FolderOpen)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .mb(px(8.))
                            .px(px(8.))
                            // 新規ファイル
                            .child(self.explorer_action_btn("📄+", "新規ファイル"))
                            // 新規フォルダ
                            .child(self.explorer_action_btn("📁+", "新規フォルダ"))
                            // 更新
                            .child(self.explorer_action_btn("🔄", "更新"))
                            // フォルダを開く
                            .child(self.explorer_action_btn("📂", "フォルダを開く")),
                    )
                    // File list
                    .children(self.files.iter().map(|f| {
                        let icon = if f.is_folder { "📁" } else { "📄" };
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .text_size(px(13.))
                            .text_color(hex(TEXT_SECONDARY))
                            .child(icon)
                            .child(f.name.to_string())
                    })),
            )
            // Terminal shortcut at bottom
            .child(
                div()
                    .border_t_1()
                    .border_color(hex(BORDER))
                    .p(px(12.))
                    .child(self.nav_item("Terminal", Page::Terminal)),
            )
    }

    fn explorer_action_btn(&self, icon: &str, _title: &str) -> impl IntoElement {
        div()
            .p(px(4.))
            .rounded(px(4.))
            .text_size(px(12.))
            .text_color(hex(TEXT_SECONDARY))
            .cursor_pointer()
            .child(icon.to_string())
    }

    fn nav_item(&self, label: &str, page: Page) -> impl IntoElement {
        let active = self.page == page;
        let bg = if active { hex(HOVER_BG) } else { hex_a(0x000000, 0.0) };
        let fg = if active { hex(TEXT_PRIMARY) } else { hex(TEXT_SECONDARY) };

        let icon = match page {
            Page::Editor => "💻",
            Page::Chat => "💬",
            Page::Settings => "⚙",
            Page::Terminal => "▶",
        };

        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(6.))
            .bg(bg)
            .text_color(fg)
            .text_size(px(13.))
            .cursor_pointer()
            .child(icon)
            .child(label.to_string())
    }

    // ============================================================
    // Editor View (Figma: tabs + code + line numbers + status bar)
    // ============================================================

    fn render_editor(&self) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .bg(hex(BG))
            // Editor Header (tabs + Run button)
            .child(
                div()
                    .h(px(48.))
                    .bg(hex(PANEL_BG))
                    .border_b_1()
                    .border_color(hex(BORDER))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.))
                    // Tabs
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(
                                div()
                                    .px(px(12.))
                                    .py(px(6.))
                                    .bg(hex(BG))
                                    .border_1()
                                    .border_color(hex(BORDER))
                                    .rounded_t(px(6.))
                                    .text_size(px(12.))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child("App.tsx")
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_MUTED))
                                            .child("×"),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(12.))
                                    .py(px(6.))
                                    .text_size(px(12.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .child("index.tsx"),
                            ),
                    )
                    // Run button
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .px(px(12.))
                                    .py(px(6.))
                                    .bg(hex(ACCENT_BLUE))
                                    .rounded(px(6.))
                                    .text_size(px(12.))
                                    .text_color(hex(0xFFFFFF))
                                    .font_weight(FontWeight::MEDIUM)
                                    .cursor_pointer()
                                    .child("▶ Run"),
                            ),
                    ),
            )
            // Code content with line numbers
            .child(
                div()
                    .flex_1()
                    .p(px(24.))
                    .text_size(px(13.))
                    .font_family("Cascadia Code")
                    .overflow_hidden()
                    .child(
                        div().flex().flex_col().gap(px(2.)).children(
                            self.code.lines().enumerate().map(|(i, line)| {
                                div()
                                    .flex()
                                    .child(
                                        div()
                                            .w(px(48.))
                                            .text_align(TextAlign::Right)
                                            .pr(px(16.))
                                            .text_color(hex(TEXT_DIM))
                                            .child(format!("{}", i + 1)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child(line.to_string()),
                                    )
                            }),
                        ),
                    ),
            )
            // Status bar (VS Code blue)
            .child(
                div()
                    .h(px(32.))
                    .bg(hex(STATUSBAR_BG))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.))
                    .text_size(px(11.))
                    .text_color(hex(0xFFFFFF))
                    .child(
                        div()
                            .flex()
                            .gap(px(16.))
                            .child("TypeScript React")
                            .child("UTF-8")
                            .child("LF"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(16.))
                            .child("Ln 1, Col 1")
                            .child("Spaces: 2"),
                    ),
            )
    }

    // ============================================================
    // Chat View (Figma: Open Agents branded, suggestions, messages)
    // ============================================================

    fn render_chat_page(&self) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .bg(hex(BG))
            // Chat header
            .child(
                div()
                    .h(px(48.))
                    .bg(hex(PANEL_BG))
                    .border_b_1()
                    .border_color(hex(BORDER))
                    .flex()
                    .items_center()
                    .px(px(16.))
                    .gap(px(8.))
                    // Gradient icon
                    .child(
                        div()
                            .w(px(24.))
                            .h(px(24.))
                            .rounded(px(6.))
                            .bg(hex(ACCENT_ORANGE))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(12.))
                            .text_color(hex(0xFFFFFF))
                            .child("✦"),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(hex(TEXT_PRIMARY))
                            .child("Open Agents"),
                    ),
            )
            // Messages area
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(
                        div()
                            .max_w(px(800.))
                            .mx_auto()
                            .px(px(24.))
                            .py(px(32.))
                            .flex()
                            .flex_col()
                            .gap(px(24.))
                            // Suggestions (show when only welcome msg)
                            .child(
                                div()
                                    .mb(px(16.))
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_MUTED))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .mb(px(12.))
                                            .child("提案"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .gap(px(8.))
                                            .child(self.suggestion_chip("Reactコンポーネントを作成"))
                                            .child(self.suggestion_chip("バグを修正"))
                                            .child(self.suggestion_chip("コードをリファクタリング"))
                                            .child(self.suggestion_chip("テストを追加")),
                                    ),
                            )
                            // Messages
                            .children(self.chat_messages.iter().map(|msg| {
                                let is_user = msg.role == "user";
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    // Role header
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(if is_user {
                                                div()
                                                    .w(px(24.))
                                                    .h(px(24.))
                                                    .rounded(px(6.))
                                                    .bg(hex(ACCENT_BLUE))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(11.))
                                                    .text_color(hex(0xFFFFFF))
                                                    .child("U")
                                                    .into_any_element()
                                            } else {
                                                div()
                                                    .w(px(24.))
                                                    .h(px(24.))
                                                    .rounded(px(6.))
                                                    .bg(hex(ACCENT_ORANGE))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(11.))
                                                    .text_color(hex(0xFFFFFF))
                                                    .child("✦")
                                                    .into_any_element()
                                            })
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(hex(TEXT_SECONDARY))
                                                    .child(if is_user { "You" } else { "Agent" }),
                                            ),
                                    )
                                    // Content
                                    .child(
                                        div()
                                            .pl(px(32.))
                                            .text_size(px(13.))
                                            .text_color(hex(TEXT_PRIMARY))
                                            .child(msg.content.clone()),
                                    )
                            })),
                    ),
            )
            // Input area
            .child(
                div()
                    .border_t_1()
                    .border_color(hex(BORDER))
                    .bg(hex(PANEL_BG))
                    .p(px(16.))
                    .child(
                        div()
                            .max_w(px(800.))
                            .mx_auto()
                            .child(
                                div()
                                    .w_full()
                                    .bg(hex(BG))
                                    .border_1()
                                    .border_color(hex(BORDER))
                                    .rounded(px(12.))
                                    .px(px(16.))
                                    .py(px(12.))
                                    .text_size(px(13.))
                                    .text_color(hex(TEXT_MUTED))
                                    .child("メッセージを入力してください..."),
                            )
                            .child(
                                div()
                                    .mt(px(8.))
                                    .text_size(px(11.))
                                    .text_color(hex(TEXT_MUTED))
                                    .child("Enter で送信、Shift + Enter で改行"),
                            ),
                    ),
            )
    }

    fn suggestion_chip(&self, label: &str) -> impl IntoElement {
        div()
            .px(px(16.))
            .py(px(12.))
            .bg(hex(PANEL_BG))
            .border_1()
            .border_color(hex(BORDER))
            .rounded(px(8.))
            .text_size(px(12.))
            .text_color(hex(TEXT_SECONDARY))
            .cursor_pointer()
            .child(label.to_string())
    }

    // ============================================================
    // Settings View (Figma: LLM settings, appearance, API keys)
    // ============================================================

    fn render_settings(&self) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .bg(hex(BG))
            .child(
                div()
                    .h(px(48.))
                    .bg(hex(PANEL_BG))
                    .border_b_1()
                    .border_color(hex(BORDER))
                    .flex()
                    .items_center()
                    .px(px(16.))
                    .gap(px(8.))
                    .child(div().text_size(px(13.)).text_color(hex(TEXT_SECONDARY)).child("⚙"))
                    .child(div().text_size(px(13.)).text_color(hex(TEXT_PRIMARY)).child("Settings")),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(
                        div()
                            .max_w(px(700.))
                            .mx_auto()
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .gap(px(32.))
                            // LLM Settings
                            .child(self.settings_section("ローカルLLM設定", "🔶", vec![
                                ("モデル形式", "GGUF / ONNX"),
                                ("Temperature", "0.7"),
                                ("最大トークン数", "2048"),
                                ("コンテキスト長", "4096"),
                            ]))
                            // Hardware
                            .child(self.settings_section("ハードウェア設定", "🖥", vec![
                                ("GPU アクセラレーション", "ON"),
                                ("GPU レイヤー数", "32"),
                                ("スレッド数", "8"),
                                ("バッチサイズ", "512"),
                            ]))
                            // Appearance
                            .child(self.settings_section("外観", "🎨", vec![
                                ("テーマ", "Dark"),
                                ("フォントサイズ", "14px"),
                                ("行番号を表示", "ON"),
                            ]))
                            // API Keys
                            .child(self.settings_section("APIキー管理", "🔑", vec![
                                ("OpenAI", "sk-••••••••"),
                                ("Anthropic", "未設定"),
                            ])),
                    ),
            )
    }

    fn settings_section(&self, title: &str, icon: &str, items: Vec<(&str, &str)>) -> impl IntoElement {
        let mut section = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mb(px(4.))
                    .child(div().text_size(px(16.)).child(icon.to_string()))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(hex(TEXT_PRIMARY))
                            .child(title.to_string()),
                    ),
            );

        let mut card = div()
            .bg(hex(PANEL_BG))
            .rounded(px(8.))
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(16.));

        for (label, value) in items {
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(hex(TEXT_PRIMARY))
                            .child(label.to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(hex(TEXT_SECONDARY))
                            .child(value.to_string()),
                    ),
            );
        }

        section = section.child(card);
        section
    }

    // ============================================================
    // Terminal View (Figma: green prompt, command history)
    // ============================================================

    fn render_terminal(&self) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .bg(hex(BG))
            .child(
                div()
                    .h(px(44.))
                    .bg(hex(TITLEBAR_BG))
                    .border_b_1()
                    .border_color(hex(BORDER))
                    .flex()
                    .items_center()
                    .px(px(16.))
                    .gap(px(8.))
                    .child(div().text_size(px(13.)).child("▶"))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(hex(TEXT_SECONDARY))
                            .child("Terminal"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .p(px(16.))
                    .font_family("Cascadia Code")
                    .text_size(px(12.))
                    .text_color(hex(TEXT_PRIMARY))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child("Open Agents Terminal v1.0.0")
                            .child("Type 'help' for available commands")
                            .child("")
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_color(hex(TRAFFIC_GREEN))
                                            .child("$"),
                                    )
                                    .child("open_agents --gpus"),
                            )
                            .child(
                                div()
                                    .text_color(hex(TRAFFIC_GREEN))
                                    .child("GPU detected: NVIDIA RTX 4090 (15.7 GB)"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(div().text_color(hex(TRAFFIC_GREEN)).child("$"))
                                    .child("_"),
                            ),
                    ),
            )
    }
}

// ============================================================
// Entry Point
// ============================================================

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Open_Agents — AI Coding Assistant".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(16.), px(16.)),),
                }),
                ..Default::default()
            },
            |window, cx| {
                // Allow custom titlebar drag
                window.set_client_inset(px(44.));

                cx.new(|_cx| AppView {
                    page: Page::Editor,
                    chat_messages: vec![ChatMsg {
                        role: "assistant".into(),
                        content: "こんにちは！Open Agents AIコーディングアシスタントです。コードの作成、編集、リファクタリングなど、お手伝いします。".into(),
                    }],
                    code: "import React from 'react';\nimport { useState } from 'react';\n\nfunction App() {\n  const [count, setCount] = useState(0);\n\n  return (\n    <div className=\"app\">\n      <h1>Counter App</h1>\n      <div className=\"counter\">\n        <button onClick={() => setCount(count - 1)}>-</button>\n        <span>{count}</span>\n        <button onClick={() => setCount(count + 1)}>+</button>\n      </div>\n    </div>\n  );\n}\n\nexport default App;".into(),
                    files: vec![
                        FileEntry { name: "src", is_folder: true },
                        FileEntry { name: "App.tsx", is_folder: false },
                        FileEntry { name: "index.tsx", is_folder: false },
                        FileEntry { name: "components", is_folder: true },
                        FileEntry { name: "utils.ts", is_folder: false },
                        FileEntry { name: "styles.css", is_folder: false },
                    ],
                })
            },
        )
        .unwrap();
    });
}
