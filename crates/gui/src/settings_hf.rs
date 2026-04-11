//! Hugging Face Discover ハンドラ（検索・モデル詳細取得・ダウンロード管理）。

use gpui::*;

use crate::hf_discover;

use super::{AppView, Page};

impl AppView {
    pub(crate) fn hf_open_discover(&mut self, cx: &mut Context<Self>) {
        self.page = Page::Discover;
        cx.notify();
    }

    pub(crate) fn hf_cycle_sort(&mut self, cx: &mut Context<Self>) {
        self.hf_state.sort = match self.hf_state.sort {
            hf_discover::SortOrder::Trending => hf_discover::SortOrder::Downloads,
            hf_discover::SortOrder::Downloads => hf_discover::SortOrder::Likes,
            hf_discover::SortOrder::Likes => hf_discover::SortOrder::LastModified,
            hf_discover::SortOrder::LastModified => hf_discover::SortOrder::Trending,
        };
        cx.notify();
        // 既に検索済みなら同じクエリで再検索
        if !self.hf_state.results.is_empty() || !self.hf_state.query.is_empty() {
            self.hf_execute_search(cx);
        }
    }

    pub(crate) fn hf_toggle_downloads_panel(&mut self, cx: &mut Context<Self>) {
        self.hf_downloads.panel_open = !self.hf_downloads.panel_open;
        cx.notify();
    }

    pub(crate) fn hf_execute_search(&mut self, cx: &mut Context<Self>) {
        // 検索中は再入禁止（連打・Enter 連打対策）
        if self.hf_state.loading {
            return;
        }
        // 検索バーから現在のテキストを取得
        let query = self.hf_search_composer.read(cx).text().trim().to_string();
        self.hf_state.query = query.clone();
        self.hf_state.loading = true;
        self.hf_state.error = None;
        self.hf_state.request_gen = self.hf_state.request_gen.wrapping_add(1);
        let gen = self.hf_state.request_gen;
        let sort = self.hf_state.sort;
        let token = self.api_keys.get_value("huggingface");
        cx.notify();

        cx.spawn(async move |app: WeakEntity<AppView>, cx: &mut AsyncApp| {
            let token_opt = if token.is_empty() { None } else { Some(token) };
            let result = smol::unblock(move || {
                hf_discover::search_hf_models(&query, sort, token_opt.as_deref())
            })
            .await;
            let _ = cx.update(|ecx| {
                let _ = app.update(ecx, |this: &mut AppView, cx| {
                    // 古いレスポンスは破棄
                    if this.hf_state.request_gen != gen {
                        return;
                    }
                    this.hf_state.loading = false;
                    match result {
                        Ok(models) => {
                            this.hf_state.results = models;
                        }
                        Err(e) => {
                            this.hf_state.error = Some(e);
                            this.hf_state.results.clear();
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub(crate) fn hf_select_model(&mut self, id: String, cx: &mut Context<Self>) {
        // 既に同じモデルが選択済みなら再取得しない
        if self.hf_state.selected_id.as_deref() == Some(id.as_str()) {
            return;
        }
        self.hf_state.selected_id = Some(id.clone());
        self.hf_state.detail = None;
        self.hf_state.detail_loading = true;
        self.hf_state.detail_error = None;
        let token = self.api_keys.get_value("huggingface");
        cx.notify();

        cx.spawn(async move |app: WeakEntity<AppView>, cx: &mut AsyncApp| {
            let token_opt = if token.is_empty() { None } else { Some(token) };
            let id_c = id.clone();
            let result = smol::unblock(move || {
                hf_discover::fetch_model_detail(&id_c, token_opt.as_deref())
            })
            .await;
            let _ = cx.update(|ecx| {
                let _ = app.update(ecx, |this: &mut AppView, cx| {
                    // 選択が変わっていたら破棄
                    if this.hf_state.selected_id.as_deref() != Some(&id) {
                        return;
                    }
                    this.hf_state.detail_loading = false;
                    match result {
                        Ok(detail) => this.hf_state.detail = Some(detail),
                        Err(e) => this.hf_state.detail_error = Some(e),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub(crate) fn hf_start_download(
        &mut self,
        model_id: String,
        file: hf_discover::GgufFile,
        cx: &mut Context<Self>,
    ) {
        // 1. キューにタスクを積む
        let token = self.api_keys.get_value("huggingface");
        let token_opt = if token.is_empty() { None } else { Some(token) };
        self.hf_downloads.enqueue(model_id, &file, token_opt);
        self.hf_downloads.panel_open = true;
        cx.notify();

        // 2. 同時ダウンロード数を最大 3 に制限
        const MAX_CONCURRENT_DOWNLOADS: u32 = 3;
        let prev = self
            .hf_downloads
            .worker_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if prev >= MAX_CONCURRENT_DOWNLOADS {
            self.hf_downloads
                .worker_count
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            return;
        }

        // 3. ワーカー + 進捗ポーリングを起動（最大 3 並列）
        let worker_counter = self.hf_downloads.worker_count.clone();
        let tx = self.hf_downloads.tx.clone();
        cx.spawn(async move |app: WeakEntity<AppView>, cx: &mut AsyncApp| {
            loop {
                // 次のキュー済みタスクを取り出す
                let next_task = cx
                    .update(|ecx| {
                        app.update(ecx, |this: &mut AppView, _| {
                            this.hf_downloads.next_queued_task()
                        })
                        .ok()
                        .flatten()
                    })
                    .ok()
                    .flatten();

                let Some(task) = next_task else {
                    // キュー空 → ワーカー終了
                    break;
                };

                // ダウンロード実行を別スレッドに投げ、同スレッドで進捗 drain
                let tx_c = tx.clone();
                let task_c = task.clone();
                let token_inner = task.hf_token.clone();
                cx.background_executor()
                    .spawn(async move {
                        hf_discover::run_download(task_c, token_inner, tx_c);
                    })
                    .detach();

                // このタスクが完了するまで 300ms ごとに drain
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(300))
                        .await;
                    let finished = cx
                        .update(|ecx| {
                            app.update(ecx, |this: &mut AppView, cx| {
                                let events = this.hf_downloads.drain_progress();
                                let mut any = false;
                                for ev in events {
                                    any = true;
                                    if let hf_discover::DownloadProgress::Completed {
                                        final_path,
                                        ..
                                    } = ev
                                    {
                                        if !this
                                            .settings_model_paths
                                            .iter()
                                            .any(|p| p == &final_path)
                                        {
                                            this.settings_model_paths.push(final_path);
                                            this.persist_local_llm_prefs();
                                        }
                                    }
                                }
                                if any {
                                    cx.notify();
                                }
                                // この特定タスクが終わったか
                                this.hf_downloads
                                    .tasks
                                    .iter()
                                    .find(|t| t.id == task.id)
                                    .map(|t| {
                                        !matches!(
                                            t.status,
                                            hf_discover::DownloadStatus::Queued
                                                | hf_discover::DownloadStatus::InProgress
                                        )
                                    })
                                    .unwrap_or(true)
                            })
                            .ok()
                            .unwrap_or(true)
                        })
                        .ok()
                        .unwrap_or(true);
                    if finished {
                        break;
                    }
                }
            }
            // ワーカー終了 — カウンタをデクリメント
            worker_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        })
        .detach();
    }

    pub(crate) fn hf_cancel_download(&mut self, id: u64, cx: &mut Context<Self>) {
        self.hf_downloads.cancel(id);
        cx.notify();
    }
}
