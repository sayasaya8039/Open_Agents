//! Chatページ — Onyx風レイアウト（セッションサイドバー + メインチャット + 入力バー）
//!
//! パフォーマンス改善:
//! - メッセージの `clone()` を SharedString に事前変換
//! - 可視メッセージ数を制限（末尾 MAX_VISIBLE_MESSAGES 件）
//! - ストリーミングの notify バッチ化は呼び出し側（main.rs）で制御

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::chat_markdown::render_markdown_blocks;
use crate::chat_session::{ChatMsg, SessionStore};
use crate::{
    hex, hex_a, ACCENT_BLUE, ACCENT_ORANGE, BG, BORDER, HOVER_BG, PANEL_BG, SIDEBAR_BG, TEXT_DIM,
    TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};

/// 描画するメッセージの最大件数（パフォーマンス対策）
const MAX_VISIBLE_MESSAGES: usize = 60;

/// セッションサイドバーの幅
const SESSION_SIDEBAR_W: f32 = 200.0;

// ── Chat ページ全体 ──

pub fn render_chat_page(
    store: &SessionStore,
    chat_pending: bool,
    chat_show_thinking: bool,
    model_status: &str,
    is_local_weights: bool,
    open_session_menu_id: Option<u64>,
    renaming_session_id: Option<u64>,
    session_title_editor: Option<Entity<crate::session_title_editor::SessionTitleEditor>>,
    composer: Entity<crate::chat_composer::ChatComposer>,
    scroll_handle: &ScrollHandle,
    cx: &mut Context<crate::AppView>,
) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .min_h(px(0.))
        .min_w(px(0.))
        .overflow_hidden()
        .bg(hex(BG))
        // 左: セッションサイドバー
        .child(render_session_sidebar(
            store,
            chat_pending,
            open_session_menu_id,
            renaming_session_id,
            session_title_editor,
            cx,
        ))
        .child(div().w(px(1.)).h_full().bg(hex(BORDER)))
        // 右: メインチャット
        .child(render_chat_main(
            store,
            chat_pending,
            chat_show_thinking,
            model_status,
            is_local_weights,
            composer,
            scroll_handle,
            cx,
        ))
}

// ── セッションサイドバー ���─

