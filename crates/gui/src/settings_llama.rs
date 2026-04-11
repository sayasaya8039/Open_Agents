//! llama.cpp ランタイム管理、ハードウェアパラメータ、GPU/NPU 設定。

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::colors::*;
use crate::llama_cpp_runtime;
use crate::model_prefs;

use super::{AppView, HardwareParamAdjustKind};

impl AppView {
    // ============================================================
    // llama.cpp ランタイム選択
    // ============================================================

    pub(crate) fn selected_runtime_backend(&self) -> llama_cpp_runtime::BundledLlamaBackend {
        match self.hardware_params.llama_runtime_preset {
            model_prefs::LlamaRuntimePreset::Auto => {
                // Auto: GPU プロファイルの resolved_preset に従う
                let profile = model_prefs::detect_gpu_profile();
                match profile.resolved_preset {
                    model_prefs::LlamaRuntimePreset::NvidiaCuda => {
                        llama_cpp_runtime::BundledLlamaBackend::Cuda
                    }
                    model_prefs::LlamaRuntimePreset::VulkanHybrid => {
                        llama_cpp_runtime::BundledLlamaBackend::Vulkan
                    }
                    _ => llama_cpp_runtime::BundledLlamaBackend::Cpu,
                }
            }
            model_prefs::LlamaRuntimePreset::NvidiaCuda => {
                llama_cpp_runtime::BundledLlamaBackend::Cuda
            }
            model_prefs::LlamaRuntimePreset::VulkanHybrid => {
                llama_cpp_runtime::BundledLlamaBackend::Vulkan
            }
            model_prefs::LlamaRuntimePreset::CpuOnly => {
                llama_cpp_runtime::BundledLlamaBackend::Cpu
            }
        }
    }

    pub(crate) fn runtime_status_for_backend(
        &self,
        backend: llama_cpp_runtime::BundledLlamaBackend,
    ) -> Option<&llama_cpp_runtime::BundledLlamaRuntimeStatus> {
        self.llama_cpp_runtime_statuses
            .iter()
            .find(|status| status.backend == backend)
    }

    pub(crate) fn selected_runtime_manifest(&self) -> Option<&llama_cpp_runtime::BundledLlamaManifest> {
        self.runtime_status_for_backend(self.selected_runtime_backend())
            .and_then(|status| status.manifest.as_ref())
    }

    pub(crate) fn selected_runtime_error(&self) -> Option<String> {
        self.runtime_status_for_backend(self.selected_runtime_backend())
            .and_then(|status| status.error.clone())
    }

    pub(crate) fn runtime_preset_is_available(&self, preset: model_prefs::LlamaRuntimePreset) -> bool {
        match preset {
            model_prefs::LlamaRuntimePreset::Auto => true, // Auto は常に利用可能
            model_prefs::LlamaRuntimePreset::NvidiaCuda => self
                .runtime_status_for_backend(llama_cpp_runtime::BundledLlamaBackend::Cuda)
                .and_then(|status| status.manifest.as_ref())
                .is_some(),
            model_prefs::LlamaRuntimePreset::VulkanHybrid => self
                .runtime_status_for_backend(llama_cpp_runtime::BundledLlamaBackend::Vulkan)
                .and_then(|status| status.manifest.as_ref())
                .is_some(),
            model_prefs::LlamaRuntimePreset::CpuOnly => self
                .runtime_status_for_backend(llama_cpp_runtime::BundledLlamaBackend::Cpu)
                .and_then(|status| status.manifest.as_ref())
                .is_some(),
        }
    }

    pub(crate) fn llama_cpp_bundle_status_line(&self) -> String {
        let backend = self.selected_runtime_backend();
        if let Some(manifest) = self.selected_runtime_manifest() {
            return format!(
                "内蔵 llama-server [{}]: {} ({})",
                backend.label(),
                manifest.llama_server_version,
                manifest.platform
            );
        }
        format!("内蔵 llama-server [{}]: 未同梱", backend.label())
    }

    pub(crate) fn copy_llama_cpp_release_url(&mut self, cx: &mut Context<Self>) {
        if let Some(notice) = &self.llama_cpp_update_notice {
            cx.write_to_clipboard(ClipboardItem::new_string(notice.release_url.clone()));
        }
    }

