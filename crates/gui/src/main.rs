use gpui::*;

// ============================================================
// Design System Colors (Graphite Mono)
// ============================================================

const BG: u32 = 0x131313;
const SURFACE: u32 = 0x1b1b1c;
const SURFACE_HIGH: u32 = 0x2a2a2a;
const SURFACE_HIGHEST: u32 = 0x353535;
const SURFACE_LOWEST: u32 = 0x0e0e0e;
const PRIMARY: u32 = 0xadc6ff;
const TERTIARY: u32 = 0xffb595;
const OUTLINE: u32 = 0x8b90a0;
const ON_SURFACE: u32 = 0xe5e2e1;
const OK_GREEN: u32 = 0x27c93f;
const WARN_YELLOW: u32 = 0xffbd2e;
const ERR_RED: u32 = 0xff5f56;

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

struct ChatMsg {
    role: String,
    content: String,
}

struct AppView {
    chat_messages: Vec<ChatMsg>,
    code: String,
}

// ============================================================
// Render
// ============================================================

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(hex(BG))
            .text_color(hex(ON_SURFACE))
            // Sidebar
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .w(px(64.))
                    .h_full()
                    .bg(hex_a(SURFACE, 0.85))
                    .border_r_1()
                    .border_color(hex_a(0xFFFFFF, 0.06))
                    .py(px(16.))
                    // Logo
                    .child(
                        div()
                            .w(px(36.))
                            .h(px(36.))
                            .rounded(px(10.))
                            .bg(hex(PRIMARY))
                            .flex()
                            .items_center()
                            .justify_center()
                            .mb(px(24.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(hex(0x001a41))
                                    .child("OA"),
                            ),
                    )
                    // Nav icons
                    .child(self.nav_icon("E", true))
                    .child(self.nav_icon("M", false))
                    .child(self.nav_icon("K", false))
                    .child(self.nav_icon("L", false))
                    .child(div().flex_1())
                    .child(self.nav_icon("S", false)),
            )
            // Main area
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    // Topbar
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(44.))
                            .px(px(20.))
                            .border_b_1()
                            .border_color(hex_a(0xFFFFFF, 0.06))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .w(px(12.))
                                                    .h(px(12.))
                                                    .rounded_full()
                                                    .bg(hex(ERR_RED)),
                                            )
                                            .child(
                                                div()
                                                    .w(px(12.))
                                                    .h(px(12.))
                                                    .rounded_full()
                                                    .bg(hex(WARN_YELLOW)),
                                            )
                                            .child(
                                                div()
                                                    .w(px(12.))
                                                    .h(px(12.))
                                                    .rounded_full()
                                                    .bg(hex(OK_GREEN)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight::BOLD)
                                            .child("Open_Agents"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(24.))
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(
                                        div()
                                            .text_color(hex(PRIMARY))
                                            .border_b_2()
                                            .border_color(hex(PRIMARY))
                                            .pb(px(4.))
                                            .child("PROJECT"),
                                    )
                                    .child(div().text_color(hex(OUTLINE)).child("BUILD"))
                                    .child(div().text_color(hex(OUTLINE)).child("DEBUG")),
                            )
                            .child(
                                div()
                                    .px(px(16.))
                                    .py(px(6.))
                                    .rounded(px(8.))
                                    .bg(hex(PRIMARY))
                                    .text_color(hex(0x001a41))
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::BOLD)
                                    .cursor_pointer()
                                    .child("Deploy"),
                            ),
                    )
                    // Content: Editor + Chat
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .overflow_hidden()
                            // Editor
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .bg(hex(SURFACE_LOWEST))
                                    // Tabs
                                    .child(
                                        div()
                                            .flex()
                                            .h(px(36.))
                                            .bg(hex(SURFACE))
                                            .border_b_1()
                                            .border_color(hex_a(0xFFFFFF, 0.04))
                                            .child(
                                                div()
                                                    .px(px(16.))
                                                    .flex()
                                                    .items_center()
                                                    .bg(hex(SURFACE_LOWEST))
                                                    .border_t_2()
                                                    .border_color(hex(PRIMARY))
                                                    .text_size(px(12.))
                                                    .text_color(hex(PRIMARY))
                                                    .child("main.c"),
                                            )
                                            .child(
                                                div()
                                                    .px(px(16.))
                                                    .flex()
                                                    .items_center()
                                                    .text_size(px(12.))
                                                    .text_color(hex(OUTLINE))
                                                    .child("model.h"),
                                            ),
                                    )
                                    // Code
                                    .child(
                                        div()
                                            .flex_1()
                                            .p(px(20.))
                                            .text_size(px(13.))
                                            .overflow_hidden()
                                            .child(self.code.clone()),
                                    ),
                            )
                            // Chat
                            .child(self.render_chat()),
                    )
                    // Status bar
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .h(px(28.))
                            .px(px(16.))
                            .gap(px(16.))
                            .text_size(px(11.))
                            .text_color(hex(OUTLINE))
                            .border_t_1()
                            .border_color(hex_a(0xFFFFFF, 0.04))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .w(px(6.))
                                            .h(px(6.))
                                            .rounded_full()
                                            .bg(hex(OK_GREEN)),
                                    )
                                    .child("System Ready"),
                            )
                            .child(div().flex_1())
                            .child("Open_Agents v0.2.0"),
                    ),
            )
    }
}

