# Llama Hybrid Runtime Preset Design

**Date:** 2026-04-07

## Goal

`Open_Agents` のローカル GGUF 実行に、`RTX 4090 最優先`、`RTX 4090 + Intel Arc 混成実験`、`Intel NPU 省電力` の 3 つの実行プリセットを追加する。UI は完全プリセット型とし、利用者には複雑な `llama.cpp` 引数を直接見せず、内部で backend 切替・device 選択・フォールバックを行う。

初期マイルストーンの主目的は、`Core Ultra 9 185H / Intel Arc / RTX 4090 Laptop` のような Windows 混成環境で、`4090 + Arc` 実験モードまで到達することにある。

## Current State

- 現在の同梱 runtime は `third_party/llama.cpp/windows-x64/manifest.json` にある `windows-x64-cuda-13.1` のみ
- 現在の UI は `GPU アクセラレーション` と `GPU レイヤー数` しか持たない
- 現在の `llama-server` 起動引数は主に `--n-gpu-layers`、`--threads`、`--batch-size`、`--jinja` に限定されている
- `--device`、`--split-mode`、`--tensor-split`、`--main-gpu`、`OpenVINO` 向け環境変数は未配線

## Scope

- Chat 設定 UI に 3 つの実行プリセットを追加する
- backend ごとの同梱 runtime 解決機構を追加する
- プリセットから backend / device / split policy へ変換する起動ポリシー層を追加する
- `4090 + Arc` 実験モードで混成起動を試し、失敗時に安全なモードへフォールバックする
- 単体テストと疑似統合テストで分岐とフォールバックを検証する

## Non-Goals

- ユーザーに `backend`、`device`、`split-mode`、`tensor-split` を直接編集させること
- 起動時の完全自動最適化
- `CUDA + Arc` のような mixed backend 最適化を保証すること
- `llama.cpp` のソース vendoring や自動ビルド統合
- runtime の自動ダウンロードと自動更新

## Product Decision

### Preset-Only UI

初期版は完全プリセット型とする。設定画面では次の 3 モードをカードまたはラジオ群として提示する。

- `4090 最優先`
  - 通常利用向けの既定モード
  - `CUDA` 同梱 runtime を優先して起動する
- `4090 + Arc 実験`
  - 実験機能として明示表示する
  - `Vulkan` 系 runtime を使い、NVIDIA + Intel GPU の混成実行を試す
- `Intel NPU 省電力`
  - `OpenVINO` 系 runtime を使い、NPU 優先で実行する

既存の `GPU レイヤー数`、`スレッド数`、`バッチサイズ` は残す。ただし説明文は「選択したプリセットの上で適用される補助パラメータ」に更新する。

### No Advanced Controls

初期版では詳細設定の折りたたみや高度な手動 override は導入しない。理由は次の通り。

- 実装の複雑さを runtime policy 側に閉じ込められる
- 混成実験モードで危険な組み合わせを UI から作りにくい
- 将来の詳細設定追加を妨げない

## Architecture

### 1. Preferences Layer

`crates/gui/src/model_prefs.rs` の `HardwareParams` を拡張し、実行プリセットを保持する enum を追加する。

想定例:

- `HighPerformance4090`
- `ExperimentalHybrid4090Arc`
- `IntelNpuEfficient`

既存の `gpu_acceleration` は段階的に役割を縮小し、最終的には「GPU を使うか否か」ではなく「どのプリセットで起動するか」に置き換える。移行期は旧設定を読み込んだ場合に `HighPerformance4090` へ寄せる。

### 2. Bundled Runtime Registry

`third_party/llama.cpp/windows-x64/` を backend ごとの runtime layout に拡張する。

想定レイアウト:

- `third_party/llama.cpp/windows-x64/cuda/`
- `third_party/llama.cpp/windows-x64/vulkan/`
- `third_party/llama.cpp/windows-x64/openvino/`

各ディレクトリに `llama-server.exe`、必要 DLL、`manifest.json` を持たせる。実行時は registry 層がプリセットに対応する backend を解決し、`build.rs` は backend ごとの同梱物を出力ディレクトリへコピーする。

### 3. Launch Policy Layer

新規に「プリセット -> 起動戦略」変換層を追加する。ここで初めて `llama.cpp` 依存の詳細を扱う。

責務:

