# Prism-Ready Llama Runtime Replacement Design

**Date:** 2026-04-07

## Goal

`Open_Agents` の同梱 `llama.cpp` runtime を、Prism ML の `Bonsai-8B-GGUF` を含む `Q1_0` 系 GGUF を扱える対応版へ全面置換する。対象 backend は `CUDA`、`Vulkan`、`CPU` の 3 系統とし、利用者には通常の GGUF モデルと同じ UX を維持する。

## Current State

- 現在の bundled runtime は公式 `ggml-org/llama.cpp` ベースで、`CUDA` と `Vulkan` の資材が同梱されている
- runtime 更新通知は `ggml-org/llama.cpp` の latest release を参照している
- [crates/gui/src/llama_cpp_chat.rs](D:/NEXTCLOUD/Windows_app/Open_Agents/crates/gui/src/llama_cpp_chat.rs) では、固定の tensor type 上限による事前ブロックがあり、新しい量子化形式に弱い
- 利用者視点では `GGUF/ONNX` の選択 UI は既にあり、runtime の fork 差し替えは UI の主導線を変えずに実施できる

## Scope

- `CUDA`、`Vulkan`、`CPU` の bundled runtime を Prism 対応版へ置換する
- runtime manifest / 探索 / 同梱コピーの前提を Prism 対応版に合わせて更新する
- `Q1_0` を含む拡張 tensor type を固定上限で落とさない互換性判定へ変更する
- モデル選択 UI は変更せず、通常 GGUF と同じ扱いで読み込めるようにする
- 更新通知の比較先は `ggml-org/llama.cpp` のまま維持する

## Non-Goals

- `Prism` 専用の新しいモデル種別や専用 UI を追加すること
- runtime の自動ダウンロードや自動更新
- `OpenVINO` / `NPU` backend の同時導入
- upstream と Prism fork の差分を UI 上で詳細比較すること
- 既存の chat / settings 画面全体の再設計

## Product Decisions

### 1. Runtime は全面置換する

既存の bundled runtime を「Prism 対応 runtime を追加する」のではなく、「Prism 対応 runtime へ置き換える」。理由は次の通り。

- 利用者に fork 差異を意識させず、通常 GGUF と同じ使い方を維持できる
- UI や設定の分岐を増やさずに済む
- `Bonsai-8B-GGUF` だけの特例実装ではなく、Prism 系 1-bit GGUF 全般への対応基盤になる

### 2. モデルは通常 GGUF と同じ扱いにする

`Q1_0` 系モデルだけを別ラベルで区別しない。利用者は通常の GGUF と同様にモデルパスを選び、同じ Chat 導線で実行する。

### 3. 更新通知は upstream 比較を維持する

同梱 runtime は Prism 対応 fork を使うが、更新通知の比較先は `ggml-org/llama.cpp` のままにする。UI 文言では「同梱 runtime の比較先が upstream である」ことを誤解なく表現する。

## Architecture

### Runtime Bundle Layout

`third_party/llama.cpp/windows-x64/` を Prism 対応 runtime 置換後の canonical source of truth とする。想定レイアウトは次の 3 系統。

- `third_party/llama.cpp/windows-x64/` または `.../cuda/` に `CUDA` runtime
- `third_party/llama.cpp/windows-x64/vulkan/` に `Vulkan` runtime
- `third_party/llama.cpp/windows-x64/cpu/` または同等の `CPU` runtime

どの配置を採るかは、既存 resolver の変更量が最小になる形を優先する。`build.rs` は再帰コピーを継続利用し、Cargo 出力先へ backend ごとの runtime tree を同期する。

### Runtime Metadata Layer

[crates/gui/src/llama_cpp_runtime.rs](D:/NEXTCLOUD/Windows_app/Open_Agents/crates/gui/src/llama_cpp_runtime.rs) を bundled runtime registry の責務として維持する。

責務:

- backend ごとの runtime 探索パス解決
- manifest 読み込み
- bundled runtime の状態表示
- upstream release との比較情報生成

ここでは「どの fork を使うか」は manifest と asset layout に閉じ込め、UI 側は backend 状態だけを知る構造にする。

### Compatibility Validation Layer

現在の [crates/gui/src/llama_cpp_chat.rs](D:/NEXTCLOUD/Windows_app/Open_Agents/crates/gui/src/llama_cpp_chat.rs) にある固定 `type_id` 上限チェックは、Prism 系量子化に対して brittle である。ここは次の方針へ変更する。

- 事前検査は「絶対に止めるべき既知の非互換」のみに縮小する
- tensor type の固定上限による包括ブロックはやめる
- 最終的な可否は bundled `llama-server` の実ロード結果に寄せる
- 起動失敗時は runtime が返した失敗内容を UI 向けメッセージへ整形する

この方針により、`Q1_0` だけでなく将来追加される拡張量子化にも追随しやすくする。

### Preserved Hard Blocks

すべてを runtime 任せにするのではなく、既知の根本非互換は preflight に残す。

例:

- Gemma 4 のように現行ローカル実装が tensor layout / attention 構造に未対応なもの
- モデルファイル破損や明らかなフォーマット不正

## UI Behavior

UI の基本導線は変えない。

- `GGUF/ONNX` 選択 UI はそのまま
- Prism 系 GGUF も通常 GGUF と同じように選択・起動する
- runtime fork 名を大きく露出しない
- 必要なら設定画面の bundled runtime 状態文だけを更新し、同梱物が Prism 対応版であることを補足する

エラー時のみ、次のような説明を出す。

- この GGUF は現在の bundled runtime で読み込めない
- runtime の起動に失敗した
- upstream 比較情報は参考であり、同梱物自体は fork ベースである

## Error Handling

### Runtime Load Failure

`llama-server` がモデルロードに失敗した場合は、固定の generic error ではなく、stderr / 応答本文 / 既知パターンから利用者向けメッセージを生成する。

目標:

- 「未対応 tensor type」だけで止めない
- 実際に bundled runtime が返した理由を優先する
- ただし raw stderr をそのまま UI に漏らさず、要点を整形する

### Manifest / Bundle Failure

runtime 資材が欠けている場合は設定エラーとして即時表示する。これはモデル互換性ではなくアプリ同梱物の問題であり、早く失敗させるべきである。

## Testing Strategy

### Unit Tests

- manifest 読み込みと backend 解決
- bundled runtime 探索順
- upstream update notice の文言生成
- GGUF preflight が `Q1_0` 系を固定上限でブロックしないこと
- Gemma 4 など既知非互換は従来通り止めること

### Runtime Error Normalization Tests

- `llama-server` の想定失敗メッセージから、利用者向けエラーが正しく生成されること
- unknown error でも情報を潰しすぎないこと

### Build Verification

- `cargo test -p open-agents-gui`
- `cargo build --release -p open-agents-gui`

必要に応じて `cargo clean` 後の release build でも bundled runtime のコピーを含めて確認する。

## Risks

- Prism fork の Windows 配布物が `CUDA / Vulkan / CPU` で均一に揃っていない可能性
- upstream 比較を残すことで、「更新通知」と「同梱 runtime の実体」がずれる
- runtime 実ロード結果に判定を寄せるぶん、事前検査だけで完全な UX は保証できない

## Mitigations

- manifest に fork / asset / backend 情報を明示し、探索・表示を一貫させる
- UI の状態文で upstream 比較であることを補足する
- runtime 失敗時のメッセージ整形を強化し、事後失敗でも利用者に次の行動が分かるようにする

## Validation Commands

- `cargo test -p open-agents-gui`
- `cargo build --release -p open-agents-gui`
