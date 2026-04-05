mod editor;

use editor::EditorView;
use gpui::*;

// ============================================================
// Figma Design Colors (VS Code Dark / macOS style)
// ============================================================

pub const BG: u32 = 0x1e1e1e;
pub const SIDEBAR_BG: u32 = 0x252526;
pub const TITLEBAR_BG: u32 = 0x2d2d2d;
pub const BORDER: u32 = 0x3d3d3d;
pub const HOVER_BG: u32 = 0x37373d;
pub const PANEL_BG: u32 = 0x252526;
pub const TEXT_PRIMARY: u32 = 0xe5e5e5;
pub const TEXT_SECONDARY: u32 = 0x9ca3af;
pub const TEXT_MUTED: u32 = 0x6b7280;
pub const TEXT_DIM: u32 = 0x4b5563;
pub const ACCENT_BLUE: u32 = 0x2563eb;
pub const ACCENT_ORANGE: u32 = 0xfb923c;
#[allow(dead_code)]
pub const ACCENT_PINK: u32 = 0xdb2777;
pub const STATUSBAR_BG: u32 = 0x007acc;
pub const TRAFFIC_RED: u32 = 0xff5f57;
pub const TRAFFIC_YELLOW: u32 = 0xfebc2e;
pub const TRAFFIC_GREEN: u32 = 0x28c840;
#[allow(dead_code)]
pub const PURPLE: u32 = 0xa855f7;

pub fn hex(c: u32) -> Hsla {
    let r = ((c >> 16) & 0xFF) as f32 / 255.0;
    let g = ((c >> 8) & 0xFF) as f32 / 255.0;
    let b = (c & 0xFF) as f32 / 255.0;
    Rgba { r, g, b, a: 1.0 }.into()
}

pub fn hex_a(c: u32, a: f32) -> Hsla {
    let r = ((c >> 16) & 0xFF) as f32 / 255.0;
    let g = ((c >> 8) & 0xFF) as f32 / 255.0;
    let b = (c & 0xFF) as f32 / 255.0;
    Rgba { r, g, b, a }.into()
}

// ============================================================
// State
// ============================================================

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Editor,
    Chat,
    Settings,
    Terminal,
}

struct ChatMsg {
    role: String,
    content: String,
}

struct FileEntry {
    name: &'static str,
    is_folder: bool,
}

struct AppView {
    page: Page,
    chat_messages: Vec<ChatMsg>,
    files: Vec<FileEntry>,
    editor_view: Entity<EditorView>,
}

// ============================================================
// Render
// ============================================================

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content: AnyElement = match self.page {
            Page::Editor => self.render_editor(cx).into_any_element(),
            Page::Chat => self.render_chat_page().into_any_element(),
            Page::Settings => self.render_settings().into_any_element(),
            Page::Terminal => self.render_terminal().into_any_element(),
        };

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
                    .child(div().w(px(1.)).h_full().bg(hex(BORDER)))
                    .child(
                        div().flex_1().flex().flex_col().child(content),
                    ),
            )
    }
}

impl AppView {
    // ============================================================
    // Title Bar
    // ============================================================

