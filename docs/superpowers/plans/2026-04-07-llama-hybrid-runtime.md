# Llama Hybrid Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `4090 最優先` と `4090 + Arc 実験` を中心に、プリセット型の llama.cpp runtime 切替と混成フォールバックを Open_Agents に実装する。

**Architecture:** 設定層に runtime preset を追加し、runtime registry と launch policy で backend 解決・device 列挙・フォールバックを吸収する。UI はプリセット選択だけを持ち、詳細な `llama.cpp` 引数は内部で決定する。

**Tech Stack:** Rust, gpui, serde, llama.cpp bundled runtimes, Windows PowerShell / Cargo

---

### Task 1: 設定モデルと plan 前提を追加する

**Files:**
- Modify: `crates/gui/src/model_prefs.rs`
- Modify: `crates/gui/src/main.rs`
- Test: `crates/gui/src/model_prefs.rs`

- [ ] `LlamaRuntimePreset` を `model_prefs.rs` に追加する
- [ ] `HardwareParams` に preset を追加し、旧設定からの既定値移行を定義する
- [ ] `main.rs` の既存 `gpu_acceleration` 依存箇所を preset ベースへ置換できる準備をする
- [ ] `model_prefs` の sanitize/serde roundtrip テストを追加する

### Task 2: backend 別 runtime registry を追加する

**Files:**
- Modify: `crates/gui/src/llama_cpp_runtime.rs`
- Modify: `crates/gui/build.rs`
- Modify: `third_party/llama.cpp/windows-x64/manifest.json`
- Create: `third_party/llama.cpp/windows-x64/vulkan/manifest.json`
- Test: `crates/gui/src/llama_cpp_runtime.rs`

- [ ] runtime backend kind と manifest model を拡張する
- [ ] `windows-x64/<backend>/...` を探索できる resolver を追加する
- [ ] root 直下の旧 CUDA layout を後方互換として扱う
- [ ] `build.rs` を再帰コピー対応へ更新する
- [ ] runtime resolver の単体テストを追加する

### Task 3: launch policy と混成フォールバックを実装する

**Files:**
- Modify: `crates/gui/src/llama_cpp_chat.rs`
- Modify: `crates/gui/src/chat_client.rs`
- Test: `crates/gui/src/llama_cpp_chat.rs`

- [ ] `--list-devices` の実行と出力解析を追加する
- [ ] `4090 最優先` の CUDA 単独 launch plan を追加する
- [ ] `4090 + Arc 実験` の `Vulkan hybrid -> Vulkan single -> CUDA single` フォールバックを実装する
- [ ] `Intel NPU 省電力` は OpenVINO runtime が無い間は unavailable 扱いにする
- [ ] 生成される引数とフォールバック順序のテストを追加する

### Task 4: 設定 UI と状態表示を更新する

**Files:**
- Modify: `crates/gui/src/main.rs`
- Test: `crates/gui/src/main.rs`

- [ ] `GPU アクセラレーション` row をプリセット選択 UI に置き換える
- [ ] 選択中 preset に対応する runtime 状態文を表示する
- [ ] `Experimental` 表示と unavailable 表示を入れる
- [ ] 設定変更時に prefs が即保存されることを確認する

### Task 5: バージョン更新と検証を完了する

**Files:**
- Modify: `crates/gui/Cargo.toml`
- Modify: `crates/gui/build.rs`

- [ ] GUI version と `OAG_VERSION` を更新する
- [ ] `cargo test -p open-agents-gui` を実行する
- [ ] `cargo build --release -p open-agents-gui` を実行する
- [ ] 変更をコミットし、`git push` を試行する