fn render_session_sidebar(
    store: &SessionStore,
    chat_pending: bool,
    open_session_menu_id: Option<u64>,
    renaming_session_id: Option<u64>,
    session_title_editor: Option<Entity<crate::session_title_editor::SessionTitleEditor>>,
    cx: &mut Context<crate::AppView>,
) -> impl IntoElement {
    let groups = store.grouped_sessions();
    let active_id = store.active_id;

    div()
        .w(px(SESSION_SIDEBAR_W))
        .h_full()
        .bg(hex(SIDEBAR_BG))
        .flex()
        .flex_col()
        .overflow_hidden()
        // New Session ボタン
        .child(
            div().p(px(12.)).child(
                div()
                    .w_full()
                    .px(px(12.))
                    .py(px(8.))
                    .bg(hex(ACCENT_BLUE))
                    .rounded(px(8.))
                    .text_size(px(12.))
                    .text_color(hex(0xFFFFFF))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child("＋")
                    .child("New Session")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.chat_new_session(cx);
                        }),
                    ),
            ),
        )
        // セッション一覧（日付グループ）
        .child(
            div()
                .id("chat-session-list")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .px(px(8.))
                .pb(px(12.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(groups.into_iter().map(|(label, sessions)| {
                    let label_for_delete = label;
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .px(px(8.))
                                .py(px(6.))
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(hex(TEXT_DIM))
                                        .child(label.to_string()),
                                )
                                .when(!chat_pending, |d| {
                                    d.child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_MUTED))
                                            .cursor_pointer()
                                            .hover(|d| d.text_color(hex(TEXT_PRIMARY)))
                                            .child("削除")
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |this, _: &MouseDownEvent, _, cx| {
                                                        this.chat_delete_section(
                                                            label_for_delete,
                                                            cx,
                                                        );
                                                    },
                                                ),
                                            ),
                                    )
                                }),
                        )
                        .children(sessions.into_iter().map(|session| {
                            let id = session.id;
                            let is_active = active_id == Some(id);
                            let is_menu_open = open_session_menu_id == Some(id);
                            let is_renaming = renaming_session_id == Some(id);
                            let title: SharedString = session.title.clone().into();
                            let editor = session_title_editor.clone();
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .px(px(8.))
                                        .py(px(6.))
                                        .rounded(px(6.))
                                        .text_size(px(12.))
                                        .text_color(if is_active {
                                            hex(TEXT_PRIMARY)
                                        } else {
                                            hex(TEXT_SECONDARY)
                                        })
                                        .when(is_active, |d| d.bg(hex(HOVER_BG)))
                                        .hover(|d| d.bg(hex(HOVER_BG)))
                                        .overflow_hidden()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .when(is_renaming, |d| {
                                                    d.when_some(editor.clone(), |d, editor| {
                                                        d.child(editor)
                                                    })
                                                })
                                                .when(!is_renaming, |d| {
                                                    d.child(
                                                        div()
                                                            .w_full()
                                                            .min_w(px(0.))
                                                            .overflow_hidden()
                                                            .whitespace_nowrap()
                                                            .cursor_pointer()
                                                            .child(title)
                                                            .on_mouse_down(
                                                                MouseButton::Left,
                                                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                                                    this.chat_switch_session(id, cx);
                                                                }),
                                                            ),
                                                    )
                                                }),
                                        )
                                        .when(is_renaming, |d| {
                                            d.child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(4.))
                                                    .child(
                                                        div()
                                                            .px(px(8.))
                                                            .py(px(4.))
                                                            .rounded(px(6.))
                                                            .bg(hex(ACCENT_BLUE))
                                                            .text_size(px(11.))
                                                            .text_color(hex(0xFFFFFF))
                                                            .cursor_pointer()
                                                            .child("保存")
                                                            .on_mouse_down(
                                                                MouseButton::Left,
                                                                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                                                    window.prevent_default();
                                                                    cx.stop_propagation();
                                                                    this.chat_commit_session_rename(cx);
                                                                }),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .px(px(8.))
                                                            .py(px(4.))
                                                            .rounded(px(6.))
                                                            .bg(hex(PANEL_BG))
                                                            .border_1()
                                                            .border_color(hex(BORDER))
                                                            .text_size(px(11.))
                                                            .text_color(hex(TEXT_MUTED))
                                                            .cursor_pointer()
                                                            .hover(|d| d.bg(hex(HOVER_BG)))
                                                            .child("取消")
                                                            .on_mouse_down(
                                                                MouseButton::Left,
                                                                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                                                    window.prevent_default();
                                                                    cx.stop_propagation();
                                                                    this.chat_cancel_session_rename(cx);
                                                                }),
                                                            ),
                                                    ),
                                            )
                                        })
                                        .when(!is_renaming && !chat_pending, |d| {
                                            d.child(
                                                div()
                                                    .w(px(22.))
                                                    .h(px(22.))
                                                    .rounded(px(6.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(14.))
                                                    .text_color(hex(TEXT_MUTED))
                                                    .cursor_pointer()
                                                    .hover(|d| d.bg(hex(PANEL_BG)).text_color(hex(TEXT_PRIMARY)))
                                                    .child("⋯")
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                                            window.prevent_default();
                                                            cx.stop_propagation();
                                                            this.chat_toggle_session_menu(id, cx);
                                                        }),
                                                    ),
                                            )
                                        }),
                                )
                                .when(is_menu_open && !chat_pending && !is_renaming, |d| {
                                    d.child(
                                        div()
                                            .ml(px(16.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(hex(BORDER))
                                            .bg(hex(PANEL_BG))
                                            .overflow_hidden()
                                            .child(
                                                session_menu_item("名前変更", false)
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                                            window.prevent_default();
                                                            cx.stop_propagation();
                                                            this.chat_begin_session_rename(id, window, cx);
                                                        }),
                                                    ),
                                            )
                                            .child(
                                                session_menu_item("複製", false)
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                                            window.prevent_default();
                                                            cx.stop_propagation();
                                                            this.chat_duplicate_session(id, cx);
                                                        }),
                                                    ),
                                            )
                                            .child(
                                                session_menu_item("エクスポート", false)
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                                            window.prevent_default();
                                                            cx.stop_propagation();
                                                            this.chat_export_session(id, cx);
                                                        }),
                                                    ),
                                            )
                                            .child(
                                                session_menu_item("エクスプローラーで表示", false)
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                                            window.prevent_default();
                                                            cx.stop_propagation();
                                                            this.chat_show_session_file_in_explorer();
                                                        }),
                                                    ),
                                            )
                                            .child(
                                                session_menu_item("削除", true)
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                                            window.prevent_default();
                                                            cx.stop_propagation();
                                                            this.chat_delete_session(id, cx);
                                                        }),
                                                    ),
                                            ),
                                    )
                                })
                                .into_any_element()
                        }))
                        .into_any_element()
                })),
        )
        // Settings ボタン（下部固定）
        .child(
            div()
                .flex_shrink_0()
                .border_t_1()
                .border_color(hex(BORDER))
                .p(px(8.))
                .child(
                    div()
                        .px(px(8.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .text_size(px(12.))
                        .text_color(hex(TEXT_SECONDARY))
                        .cursor_pointer()
                        .hover(|d| d.bg(hex(HOVER_BG)))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child("⚙")
                        .child("Settings")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.page = crate::Page::Settings;
                                cx.notify();
                            }),
                        ),
                ),
        )
}

