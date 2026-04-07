# Prism Llama Runtime Replacement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open_Agents の同梱 `llama.cpp` runtime を Prism 互換版へ置き換え、`Q1_0` を含む将来の拡張 GGUF 量子化を UI の事前弾きなしで実行できるようにする。

**Architecture:** runtime bundling は `CUDA / Vulkan / CPU` の 3 backend を明示的に扱う registry へ整理し、upstream 更新通知は `ggml-org/llama.cpp` を比較先として維持する。GGUF 互換性判定は固定 tensor type ceiling に依存せず、既知の根本非互換のみ preflight で止め、最終的な可否は実ランタイムの起動結果とエラー整形に寄せる。

**Tech Stack:** Rust, Cargo, gpui, serde, bundled llama.cpp runtimes, Windows PowerShell

---

### Task 1: runtime registry の期待挙動を failing test で固定する

**Files:**
- Modify: `crates/gui/src/llama_cpp_runtime.rs`
- Test: `crates/gui/src/llama_cpp_runtime.rs`

- [ ] `BundledLlamaBackend` を `Cuda / Vulkan / Cpu` 前提で扱うテストを追加する
- [ ] `Cpu` backend が `cpu/` 配下だけを探索し、legacy root を使わない failing test を追加する
- [ ] update notice が upstream 比較を継続することを明示するテストを追加する
- [ ] Run: `cargo test -p open-agents-gui llama_cpp_runtime -- --nocapture`
- [ ] Commit: `test: cover prism runtime registry expectations`

### Task 2: runtime registry と build copy を Prism layout に合わせる

**Files:**
- Modify: `crates/gui/src/llama_cpp_runtime.rs`
- Modify: `crates/gui/build.rs`
- Modify: `third_party/llama.cpp/windows-x64/manifest.json`
- Create: `third_party/llama.cpp/windows-x64/cpu/manifest.json`
- Modify: `third_party/llama.cpp/windows-x64/vulkan/manifest.json`

- [ ] `OpenVino` backend を `Cpu` backend へ置換する
- [ ] `Cpu` backend の label と `dir_name()` を追加する
- [ ] runtime search dirs を `cuda(root legacy) / vulkan(subdir) / cpu(subdir)` に整理する
- [ ] `build.rs` の bundled runtime sync が新しい `cpu/` subtree をそのままコピーできる状態を確認する
- [ ] Prism 互換 runtime manifest を `CUDA / Vulkan / CPU` で揃える
- [ ] Run: `cargo test -p open-agents-gui bundled_runtime_search_dirs -- --nocapture`
- [ ] Commit: `feat: switch bundled llama runtime registry to prism layout`

### Task 3: GGUF preflight を runtime 実測寄りに切り替える

**Files:**
- Modify: `crates/gui/src/llama_cpp_chat.rs`
- Test: `crates/gui/src/llama_cpp_chat.rs`

- [ ] `Q1_0` 相当の未知 tensor type を preflight で弾かない failing test を追加する
- [ ] 壊れた GGUF / 非 GGUF は引き続き安全に扱う failing test を追加する
- [ ] 固定 `GGML_SUPPORTED_TENSOR_TYPE_COUNT` 依存の validation を削るか縮小し、既知の構造エラーだけ返すようにする
- [ ] 起動失敗ログから runtime 非対応を読みやすく返す正規化関数の failing test を追加する
- [ ] Run: `cargo test -p open-agents-gui unsupported_tensor -- --nocapture`
- [ ] Commit: `feat: relax gguf tensor preflight for prism runtime`

### Task 4: launch failure message と設定 UI の backend 表示を整える

**Files:**
- Modify: `crates/gui/src/llama_cpp_chat.rs`
- Modify: `crates/gui/src/main.rs`
- Test: `crates/gui/src/llama_cpp_chat.rs`

- [ ] runtime 起動失敗時に backend 名とログ tail を含む整形エラーを返す
- [ ] 設定 UI の runtime status 表示を `CUDA / Vulkan / CPU` へ更新する
- [ ] upstream 更新通知文言を「比較先は ggml-org、同梱は Prism 互換 runtime」に合わせる
- [ ] Run: `cargo test -p open-agents-gui llama_server_args -- --nocapture`
- [ ] Commit: `fix: clarify prism runtime status and launch failures`

### Task 5: バージョン更新と最終検証を完了する

**Files:**
- Modify: `crates/gui/Cargo.toml`
- Modify: `crates/gui/build.rs`
- Modify: `build.zig`

- [ ] 設定・runtime 関連変更に合わせて GUI version と `OAG_VERSION` を整合させる
- [ ] Run: `cargo test -p open-agents-gui`
- [ ] Run: `cargo build --release -p open-agents-gui`
- [ ] Run: `git status --short`
- [ ] Commit: `feat: add prism-compatible llama runtime support`
- [ ] Run: `git push`