    fn render_titlebar(&self) -> impl IntoElement {
        div()
            .h(px(44.))
            .bg(hex(TITLEBAR_BG))
            .border_b_1()
            .border_color(hex(BORDER))
            .child(
                canvas(
                    |bounds, window, _cx| {
                        let drag_bounds = Bounds {
                            origin: point(bounds.origin.x + px(80.), bounds.origin.y),
                            size: size(bounds.size.width - px(80.), bounds.size.height),
                        };
                        let drag_hitbox =
                            window.insert_hitbox(drag_bounds, HitboxBehavior::Normal);

                        let close_bounds = Bounds {
                            origin: point(bounds.origin.x + px(16.), bounds.origin.y + px(16.)),
                            size: size(px(12.), px(12.)),
                        };
                        let close_hitbox =
                            window.insert_hitbox(close_bounds, HitboxBehavior::Normal);

                        let min_bounds = Bounds {
                            origin: point(bounds.origin.x + px(36.), bounds.origin.y + px(16.)),
                            size: size(px(12.), px(12.)),
                        };
                        let min_hitbox =
                            window.insert_hitbox(min_bounds, HitboxBehavior::Normal);

                        let max_bounds = Bounds {
                            origin: point(bounds.origin.x + px(56.), bounds.origin.y + px(16.)),
                            size: size(px(12.), px(12.)),
                        };
                        let max_hitbox =
                            window.insert_hitbox(max_bounds, HitboxBehavior::Normal);

                        (drag_hitbox, close_hitbox, min_hitbox, max_hitbox)
                    },
                    |bounds, (drag_hb, close_hb, min_hb, max_hb), window, _cx| {
                        window.insert_window_control_hitbox(WindowControlArea::Drag, drag_hb);
                        window.insert_window_control_hitbox(WindowControlArea::Close, close_hb);
                        window.insert_window_control_hitbox(WindowControlArea::Min, min_hb);
                        window.insert_window_control_hitbox(WindowControlArea::Max, max_hb);

                        let btn_y = bounds.origin.y + px(16.);
                        let btn_r = px(6.);

                        let close_center = point(bounds.origin.x + px(22.), btn_y + btn_r);
                        window.paint_quad(
                            fill(
                                Bounds::centered_at(close_center, size(px(12.), px(12.))),
                                hex(TRAFFIC_RED),
                            )
                            .corner_radii(px(6.)),
                        );

                        let min_center = point(bounds.origin.x + px(42.), btn_y + btn_r);
                        window.paint_quad(
                            fill(
                                Bounds::centered_at(min_center, size(px(12.), px(12.))),
                                hex(TRAFFIC_YELLOW),
                            )
                            .corner_radii(px(6.)),
                        );

                        let max_center = point(bounds.origin.x + px(62.), btn_y + btn_r);
                        window.paint_quad(
                            fill(
                                Bounds::centered_at(max_center, size(px(12.), px(12.))),
                                hex(TRAFFIC_GREEN),
                            )
                            .corner_radii(px(6.)),
                        );
                    },
                )
                .size_full(),
            )
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
    // Sidebar
    // ============================================================

    fn render_sidebar(&self) -> impl IntoElement {
        div()
            .w(px(220.))
            .h_full()
            .bg(hex(SIDEBAR_BG))
            .flex()
            .flex_col()
            .overflow_hidden()
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
            .child(
                div()
                    .flex_1()
                    .border_t_1()
                    .border_color(hex(BORDER))
                    .p(px(12.))
                    .flex()
                    .flex_col()
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
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .mb(px(8.))
                            .px(px(8.))
                            .child(self.explorer_action_btn("📄+"))
                            .child(self.explorer_action_btn("📁+"))
                            .child(self.explorer_action_btn("🔄"))
                            .child(self.explorer_action_btn("📂")),
                    )
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
            .child(
                div()
                    .border_t_1()
                    .border_color(hex(BORDER))
                    .p(px(12.))
                    .child(self.nav_item("Terminal", Page::Terminal)),
            )
    }

    fn explorer_action_btn(&self, icon: &str) -> impl IntoElement {
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
        let bg = if active {
            hex(HOVER_BG)
        } else {
            hex_a(0x000000, 0.0)
        };
        let fg = if active {
            hex(TEXT_PRIMARY)
        } else {
            hex(TEXT_SECONDARY)
        };

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
    // Editor View — EditorView Entity を組み込む
    // ============================================================

    fn render_editor(&self, cx: &Context<Self>) -> impl IntoElement {
        let tab_title = self.editor_view.read(cx).tab_title();

        div()
            .flex_1()
            .flex()
            .flex_col()
            .bg(hex(BG))
            // タブヘッダー
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
                                    .child(tab_title)
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_MUTED))
                                            .child("×"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .p(px(6.))
                                    .rounded(px(4.))
                                    .text_size(px(14.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .cursor_pointer()
                                    .child("💾"),
                            ),
                    ),
            )
            // エディタ本体
            .child(self.editor_view.clone())
            // ステータスバー
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
                            .child("UTF-8")
                            .child("LF"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(16.))
                            .child("Spaces: 4"),
                    ),
            )
    }

    // ============================================================
    // Chat View
    // ============================================================

    fn render_chat_page(&self) -> impl IntoElement {
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
                            .children(self.chat_messages.iter().map(|msg| {
                                let is_user = msg.role == "user";
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
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
                                                    .child(if is_user {
                                                        "You"
                                                    } else {
                                                        "Agent"
                                                    }),
                                            ),
                                    )
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
    // Settings View
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
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(hex(TEXT_SECONDARY))
                            .child("⚙"),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(hex(TEXT_PRIMARY))
                            .child("Settings"),
                    ),
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
                            .child(self.settings_section(
                                "ローカルLLM設定",
                                "🔶",
                                vec![
                                    ("モデル形式", "GGUF / ONNX"),
                                    ("Temperature", "0.7"),
                                    ("最大トークン数", "2048"),
                                    ("コンテキスト長", "4096"),
                                ],
                            ))
                            .child(self.settings_section(
                                "ハードウェア設定",
                                "🖥",
                                vec![
                                    ("GPU アクセラレーション", "ON"),
                                    ("GPU レイヤー数", "32"),
                                    ("スレッド数", "8"),
                                    ("バッチサイズ", "512"),
                                ],
                            ))
                            .child(self.settings_section(
                                "外観",
                                "🎨",
                                vec![
                                    ("テーマ", "Dark"),
                                    ("フォントサイズ", "14px"),
                                    ("行番号を表示", "ON"),
                                ],
                            ))
                            .child(self.settings_section(
                                "APIキー管理",
                                "🔑",
                                vec![("OpenAI", "sk-••••••••"), ("Anthropic", "未設定")],
                            )),
                    ),
            )
    }

    fn settings_section(
        &self,
        title: &str,
        icon: &str,
        items: Vec<(&str, &str)>,
    ) -> impl IntoElement {
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
    // Terminal View
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
                                    .child(
                                        div()
                                            .text_color(hex(TRAFFIC_GREEN))
                                            .child("$"),
                                    )
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
        // キーバインド登録
        editor::actions::register_keybindings(cx);

        let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Open_Agents — AI Coding Assistant".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(16.), px(16.))),
                }),
                ..Default::default()
            },
            |window, cx| {
                window.set_client_inset(px(44.));

                // EditorView Entity を作成
                let editor_view = cx.new(|cx| EditorView::new(cx));

                cx.new(|_cx| AppView {
                    page: Page::Editor,
                    chat_messages: vec![ChatMsg {
                        role: "assistant".into(),
                        content: "こんにちは！Open Agents AIコーディングアシスタントです。".into(),
                    }],
                    files: vec![
                        FileEntry {
                            name: "src",
                            is_folder: true,
                        },
                        FileEntry {
                            name: "App.tsx",
                            is_folder: false,
                        },
                        FileEntry {
                            name: "index.tsx",
                            is_folder: false,
                        },
                        FileEntry {
                            name: "components",
                            is_folder: true,
                        },
                        FileEntry {
                            name: "utils.ts",
                            is_folder: false,
                        },
                        FileEntry {
                            name: "styles.css",
                            is_folder: false,
                        },
                    ],
                    editor_view,
                })
            },
        )
        .unwrap();
    });
}