fn session_menu_item(label: &'static str, destructive: bool) -> Div {
    div()
        .w_full()
        .px(px(10.))
        .py(px(8.))
        .text_size(px(11.))
        .text_color(if destructive {
            hex(ACCENT_ORANGE)
        } else {
            hex(TEXT_PRIMARY)
        })
        .cursor_pointer()
        .hover(|d| d.bg(hex(HOVER_BG)))
        .child(label)
}

// ── メインチャットエリア ──

fn render_chat_main(
    store: &SessionStore,
    chat_pending: bool,
    chat_show_thinking: bool,
    model_status: &str,
    is_local_weights: bool,
    composer: Entity<crate::chat_composer::ChatComposer>,
    scroll_handle: &ScrollHandle,
    cx: &mut Context<crate::AppView>,
) -> impl IntoElement {
    let messages: &[ChatMsg] = store.active().map(|s| s.messages.as_slice()).unwrap_or(&[]);
    let show_suggestions = messages.len() <= 1;

    // パフォーマンス: 末尾 MAX_VISIBLE_MESSAGES 件のみ描画
    let total = messages.len();
    let skip = total.saturating_sub(MAX_VISIBLE_MESSAGES);
    let visible = &messages[skip..];
    let has_older = skip > 0;

    div()
        .flex_1()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .min_w(px(0.))
        .overflow_hidden()
        // ── ミニヘッダー ──
        .child(
            div()
                .flex_shrink_0()
                .px(px(24.))
                .pt(px(18.))
                .pb(px(8.))
                .flex()
                .justify_end()
                .child(
                    div()
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(px(999.))
                        .bg(hex(PANEL_BG))
                        .border_1()
                        .border_color(hex(BORDER))
                        .text_size(px(10.))
                        .text_color(hex(TEXT_MUTED))
                        .cursor_pointer()
                        .hover(|d| d.bg(hex(HOVER_BG)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.chat_show_thinking = !this.chat_show_thinking;
                                this.chat_scroll.scroll_to_bottom();
                                cx.notify();
                            }),
                        )
                        .child(if chat_show_thinking {
                            "思考: 表示"
                        } else {
                            "思考: 非表示"
                        }),
                ),
        )
        // ── メッセージエリア ──
        .child(
            div()
                .id("chat-messages-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .track_scroll(scroll_handle)
                .child(
                    div()
                        .max_w(px(860.))
                        .mx_auto()
                        .px(px(32.))
                        .pt(px(8.))
                        .pb(px(180.))
                        .flex()
                        .flex_col()
                        // 提案チップ
                        .when(show_suggestions, |d| {
                            d.child(
                                div()
                                    .mb(px(24.))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_DIM))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .mb(px(10.))
                                            .child("提案"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap(px(6.))
                                                    .child(suggestion_chip(
                                                        "Reactコンポーネントを作成",
                                                    ))
                                                    .child(suggestion_chip("バグを修正")),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap(px(6.))
                                                    .child(suggestion_chip(
                                                        "コードをリファクタリング",
                                                    ))
                                                    .child(suggestion_chip("テストを追加")),
                                            ),
                                    ),
                            )
                        })
                        // 「過去のメッセージを読み込む」
                        .when(has_older, |d| {
                            d.child(
                                div().mb(px(16.)).py(px(8.)).flex().justify_center().child(
                                    div()
                                        .px(px(12.))
                                        .py(px(4.))
                                        .rounded(px(6.))
                                        .bg(hex(PANEL_BG))
                                        .border_1()
                                        .border_color(hex(BORDER))
                                        .text_size(px(11.))
                                        .text_color(hex(TEXT_MUTED))
                                        .child(format!("↑ 過去 {skip} 件のメッセージ")),
                                ),
                            )
                        })
                        // メッセージ一覧
                        .child(div().flex().flex_col().gap(px(24.)).children(
                            visible.iter().enumerate().map(|(index, msg)| {
                                render_message(
                                    msg,
                                    skip + index,
                                    total,
                                    chat_show_thinking,
                                    chat_pending,
                                    cx,
                                )
                            }),
                        ))
                        // 入力バーに隠れないためのスペーサー
                        .child(div().h(px(160.)).flex_shrink_0()),
                ),
        )
        // ── 入力バー ──
        .child(render_input_bar(
            chat_pending,
            model_status,
            is_local_weights,
            composer,
            cx,
        ))
}

