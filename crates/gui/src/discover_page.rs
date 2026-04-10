//! Hugging Face モデル Discover ページ UI
//!
//! 左: 検索バー + 結果カードリスト
//! 右: 選択されたモデルの詳細（ヘッダー + Download Options + README）

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::hf_discover::{
    format_count, human_size, CapabilitySet, DownloadStatus, GgufFile, HfModelDetail,
    HfModelSummary, HuggingFaceSearchState,
};
use crate::{
    hex, hex_a, ACCENT_BLUE, ACCENT_ORANGE, BG, BORDER, CONTROL_BG, CONTROL_BORDER, HOVER_BG,
    PANEL_BG, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};

pub fn render_discover_page(
    state: &HuggingFaceSearchState,
    search_composer: Entity<crate::chat_composer::ChatComposer>,
    downloads: &crate::hf_discover::DownloadManager,
    cx: &mut Context<crate::AppView>,
) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .min_w(px(0.))
        .bg(hex(BG))
        .child(render_discover_header(state, downloads, cx))
        .child(
            div()
                .flex_1()
                .flex()
                .min_h(px(0.))
                .min_w(px(0.))
                // 左: 検索バー + 結果リスト
                .child(render_results_panel(state, search_composer, cx))
                .child(div().w(px(1.)).h_full().bg(hex(BORDER)))
                // 右: 詳細 + ダウンロードドロワー
                .child(render_detail_panel(state, downloads, cx)),
        )
}

fn render_discover_header(
    state: &HuggingFaceSearchState,
    downloads: &crate::hf_discover::DownloadManager,
    cx: &mut Context<crate::AppView>,
) -> impl IntoElement {
    let active = downloads
        .tasks
        .iter()
        .filter(|t| t.status == DownloadStatus::InProgress || t.status == DownloadStatus::Queued)
        .count();
    let sort_label: String = format!("Sort: {}", state.sort.label());
    div()
        .flex_shrink_0()
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
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .text_size(px(12.))
                        .text_color(hex(TEXT_SECONDARY))
                        .cursor_pointer()
                        .hover(|d| d.bg(hex(HOVER_BG)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.page = crate::Page::Chat;
                                cx.notify();
                            }),
                        )
                        .child("← Chat"),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .text_color(hex(TEXT_SECONDARY))
                        .child("🤗"),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(hex(TEXT_PRIMARY))
                        .child(crate::i18n::discover_title()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                // Sort ボタン
                .child(
                    div()
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .bg(hex(CONTROL_BG))
                        .border_1()
                        .border_color(hex(CONTROL_BORDER))
                        .text_size(px(11.))
                        .text_color(hex(TEXT_PRIMARY))
                        .cursor_pointer()
                        .hover(|d| d.bg(hex(HOVER_BG)))
                        .child(sort_label)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.hf_cycle_sort(cx);
                            }),
                        ),
                )
                // Downloads ドロワーボタン
                .child(
                    div()
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .bg(if active > 0 {
                            hex_a(ACCENT_BLUE, 0.2)
                        } else {
                            hex(CONTROL_BG)
                        })
                        .border_1()
                        .border_color(hex(CONTROL_BORDER))
                        .text_size(px(11.))
                        .text_color(hex(TEXT_PRIMARY))
                        .cursor_pointer()
                        .hover(|d| d.bg(hex(HOVER_BG)))
                        .child(format!("⬇ Downloads ({active})"))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.hf_toggle_downloads_panel(cx);
                            }),
                        ),
                ),
        )
}