- 起動対象 backend の決定
- 利用可能 device の列挙
- `--device`、`--split-mode`、必要な環境変数の生成
- 失敗時フォールバック順序の決定

この層を独立させることで、UI はプリセットだけを知り、`llama_cpp_chat.rs` は最終的な起動計画だけを受け取る構造にする。

### 4. Runtime Resolver

runtime resolver は現在の単一 binary 探索を置き換え、backend 名を受けて対応する同梱 binary と manifest を返す。

責務:

- `current_exe()` 配下の backend 別同梱物を優先探索
- 開発時のみ repo 内 `third_party` を探索
- manifest 読み込み
- 更新通知対象 backend の決定

## Preset Behavior

### 4090 最優先

- 優先 backend: `CUDA`
- 既定方針: `RTX 4090` 単独
- `--n-gpu-layers`、`--threads`、`--batch-size` は既存設定を流用
- 初期版では、CUDA 起動失敗時に無理に他 backend へ逃がさず、まずは明確なエラーまたは別プリセット選択を促す

### 4090 + Arc 実験

- 優先 backend: `Vulkan`
- 起動時に device 列挙を行い、NVIDIA と Intel の両 GPU が見えた場合のみ混成起動を試す
- 内部で `--device` と `--split-mode layer` を構成する
- 初期版では `row` / tensor parallel は使用しない

フォールバック順:

1. `4090 + Arc` 混成 `Vulkan`
2. `4090` 単独 `Vulkan`
3. `4090` 単独 `CUDA`

UI には短い状態文だけを表示する。

- 例: `混成起動に失敗したため、4090 単独で継続しています`

詳細な失敗理由はログへ出す。

### Intel NPU 省電力

- 優先 backend: `OpenVINO`
- 優先デバイス: `NPU`
- 内部では `GGML_OPENVINO_DEVICE=NPU` を設定する

フォールバック順:

1. `OpenVINO NPU`
2. `OpenVINO GPU`
3. `CPU`

## Device Detection

初期版では完全自動最適化ではなく、プリセットに必要な最低限の判定だけを行う。

- `CUDA` は runtime と起動成功可否で判断する
- `Vulkan` 混成は `llama-server --list-devices` 相当の列挙結果から NVIDIA / Intel の共存を判断する
- `OpenVINO` は backend 初期化の結果で NPU / GPU / CPU の利用可能性を判断する

自動判定結果は UI に詳細表示せず、ログと短い状態文に留める。

## UI Changes

対象は Chat 設定ブロックのハードウェア設定部分。

- `GPU アクセラレーション` row を削除または置換
- 新たに `実行モード` row を追加し、3 つのプリセットを提示
- `GPU レイヤー数`、`スレッド数`、`バッチサイズ` は継続表示
- 選択中プリセットに対応する runtime manifest の状態を表示する
- `4090 + Arc 実験` は明示的に `Experimental` と表示する

## Error Handling

原則は「安全なモードへ落とす」である。

- 混成モードの失敗でアプリ全体を止めない
- 失敗理由はログで追えるようにする
- UI には短く、次に何が起きたかだけを出す
- runtime が欠損している場合だけは即時に設定エラーとして見せる

## Testing

### Unit Tests

- プリセット enum の既定値と旧設定からの移行
- backend ごとの manifest 解決
- プリセットから起動戦略への変換
- フォールバック順序

### Pseudo-Integration Tests

- device 列挙結果スタブから以下を再現する
  - `4090 + Arc` が両方見える
  - `4090` のみ見える
  - Intel 系のみ見える
  - 何も見えない
- 起動戦略が期待通り `Vulkan hybrid -> Vulkan single -> CUDA single` へ落ちることを検証する

### Manual Validation

少なくとも次を実機で確認する。

- `4090 最優先`
- `4090 + Arc 実験`

`Intel NPU 省電力` は OpenVINO runtime を同梱した時点で確認対象に含める。

## Validation Commands

- `cargo test -p open-agents-gui`
- `cargo test -p open-agents-gui --features test-support`
- `cargo build --release -p open-agents-gui`

## Rollout Notes

- 現在の同梱 manifest は `b8678 / windows-x64-cuda-13.1` であり、backend 増設時は manifest と探索ロジックの前提が変わる
- 初期版では `4090 + Arc` をあくまで実験機能として扱い、速度向上を保証しない
- 将来の拡張で `カード + 詳細設定` へ進化できるよう、プリセット変換層は UI と分離して設計する