// ── メッセージバブル ──

fn render_message(
    msg: &ChatMsg,
    message_index: usize,
    total_messages: usize,
    show_thinking: bool,
    chat_pending: bool,
    cx: &mut Context<crate::AppView>,
) -> impl IntoElement {
    let is_user = msg.role == "user";
    let is_last_message = message_index + 1 == total_messages;
    let model_label: Option<SharedString> = msg
        .metrics
        .as_ref()
        .and_then(|metrics| metrics.model_label.clone())
        .map(Into::into);
    let metric_labels = msg.metrics.as_ref().map(metric_labels).unwrap_or_default();

    let mut block = div()
        .w_full()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .when(!is_user, |d| {
            d.when_some(model_label.clone(), |d, model_label| {
                d.child(
                    div()
                        .ml(px(30.))
                        .text_size(px(11.))
                        .text_color(hex(TEXT_MUTED))
                        .child(model_label),
                )
            })
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(if is_user {
                    div()
                        .w(px(22.))
                        .h(px(22.))
                        .rounded(px(6.))
                        .bg(hex(ACCENT_BLUE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.))
                        .text_color(hex(0xFFFFFF))
                        .child("U")
                        .into_any_element()
                } else {
                    div()
                        .w(px(22.))
                        .h(px(22.))
                        .rounded(px(6.))
                        .bg(hex(ACCENT_ORANGE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.))
                        .text_color(hex(0xFFFFFF))
                        .child("✦")
                        .into_any_element()
                })
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(hex(TEXT_DIM))
                        .child(if is_user { "You" } else { "Agent" }),
                ),
        );

    // Thinking（折りた��み）
    if let Some(th) = &msg.thinking {
        if show_thinking {
            let thinking_text: SharedString = th.clone().into();
            block = block.child(
                div()
                    .ml(px(30.))
                    .w_full()
                    .min_w(px(0.))
                    .flex()
                    .child(
                        div().w(px(2.)).flex_shrink_0().bg(hex(0xa855f7)), // PURPLE
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .p(px(10.))
                            .bg(hex(PANEL_BG))
                            .rounded_r(px(4.))
                            .text_size(px(12.))
                            .text_color(hex(TEXT_SECONDARY))
                            .whitespace_normal()
                            .child(thinking_text),
                    ),
            );
        }
    }

    block = block.child(
        div()
            .ml(px(30.))
            .w_full()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .children(render_markdown_blocks(&msg.content)),
    );

    if !is_user && !metric_labels.is_empty() {
        block = block.child(
            div()
                .ml(px(30.))
                .mt(px(2.))
                .flex()
                .items_center()
                .gap(px(8.))
                .flex_wrap()
                .children(metric_labels.into_iter().map(render_metric_chip)),
        );
    }

    if !is_user {
        let can_regenerate = is_last_message && !chat_pending;
        let copy_text = msg.content.clone();
        block = block.child(
            div()
                .ml(px(30.))
                .mt(px(2.))
                .flex()
                .items_center()
                .gap(px(10.))
                .child(render_action_button(
                    "⧉",
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.chat_copy_message(copy_text.clone(), cx);
                    }),
                ))
                .child(render_action_button(
                    "🗑",
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.chat_delete_message(message_index, cx);
                    }),
                ))
                .when(can_regenerate, |d| {
                    d.child(render_action_button(
                        "↻",
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.chat_regenerate_last_reply(cx);
                        }),
                    ))
                }),
        );
    }

    block
}

// ── 入力バー ──