    pub(crate) fn start_llama_cpp_update_check(&mut self, cx: &mut Context<Self>) {
        self.llama_cpp_update_notice = None;
        let Some(manifest) = self.selected_runtime_manifest().cloned() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let notice = smol::unblock(move || {
                let latest = llama_cpp_runtime::fetch_latest_release().ok()?;
                llama_cpp_runtime::compute_update_notice(&manifest, &latest)
            })
            .await;

            let Some(notice) = notice else {
                return;
            };

            let _ = cx.update(|app| {
                let _ = this.update(app, |this: &mut AppView, cx| {
                    this.llama_cpp_update_notice = Some(notice);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    // ============================================================
    // ハードウェアパラメータ
    // ============================================================

    pub(crate) fn adjust_hardware_param(
        &mut self,
        kind: HardwareParamAdjustKind,
        steps: i32,
        cx: &mut Context<Self>,
    ) {
        match kind {
            HardwareParamAdjustKind::GpuLayers => {
                let v = self.hardware_params.gpu_layers + steps;
                self.hardware_params.gpu_layers = v.clamp(0, 80);
            }
            HardwareParamAdjustKind::NThreads => {
                let v = self.hardware_params.n_threads + steps;
                self.hardware_params.n_threads = v.clamp(1, 32);
            }
            HardwareParamAdjustKind::BatchSize => {
                let v = self.hardware_params.batch_size + steps * 128;
                self.hardware_params.batch_size = v.clamp(128, 2048);
            }
        }
        self.hardware_params.clamp();
        self.persist_local_llm_prefs();
        cx.notify();
    }

    // ============================================================
    // ランタイムプリセット選択 UI
    // ============================================================

    pub(crate) fn settings_runtime_preset_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.hardware_params.llama_runtime_preset;
        let options = model_prefs::LlamaRuntimePreset::ALL;
        let gpu_summary: SharedString = if self.gpu_profile_summary.is_empty() {
            "GPU 検出中…".into()
        } else {
            format!("検出: {}", self.gpu_profile_summary).into()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(self.settings_labeled_block(
                "実行モード",
                "起動時に GPU を自動検出し、最適な runtime を選択します。手動で固定することもできます。",
            ))
            .child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .bg(hex(PANEL_BG))
                    .border_1()
                    .border_color(hex(BORDER))
                    .text_size(px(11.))
                    .text_color(hex(TEXT_SECONDARY))
                    .child(gpu_summary),
            )
            .child(
                div().flex().flex_col().gap(px(8.)).children(options.into_iter().map(|preset| {
                    let is_selected = preset == selected;
                    let is_available = self.runtime_preset_is_available(preset);
                    let badge = if preset.is_experimental() {
                        "Experimental"
                    } else if is_available {
                        "Available"
                    } else {
                        "Unavailable"
                    };
                    div()
                        .p(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(if is_selected {
                            hex(ACCENT_BLUE)
                        } else {
                            hex(CONTROL_BORDER)
                        })
                        .bg(if is_selected {
                            hex_a(ACCENT_BLUE, 0.18)
                        } else {
                            hex(CONTROL_BG)
                        })
                        .cursor_pointer()
                        .when(!is_available, |d| d.opacity(0.55))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                if !this.runtime_preset_is_available(preset) {
                                    return;
                                }
                                this.hardware_params.llama_runtime_preset = preset;
                                this.hardware_params.gpu_acceleration = true;
                                this.persist_local_llm_prefs();
                                this.start_llama_cpp_update_check(cx);
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(hex(TEXT_PRIMARY))
                                                .child(preset.label()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(hex(TEXT_MUTED))
                                                .whitespace_normal()
                                                .child(preset.subtitle()),
                                        ),
                                )
                                .child(
                                    div()
                                        .px(px(10.))
                                        .py(px(4.))
                                        .rounded(px(9999.))
                                        .bg(if is_selected {
                                            hex_a(ACCENT_BLUE, 0.35)
                                        } else if preset.is_experimental() {
                                            hex_a(ACCENT_ORANGE, 0.2)
                                        } else {
                                            hex(0x2c2c2c)
                                        })
                                        .text_size(px(10.))
                                        .text_color(if is_selected {
                                            hex(TEXT_PRIMARY)
                                        } else if is_available {
                                            hex(TEXT_SECONDARY)
                                        } else {
                                            hex(TEXT_MUTED)
                                        })
                                        .child(badge),
                                ),
                        )
                })),
            )
    }

    pub(crate) fn settings_hardware_stepper_row(
        &mut self,
        cx: &mut Context<Self>,
        kind: HardwareParamAdjustKind,
        label: &'static str,
        hint: Option<&'static str>,
    ) -> impl IntoElement {
        let value_str: SharedString = match kind {
            HardwareParamAdjustKind::GpuLayers => {
                format!("{}", self.hardware_params.gpu_layers).into()
            }
            HardwareParamAdjustKind::NThreads => {
                format!("{}", self.hardware_params.n_threads).into()
            }
            HardwareParamAdjustKind::BatchSize => {
                format!("{}", self.hardware_params.batch_size).into()
            }
        };

        let k_minus = kind;
        let k_plus = kind;

        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(hex(TEXT_PRIMARY))
                            .child(label),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(4.))
                                    .rounded(px(6.))
                                    .bg(hex(CONTROL_BG))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.adjust_hardware_param(k_minus, -1, cx);
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child("−"),
                                    ),
                            )
                            .child(
                                div()
                                    .min_w(px(52.))
                                    .text_size(px(12.))
                                    .text_color(hex(TEXT_MUTED))
                                    .text_center()
                                    .child(value_str),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(4.))
                                    .rounded(px(6.))
                                    .bg(hex(CONTROL_BG))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.adjust_hardware_param(k_plus, 1, cx);
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child("+"),
                                    ),
                            ),
                    ),
            )
            .child(div().h(px(4.)).w_full().rounded(px(2.)).bg(hex(CONTROL_BG)));
        if let Some(h) = hint {
            col = col.child(div().text_size(px(11.)).text_color(hex(TEXT_DIM)).child(h));
        }
        col
    }
}