impl AppView {
    fn nav_icon(&self, label: &str, active: bool) -> impl IntoElement {
        let bg = if active {
            hex(SURFACE_HIGH)
        } else {
            hex_a(0x000000, 0.0)
        };
        let fg = if active { hex(PRIMARY) } else { hex(OUTLINE) };

        div()
            .w(px(40.))
            .h(px(40.))
            .rounded(px(10.))
            .flex()
            .items_center()
            .justify_center()
            .my(px(4.))
            .bg(bg)
            .text_color(fg)
            .text_size(px(16.))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .child(label.to_string())
    }

    fn render_chat(&self) -> impl IntoElement {
        let mut msgs = div().flex().flex_col().gap(px(12.)).flex_1().p(px(16.));

        for msg in &self.chat_messages {
            let is_user = msg.role == "user";
            let bg = if is_user {
                hex(SURFACE_HIGH)
            } else {
                hex(SURFACE)
            };
            let role = if is_user { "You" } else { "AI" };
            let role_color = if is_user { hex(PRIMARY) } else { hex(TERTIARY) };

            msgs = msgs.child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .rounded(px(12.))
                    .bg(bg)
                    .child(
                        div()
                            .text_size(px(10.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(role_color)
                            .mb(px(6.))
                            .child(role.to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(hex(ON_SURFACE))
                            .child(msg.content.clone()),
                    ),
            );
        }

        div()
            .w(px(340.))
            .flex()
            .flex_col()
            .bg(hex(BG))
            .border_l_1()
            .border_color(hex_a(0xFFFFFF, 0.04))
            .child(
                div()
                    .px(px(20.))
                    .py(px(16.))
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(hex(OUTLINE))
                    .border_b_1()
                    .border_color(hex_a(0xFFFFFF, 0.04))
                    .child("AI ASSISTANT"),
            )
            .child(msgs)
            .child(
                div()
                    .p(px(12.))
                    .border_t_1()
                    .border_color(hex_a(0xFFFFFF, 0.04))
                    .child(
                        div()
                            .w_full()
                            .bg(hex(SURFACE_HIGHEST))
                            .rounded(px(12.))
                            .px(px(16.))
                            .py(px(10.))
                            .text_size(px(13.))
                            .text_color(hex(OUTLINE))
                            .child("Ask AI anything..."),
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
                    title: Some("Open_Agents — The Intelligent Workspace".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_window, cx| {
                cx.new(|_cx| AppView {
                    chat_messages: vec![ChatMsg {
                        role: "ai".into(),
                        content: "Open_Agents へようこそ！コードの作成・レビュー・デバッグをお手伝いします。"
                            .into(),
                    }],
                    code: concat!(
                        "// Open_Agents — AI Coding Engine\n",
                        "#include \"core/inference.h\"\n",
                        "\n",
                        "int main(int argc, char** argv) {\n",
                        "    oag_inference_t* inf = oag_inference_create(argv[1]);\n",
                        "    if (!inf) return 1;\n",
                        "\n",
                        "    run_interactive(inf);\n",
                        "    oag_inference_free(inf);\n",
                        "    return 0;\n",
                        "}\n",
                    )
                    .into(),
                })
            },
        )
        .unwrap();
    });
}