fn render_input_bar(
    send_disabled: bool,
    model_status: &str,
    is_local_weights: bool,
    composer: Entity<crate::chat_composer::ChatComposer>,
    cx: &mut Context<crate::AppView>,
) -> impl IntoElement {
    // モデル名を短縮表示
    let short_model: SharedString = shorten_model_name(model_status).into();

    div()
        .flex_shrink_0()
        .border_t_1()
        .border_color(hex(BORDER))
        .bg(hex(PANEL_BG))
        .p(px(12.))
        .child(
            div()
                .max_w(px(720.))
                .mx_auto()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .w_full()
                        .min_h(px(52.))
                        .flex()
                        .gap(px(8.))
                        .items_end()
                        // テキス���入力
                        .child(
                            div()
                                .flex_1()
                                .min_h(px(52.))
                                .min_w(px(0.))
                                .bg(hex(BG))
                                .border_1()
                                .border_color(hex(BORDER))
                                .rounded(px(12.))
                                .px(px(12.))
                                .py(px(8.))
                                .flex()
                                .items_center()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, window, cx| {
                                        this.chat_composer.read(cx).focus(window);
                                    }),
                                )
                                .child(composer),
                        )
                        // 送信ボタン
                        .child(
                            div()
                                .flex_shrink_0()
                                .mb(px(4.))
                                .p(px(8.))
                                .rounded(px(8.))
                                .bg(if send_disabled {
                                    hex_a(ACCENT_BLUE, 0.45)
                                } else {
                                    hex(ACCENT_BLUE)
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(12.))
                                .text_color(hex(0xFFFFFF))
                                .cursor(if send_disabled {
                                    CursorStyle::OperationNotAllowed
                                } else {
                                    CursorStyle::PointingHand
                                })
                                .when(!send_disabled, |d| {
                                    d.on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.on_chat_submitted(cx);
                                        }),
                                    )
                                })
                                .child("➤"),
                        ),
                )
                // モデル選択 + キーヒント
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                // 推論先切替ボタン（クリックでサイクル: API → Ollama → ローカルGGUF）
                                .child(
                                    div()
                                        .px(px(8.))
                                        .py(px(3.))
                                        .bg(hex_a(ACCENT_BLUE, 0.15))
                                        .rounded(px(4.))
                                        .text_size(px(10.))
                                        .text_color(hex(ACCENT_BLUE))
                                        .cursor_pointer()
                                        .hover(|d| d.bg(hex_a(ACCENT_BLUE, 0.3)))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                                this.chat_prefs.source =
                                                    this.chat_prefs.source.cycle();
                                                this.save_chat_prefs();
                                                cx.notify();
                                            }),
                                        )
                                        .child(short_model),
                                )
                                // ローカルモデル切替（LocalWeights時のみ、◀ ▶ でインデックス切替）
                                .when(is_local_weights, |d| {
                                    d.child(
                                        div()
                                            .px(px(6.))
                                            .py(px(2.))
                                            .rounded(px(4.))
                                            .bg(hex(BORDER))
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .cursor_pointer()
                                            .hover(|d| d.bg(hex(HOVER_BG)))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                                    this.cycle_local_model(cx);
                                                }),
                                            )
                                            .child("▶ 次のモデル"),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_size(px(10.))
                                .text_color(hex(TEXT_MUTED))
                                .child(
                                    div()
                                        .px(px(5.))
                                        .py(px(1.))
                                        .bg(hex(BORDER))
                                        .rounded(px(3.))
                                        .child("Enter"),
                                )
                                .child("送信")
                                .child(
                                    div()
                                        .px(px(5.))
                                        .py(px(1.))
                                        .bg(hex(BORDER))
                                        .rounded(px(3.))
                                        .child("Shift+Enter"),
                                )
                                .child("改行"),
                        ),
                ),
        )
}

// ── 提案チップ ──

fn suggestion_chip(label: &str) -> impl IntoElement {
    div()
        .flex_1()
        .min_w(px(0.))
        .px(px(14.))
        .py(px(10.))
        .bg(hex(PANEL_BG))
        .border_1()
        .border_color(hex(BORDER))
        .rounded(px(8.))
        .text_size(px(12.))
        .text_color(hex(TEXT_SECONDARY))
        .cursor_pointer()
        .hover(|d| d.bg(hex(HOVER_BG)))
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .text_size(px(13.))
                .text_color(hex(ACCENT_BLUE))
                .child("⚡"),
        )
        .child(label.to_string())
}

// ── ヘルパー ──