fn render_results_panel(
    state: &HuggingFaceSearchState,
    search_composer: Entity<crate::chat_composer::ChatComposer>,
    cx: &mut Context<crate::AppView>,
) -> impl IntoElement {
    div()
        .w(px(440.))
        .h_full()
        .flex()
        .flex_col()
        .bg(hex(BG))
        .child(
            div()
                .p(px(12.))
                .flex_shrink_0()
                .border_b_1()
                .border_color(hex(BORDER))
                .flex()
                .gap(px(8.))
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .px(px(10.))
                        .py(px(8.))
                        .rounded(px(6.))
                        .bg(hex(CONTROL_BG))
                        .border_1()
                        .border_color(hex(CONTROL_BORDER))
                        .child(search_composer),
                )
                .child({
                    let loading = state.loading;
                    let mut btn = div()
                        .px(px(14.))
                        .py(px(8.))
                        .rounded(px(6.))
                        .text_size(px(12.))
                        .text_color(hex(0xFFFFFF))
                        .child(crate::i18n::discover_search_button());
                    if loading {
                        btn = btn.bg(hex_a(ACCENT_BLUE, 0.4));
                    } else {
                        btn = btn
                            .bg(hex(ACCENT_BLUE))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    this.hf_execute_search(cx);
                                }),
                            );
                    }
                    btn
                }),
        )
        .child(
            div()
                .id("hf-results-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .when(state.loading, |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(hex(TEXT_MUTED))
                            .child(crate::i18n::discover_loading().to_string()),
                    )
                })
                .when_some(state.error.clone(), |d, err| {
                    d.child(
                        div()
                            .p(px(10.))
                            .rounded(px(6.))
                            .bg(hex_a(ACCENT_ORANGE, 0.1))
                            .border_1()
                            .border_color(hex(ACCENT_ORANGE))
                            .text_size(px(11.))
                            .text_color(hex(ACCENT_ORANGE))
                            .child(err),
                    )
                })
                .when(!state.loading && state.results.is_empty() && state.error.is_none(), |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(hex(TEXT_MUTED))
                            .child(crate::i18n::discover_empty().to_string()),
                    )
                })
                .children(state.results.iter().enumerate().map(|(idx, model)| {
                    render_result_card(idx, model, state.selected_id.as_deref() == Some(&model.id), cx)
                })),
        )
}

fn render_result_card(
    idx: usize,
    model: &HfModelSummary,
    selected: bool,
    cx: &mut Context<crate::AppView>,
) -> AnyElement {
    let id_owned = model.id.clone();
    let title: SharedString = model.name.clone().into();
    let author: SharedString = model.author.clone().into();
    let updated_short = if model.updated_at.len() >= 10 {
        &model.updated_at[..10]
    } else {
        model.updated_at.as_str()
    };
    let stats = if updated_short.is_empty() {
        format!(
            "⬇ {}  ♥ {}",
            format_count(model.downloads),
            format_count(model.likes)
        )
    } else {
        format!(
            "⬇ {}  ♥ {}  · {}",
            format_count(model.downloads),
            format_count(model.likes),
            updated_short
        )
    };

    div()
        .id(("hf-card", idx))
        .p(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(if selected {
            hex(ACCENT_BLUE)
        } else {
            hex(BORDER)
        })
        .bg(if selected {
            hex_a(ACCENT_BLUE, 0.08)
        } else {
            hex(PANEL_BG)
        })
        .cursor_pointer()
        .hover(|d| d.bg(hex(HOVER_BG)))
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hex(TEXT_PRIMARY))
                .overflow_hidden()
                .whitespace_nowrap()
                .child(title),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(hex(TEXT_SECONDARY))
                .child(author),
        )
        .child(render_capability_badges(model.caps))
        .child(
            div()
                .text_size(px(10.))
                .text_color(hex(TEXT_MUTED))
                .child(stats),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.hf_select_model(id_owned.clone(), cx);
            }),
        )
        .into_any_element()
}

fn render_capability_badges(caps: CapabilitySet) -> Div {
    let mut row = div().flex().gap(px(4.));
    if caps.vision {
        row = row.child(badge("Vision", 0x3b82f6));
    }
    if caps.tool_use {
        row = row.child(badge("Tools", 0x22c55e));
    }
    if caps.reasoning {
        row = row.child(badge("Reasoning", 0xa855f7));
    }
    row
}

fn badge(label: &'static str, color: u32) -> Div {
    div()
        .px(px(6.))
        .py(px(2.))
        .rounded(px(4.))
        .bg(hex_a(color, 0.18))
        .text_size(px(9.))
        .text_color(hex(color))
        .child(label)
}

