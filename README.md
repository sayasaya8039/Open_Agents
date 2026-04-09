# Open Agents

Windows ネイティブの AI コーディングアシスタント。ローカル LLM（GGUF）とクラウド API の両方に対応し、gpui ベースの高速 UI でチャット・コードエディタ・プロジェクト管理を統合する。

## 特徴

- **ローカル推論** — llama.cpp 同梱。GGUF モデルをドラッグ＆ドロップで読み込み、GPU 自動検出（CUDA / Vulkan / CPU）で最適パラメータを設定
- **クラウド API** — OpenAI, Anthropic, Google Gemini, Groq, DeepSeek, xAI, OpenRouter ほか 20+ プロバイダに対応
- **Ollama 連携** — HTTP 経由で Ollama サーバに接続
- **高度な推論** — Chain-of-Thought / Tree-of-Thoughts / ReAct（ツール使用ループ） / Self-Consistency（多数決）
- **コードエディタ** — シンタックスハイライト、IME 対応、ファイル入出力
- **i18n** — システム言語に応じた日英自動切替
- **gpui ベース** — GPU アクセラレーション描画で高速レスポンス

## スクリーンショット

> *（準備中）*

## 必要環境

| 項目 | 要件 |
|------|------|
| OS | Windows 10/11 (x64) |
| Rust | 1.75+ |
| Zig | 0.13+（`cargo-zigbuild` 用） |
| GPU（任意） | NVIDIA (CUDA) / AMD (Vulkan) / Intel Arc (Vulkan) |

## ビルド

```bash
# Release ビルド
cargo zigbuild --release -p open-agents-gui

# 実行
./target/release/open_agents.exe
```

## プロジェクト構成

```
Open_Agents/
├── Cargo.toml              # ワークスペースルート
├── build.zig               # Zig ビルド定義
├── crates/
│   └── gui/
│       └── src/
│           ├── main.rs             # エントリポイント、Settings UI、AppView
│           ├── i18n.rs             # 国際化（日英自動切替）
│           ├── chat_page.rs        # チャット UI
│           ├── chat_session.rs     # セッション管理・永続化
│           ├── chat_client.rs      # クラウド API クライアント
│           ├── chat_composer.rs    # メッセージ入力コンポーネント
│           ├── chat_markdown.rs    # Markdown レンダリング
│           ├── model_prefs.rs      # モデル・ハードウェア・AI 設定
│           ├── api_key_prefs.rs    # API キー管理（37 プロバイダ）
│           ├── llama_cpp_chat.rs   # llama.cpp サーバ連携
│           ├── llama_cpp_runtime.rs # llama.cpp バイナリ管理
│           ├── native_chat.rs      # ネイティブ GGUF 推論
│           ├── editor/             # コードエディタ
│           │   ├── mod.rs          # エディタビュー
│           │   ├── buffer.rs       # テキストバッファ
│           │   ├── cursor.rs       # カーソル管理
│           │   ├── actions.rs      # キーバインド
│           │   ├── grid_renderer.rs # GPU グリッドレンダラ
│           │   └── syntax_highlight.rs # シンタックスハイライト
│           ├── project_explorer.rs # ファイルツリー
│           ├── session_title_editor.rs # セッション名編集
��           └── workspace_prefs.rs  # ワークスペース設定
├── third_party/
│   └── llama.cpp/          # llama.cpp バイナリ（CUDA / Vulkan / CPU）
├── models/                 # GGUF モデルファイル（.gitignore 対象）
├── docs/                   # 仕様書
└── stitch/                 # UI デザインモック
```

## 設定ファイル

| ファイル | パス | 内容 |
|---------|------|------|
| `model_params.json` | `%LOCALAPPDATA%\open_agents_gui\` | モデル・ハードウェア・外観・AI 設定 |
| `api_keys.json` | `%LOCALAPPDATA%\open_agents_gui\` | API キー・URL |
| `chat_sessions.json` | `%LOCALAPPDATA%\open_agents_gui\` | チャット履歴 |
| `last_workspace.txt` | `%LOCALAPPDATA%\open_agents_gui\` | 最後に開いたワークスペース |

## 対応プロバイダ

### クラウド API

OpenAI, Anthropic (Claude), Google Gemini, OpenRouter, Groq, Mistral AI, Cohere, DeepSeek, xAI (Grok), Perplexity, Fireworks AI, Together AI, Replicate, Anyscale, Moonshot (Kimi), Zhipu GLM, SiliconFlow, Novita AI, Nebius

### ローカル / セルフホスト

Ollama, llama.cpp サーバ, LM Studio, vLLM, Hugging Face TGI, OpenAI 互換エンドポイント

### クラウドインフラ

Azure OpenAI, AWS Bedrock

## AI 推論モード

| モード | 説明 |
|--------|------|
| **Chain-of-Thought** | ステップバイステップ推論（基本 / 4段階） |
| **Tree-of-Thoughts** | 複数思考経路を探索・評価・合成（2 or 3 経路） |
| **ReAct** | Thought → Action → Observation ループ（検索・計算ツール使用） |
| **Self-Consistency** | 同じ質問を複数回投げて多数決（3 or 5 回） |

## ライセンス

MIT
