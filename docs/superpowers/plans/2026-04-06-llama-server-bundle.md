# llama-server Bundle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `Open_Agents` に Windows x64 CPU 版 `llama-server.exe` を同梱し、ローカル GGUF チャットが外部インストール不要で動作するようにする。

**Architecture:** `third_party/llama.cpp/windows-x64/` に同梱資材と manifest を置き、`build.rs` が Cargo 出力先へコピーする。実行時は `llama_cpp_chat.rs` が同梱版を優先起動し、`main.rs` が GitHub 最新リリースとの差分を設定画面に通知する。

**Tech Stack:** Rust, gpui, ureq, Cargo build script, GitHub Releases API, bundled llama.cpp Windows binaries

---

### Task 1: 資材配置と manifest 追加

**Files:**
- Create: `third_party/llama.cpp/windows-x64/manifest.json`
- Create: `third_party/llama.cpp/windows-x64/*.exe`
- Create: `third_party/llama.cpp/windows-x64/*.dll`

- [ ] **Step 1: 最新 Windows x64 CPU 版 `llama-server` 資材を取得する**

Run: `Invoke-WebRequest https://github.com/ggml-org/llama.cpp/releases/download/<tag>/llama-<tag>-bin-win-cpu-x64.zip`
Expected: zip が取得できる

- [ ] **Step 2: `llama-server.exe` と必要 DLL を `third_party` へ配置する**

Run: `Expand-Archive` と `Copy-Item`
Expected: `third_party/llama.cpp/windows-x64/` に runtime 一式がある

- [ ] **Step 3: manifest を追加する**

Expected: tag, version, platform, source URL が保存される

### Task 2: ビルド時コピー導線

**Files:**
- Modify: `crates/gui/build.rs`

- [ ] **Step 1: 出力ディレクトリ解決の失敗テストケースを洗い出す**
- [ ] **Step 2: `third_party` から Cargo 出力先へ runtime をコピーする最小実装を書く**
- [ ] **Step 3: `rerun-if-changed` を追加する**
- [ ] **Step 4: バージョン定数も更新する**

### Task 3: runtime 解決と更新通知

**Files:**
- Modify: `crates/gui/src/llama_cpp_chat.rs`
- Create: `crates/gui/src/llama_cpp_runtime.rs`
- Modify: `crates/gui/src/main.rs`

- [ ] **Step 1: manifest 読み込みと GitHub latest 比較ロジックを書く**
- [ ] **Step 2: 同梱 `llama-server.exe` 優先の探索ロジックへ差し替える**
- [ ] **Step 3: 同梱欠損時のエラー文言を更新する**
- [ ] **Step 4: 起動時に非同期 update check を走らせる**
- [ ] **Step 5: 設定 UI に bundle 状態と更新通知を表示する**

### Task 4: テストと検証

**Files:**
- Modify: `crates/gui/src/llama_cpp_chat.rs`
- Create: `crates/gui/src/llama_cpp_runtime.rs` (tests)
- Modify: `crates/gui/Cargo.toml`

- [ ] **Step 1: version を上げる**
- [ ] **Step 2: 同梱パス優先・manifest 比較・欠損エラーのテストを書く**
- [ ] **Step 3: `cargo test -p open-agents-gui` を通す**
- [ ] **Step 4: `cargo test -p open-agents-gui --features test-support` を通す**
- [ ] **Step 5: `cargo build --release -p open-agents-gui` を通す**
- [ ] **Step 6: `git commit` と `git push` を行う**