fn render_detail_panel(
    state: &HuggingFaceSearchState,
    downloads: &crate::hf_discover::DownloadManager,
    cx: &mut Context<crate::AppView>,
) -> impl IntoElement {
    div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .bg(hex(BG))
        .when(downloads.panel_open, |d| {
            d.child(render_downloads_drawer(downloads, cx))
        })
        .child(
            div()
                .id("hf-detail-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .p(px(20.))
                .when(state.detail_loading, |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(hex(TEXT_MUTED))
                            .child(crate::i18n::discover_loading().to_string()),
                    )
                })
                .when_some(state.detail_error.clone(), |d, err| {
                    d.child(
                        div()
                            .p(px(10.))
                            .rounded(px(6.))
                            .bg(hex_a(ACCENT_ORANGE, 0.1))
                            .text_size(px(11.))
                            .text_color(hex(ACCENT_ORANGE))
                            .child(err),
                    )
                })
                .when_some(state.detail.clone(), |d, detail| {
                    d.child(render_detail_body(detail, cx))
                })
                .when(state.detail.is_none() && !state.detail_loading && state.detail_error.is_none(), |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(hex(TEXT_MUTED))
                            .child(crate::i18n::discover_pick_model().to_string()),
                    )
                }),
        )
}

fn render_detail_body(detail: HfModelDetail, cx: &mut Context<crate::AppView>) -> Div {
    let id_title: SharedString = detail.id.clone().into();
    let stats = format!(
        "⬇ {} downloads  ♥ {} likes",
        format_count(detail.downloads),
        format_count(detail.likes)
    );
    let tags_line: Option<SharedString> = if detail.tags.is_empty() {
        None
    } else {
        let joined = detail
            .tags
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ");
        Some(joined.into())
    };
    let readme_text: SharedString = if detail.readme.is_empty() {
        "(README なし)".to_string().into()
    } else {
        detail.readme.clone().into()
    };
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(
            div()
                .text_size(px(18.))
                .font_weight(FontWeight::BOLD)
                .text_color(hex(TEXT_PRIMARY))
                .child(id_title),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(hex(TEXT_MUTED))
                .child(stats),
        )
        .child(render_capability_badges(detail.caps))
        .when_some(tags_line, |d, tags| {
            d.child(
                div()
                    .text_size(px(10.))
                    .text_color(hex(TEXT_MUTED))
                    .whitespace_normal()
                    .child(tags),
            )
        })
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hex(TEXT_PRIMARY))
                .child(crate::i18n::discover_download_options().to_string()),
        )
        .child(render_download_options(&detail.id, &detail.files, cx))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hex(TEXT_PRIMARY))
                .mt(px(12.))
                .child("README"),
        )
        .child(
            div()
                .p(px(12.))
                .rounded(px(6.))
                .bg(hex(PANEL_BG))
                .border_1()
                .border_color(hex(BORDER))
                .text_size(px(11.))
                .text_color(hex(TEXT_SECONDARY))
                .whitespace_normal()
                .child(readme_text),
        )
}

fn render_download_options(
    model_id: &str,
    files: &[GgufFile],
    cx: &mut Context<crate::AppView>,
) -> Div {
    if files.is_empty() {
        return div()
            .p(px(10.))
            .text_size(px(11.))
            .text_color(hex(TEXT_MUTED))
            .child("GGUF ファイルが見つかりません".to_string());
    }
    let model_id_owned = model_id.to_string();
    let mut col = div().flex().flex_col().gap(px(6.));
    for (idx, file) in files.iter().enumerate() {
        let filename: SharedString = file.filename.clone().into();
        let quant = file
            .quant
            .clone()
            .unwrap_or_else(|| "?".to_string());
        let size_label = human_size(file.size_bytes);
        let fit = file.fit_hint.label();
        let model_id_c = model_id_owned.clone();
        let file_c = file.clone();
        col = col.child(
            div()
                .id(("hf-download-row", idx))
                .p(px(10.))
                .rounded(px(6.))
                .bg(hex(PANEL_BG))
                .border_1()
                .border_color(hex(BORDER))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(hex(TEXT_PRIMARY))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(filename),
                        )
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(hex(TEXT_MUTED))
                                .child(format!("{quant} · {size_label} · {fit}")),
                        ),
                )
                .child(
                    div()
                        .px(px(12.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .bg(hex(ACCENT_BLUE))
                        .text_size(px(11.))
                        .text_color(hex(0xFFFFFF))
                        .cursor_pointer()
                        .child(crate::i18n::discover_download_button().to_string())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                this.hf_start_download(model_id_c.clone(), file_c.clone(), cx);
                            }),
                        ),
                ),
        );
    }
    col
}

