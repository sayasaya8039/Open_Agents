# llama-server Bundle Design

**Date:** 2026-04-06

## Goal

`Open_Agents` のローカル GGUF チャットを、外部インストール前提ではなく同梱済み `llama-server.exe` で動かす。加えて、同梱バージョンと `ggml-org/llama.cpp` の最新リリースとの差分を起動時に確認し、設定画面で更新通知を出す。

## Scope

- Windows x64 CPU 版の `llama-server.exe` と必要 DLL のみを同梱する
- Chat のローカル GGUF は常に同梱版 `llama-server` を使う
- `llama.cpp` の更新監視は `llama-server.exe` のみを対象にする
- 更新検知は通知のみで、自動更新は行わない

## Non-Goals

- CUDA / Vulkan / SYCL 版の同梱
- `llama.cpp` ソースの vendoring とビルド統合
- Chat ローカル ONNX の有効化
- `llama-server` の自動ダウンロード・自動更新

## Architecture

### Bundled Runtime Layout

- `third_party/llama.cpp/windows-x64/`
  - `llama-server.exe`
  - 必要 DLL 群
  - `manifest.json`
- `crates/gui/build.rs`
  - `third_party` の同梱物を Cargo の出力ディレクトリへコピーする
- `crates/gui/src/llama_cpp_chat.rs`
  - 同梱版 `llama-server.exe` を最優先で探索して起動する

### Version Manifest

`manifest.json` は最低限次を持つ。

- `llama_cpp_tag`
- `llama_server_version`
- `platform`
- `source_release_url`

この manifest を実行時にも読めるようにして、設定 UI で現在の同梱版表示と最新リリースとの差分判定に使う。

### Runtime Resolution

ローカル GGUF を選択した場合の優先順位は次の通り。

1. `current_exe().parent()/llama-server.exe`
2. 開発時のみ `third_party/llama.cpp/windows-x64/llama-server.exe`

`PATH` 探索にはフォールバックしない。見つからなければ「同梱ランタイムが壊れている/欠けている」ことを明示する。

### Update Check

- アプリ起動時に非同期で `https://api.github.com/repos/ggml-org/llama.cpp/releases/latest` を確認する
- `manifest.json` の `llama_cpp_tag` と GitHub 最新 `tag_name` を比較する
- 差分があれば、設定画面の Chat 推論ブロックに通知を表示する
- オフラインや GitHub 制限時は静かに失敗し、チャット機能には影響させない

## UI

Chat 推論設定ブロックに次の表示を追加する。

- 現在の同梱 `llama-server` バージョン
- 最新版ありの場合の警告文
- GitHub Releases への URL

## Validation

- 同梱パス優先解決の単体テスト
- manifest 読み込みとバージョン比較の単体テスト
- 同梱ランタイム欠損時のエラー文言テスト
- `cargo test -p open-agents-gui`
- `cargo test -p open-agents-gui --features test-support`
- `cargo build --release -p open-agents-gui`