fn metric_labels(metrics: &crate::chat_session::ChatMsgMetrics) -> Vec<String> {
    let mut labels = Vec::new();
    let token_count = metrics.completion_tokens.or(metrics.total_tokens);
    if let Some(token_count) = token_count {
        labels.push(format!("トークン数: {token_count} トークン"));
    }
    if let Some(tokens_per_second) =
        metrics
            .tokens_per_second
            .or_else(|| match (token_count, metrics.elapsed_ms) {
                (Some(tokens), Some(elapsed_ms)) if elapsed_ms > 0 => {
                    Some(tokens as f64 / (elapsed_ms as f64 / 1000.0))
                }
                _ => None,
            })
    {
        labels.push(format!(
            "トークン毎秒: {:.2} トークン/秒",
            tokens_per_second
        ));
    }
    if let Some(elapsed_ms) = metrics.elapsed_ms {
        labels.push(format!("応答時間: {}", format_elapsed_ms(elapsed_ms)));
    }
    if let Some(stop_reason) = &metrics.stop_reason {
        labels.push(format!("停止理由: {stop_reason}"));
    }
    labels
}

fn render_metric_chip(label: String) -> impl IntoElement {
    div()
        .px(px(10.))
        .py(px(4.))
        .bg(hex(PANEL_BG))
        .border_1()
        .border_color(hex(BORDER))
        .rounded(px(999.))
        .text_size(px(10.))
        .text_color(hex(TEXT_SECONDARY))
        .child(label)
}

fn render_action_button(
    icon: &'static str,
    listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .w(px(24.))
        .h(px(24.))
        .rounded(px(999.))
        .text_size(px(13.))
        .text_color(hex(TEXT_MUTED))
        .cursor_pointer()
        .hover(|d| d.bg(hex(HOVER_BG)).text_color(hex(TEXT_PRIMARY)))
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(MouseButton::Left, listener)
        .child(icon)
}

fn format_elapsed_ms(elapsed_ms: u64) -> String {
    format!("{:.2}s", elapsed_ms as f64 / 1000.0)
}

fn shorten_model_name(status: &str) -> String {
    // "Chat: クラウド API（gpt-4o-mini）" → "gpt-4o-mini"
    // "Chat: GGUF/ONNX [1/2] model.gguf" → "model.gguf"
    if let Some(start) = status.find('（') {
        if let Some(end) = status.find('）') {
            return status[start + '（'.len_utf8()..end].to_string();
        }
    }
    if let Some(pos) = status.rfind(']') {
        let rest = status[pos + 1..].trim();
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    if let Some(pos) = status.find(':') {
        return status[pos + 1..].trim().to_string();
    }
    status.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_api_model() {
        let s = "Chat: クラウド API（gpt-4o-mini）";
        assert_eq!(shorten_model_name(s), "gpt-4o-mini");
    }

    #[test]
    fn shorten_gguf_model() {
        let s = "Chat: GGUF/ONNX [1/2] gemma-4-12b-Q4.gguf";
        assert_eq!(shorten_model_name(s), "gemma-4-12b-Q4.gguf");
    }

    #[test]
    fn shorten_ollama_model() {
        let s = "Chat: Ollama（llama3.2）";
        assert_eq!(shorten_model_name(s), "llama3.2");
    }

    #[test]
    fn metric_labels_render_expected_order() {
        let labels = metric_labels(&crate::chat_session::ChatMsgMetrics {
            completion_tokens: Some(66),
            tokens_per_second: Some(77.24),
            elapsed_ms: Some(430),
            stop_reason: Some("EOSトークン検出".into()),
            ..Default::default()
        });
        assert_eq!(labels[0], "トークン数: 66 トークン");
        assert_eq!(labels[1], "トークン毎秒: 77.24 トークン/秒");
        assert_eq!(labels[2], "応答時間: 0.43s");
        assert_eq!(labels[3], "停止理由: EOSトークン検出");
    }

    #[test]
    fn metric_labels_derive_tokens_per_second_from_elapsed_time() {
        let labels = metric_labels(&crate::chat_session::ChatMsgMetrics {
            completion_tokens: Some(40),
            elapsed_ms: Some(500),
            ..Default::default()
        });
        assert_eq!(labels[0], "トークン数: 40 トークン");
        assert_eq!(labels[1], "トークン毎秒: 80.00 トークン/秒");
        assert_eq!(labels[2], "応答時間: 0.50s");
    }
}