fn render_downloads_drawer(
    downloads: &crate::hf_discover::DownloadManager,
    cx: &mut Context<crate::AppView>,
) -> Div {
    let mut col = div()
        .flex_shrink_0()
        .p(px(12.))
        .bg(hex(PANEL_BG))
        .border_b_1()
        .border_color(hex(BORDER))
        .flex()
        .flex_col()
        .gap(px(6.));
    let has_completed = downloads.tasks.iter().any(|t| {
        matches!(
            t.status,
            DownloadStatus::Completed | DownloadStatus::Failed | DownloadStatus::Cancelled
        )
    });
    col = col.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(hex(TEXT_PRIMARY))
                    .child(crate::i18n::discover_downloads().to_string()),
            )
            .when(has_completed, |d| {
                d.child(
                    div()
                        .px(px(8.))
                        .py(px(3.))
                        .rounded(px(4.))
                        .bg(hex(CONTROL_BG))
                        .border_1()
                        .border_color(hex(CONTROL_BORDER))
                        .text_size(px(10.))
                        .text_color(hex(TEXT_SECONDARY))
                        .cursor_pointer()
                        .hover(|d| d.bg(hex(HOVER_BG)))
                        .child("完了をクリア")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.hf_downloads.clear_completed();
                                cx.notify();
                            }),
                        ),
                )
            }),
    );
    if downloads.tasks.is_empty() {
        col = col.child(
            div()
                .text_size(px(11.))
                .text_color(hex(TEXT_MUTED))
                .child(crate::i18n::discover_no_downloads().to_string()),
        );
        return col;
    }
    for (idx, task) in downloads.tasks.iter().enumerate() {
        let id = task.id;
        let progress_pct = if task.total_bytes > 0 {
            (task.downloaded_bytes as f64 / task.total_bytes as f64 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let filename: SharedString = task.filename.clone().into();
        let model_id_label: SharedString = task.model_id.clone().into();
        let status_label: String = match task.status {
            DownloadStatus::Queued => "Queued".to_string(),
            DownloadStatus::InProgress => format!(
                "{:.0}% · {}/{}",
                progress_pct,
                human_size(task.downloaded_bytes),
                human_size(task.total_bytes)
            ),
            DownloadStatus::Completed => "Completed".to_string(),
            DownloadStatus::Failed => task.error.clone().unwrap_or_else(|| "Failed".to_string()),
            DownloadStatus::Cancelled => "Cancelled".to_string(),
        };
        col = col.child(
            div()
                .id(("hf-dl-row", idx))
                .p(px(8.))
                .rounded(px(6.))
                .bg(hex(CONTROL_BG))
                .border_1()
                .border_color(hex(CONTROL_BORDER))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(1.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(hex(TEXT_PRIMARY))
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .child(filename),
                                )
                                .child(
                                    div()
                                        .text_size(px(9.))
                                        .text_color(hex(TEXT_MUTED))
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .child(model_id_label),
                                ),
                        )
                        .when(task.status == DownloadStatus::InProgress, |d| {
                            d.child(
                                div()
                                    .px(px(8.))
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .bg(hex(CONTROL_BG))
                                    .border_1()
                                    .border_color(hex(CONTROL_BORDER))
                                    .text_size(px(10.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .cursor_pointer()
                                    .hover(|d| d.bg(hex(HOVER_BG)))
                                    .child("Cancel")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.hf_cancel_download(id, cx);
                                        }),
                                    ),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(hex(TEXT_MUTED))
                        .child(status_label),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(3.))
                        .rounded(px(2.))
                        .bg(hex(BORDER))
                        .child(
                            div()
                                .h(px(3.))
                                .w(relative(progress_pct as f32 / 100.0))
                                .rounded(px(2.))
                                .bg(hex(ACCENT_BLUE)),
                        ),
                ),
        );
    }
    col
}

