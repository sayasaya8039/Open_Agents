# HANDOFF.md — Open Agents セッション引き継ぎ

**作成日**: 2026-04-10
**バージョン**: v0.4.10 (コミット `b9ab2f0`)
**ブランチ**: master

---

## 今セッションで完了した作業

### 1. i18n 対応 (v0.4.7 → v0.4.8)
- `crates/gui/src/i18n.rs` 新規作成
- Windows `GetUserDefaultUILanguage()` でシステム言語自動検出 (ja/en)
- `t(ja, en)` / `tf(ja, en)` 関数で全UI文字列を切替
- 対象: chat_session, chat_page, model_prefs, api_key_prefs, editor, session_title_editor, main.rs Settings

### 2. llama.cpp 本家更新 (v0.4.8)
- b8201 (Prism fork) → b8724 (ggml-org/llama.cpp 本家) に更新
- CUDA 13.1 / Vulkan / CPU 全 runtime 更新
- CUDA ランタイム DLL (cudart64_13, cublas64_13, cublasLt64_13) を同梱
- Git LFS で大きなDLLを管理 (.gitattributes 追加)

### 3. RotorQuant KV Cache 圧縮 (v0.4.9)
- johndpope/llama-cpp-turboquant fork (feature/planarquant-kv-cache) を**自前ビルド**
- CUDA 13.2 / Vulkan / CPU の3バックエンドでビルド
- Windows DLL export/import 問題を4パッチで解決:
  - `_USE_MATH_DEFINES` (MSVC M_PI)
  - `extern "C" + dllimport` (turbo3_cpu_wht_group_size)
  - `INNERQ_API` マクロ (turbo-innerq.cu/cuh)
  - `llama-kv-cache.cpp` の dllimport 対応
- KvCacheType に Planar3/Iso3/Planar4/Iso4 追加 (model_prefs.rs + i18n.rs)

### 4. Hugging Face モデル検索・ダウンロード (v0.4.10)
- Planner → Generator → Evaluator → Generator(修正) → Evaluator(PASS) フローで開発
- 新規ファイル:
  - `crates/gui/src/hf_discover.rs` — HF API統合、DownloadManager
  - `crates/gui/src/discover_page.rs` — Discover ページ UI
- 機能: 検索 → 詳細 → ダウンロード(1並列) → 自動ローカル登録
- TLS修正: ureq に `native-tls` (SChannel) を明示設定 (rustls で HF API 接続失敗するため)

### 5. README.md 作成
### 6. Portable ZIP + Inno Setup インストーラー作成
- `scripts/build-portable.sh`
- `installer/open_agents.iss` + `installer/open_agents.ico`

---

## GitHub Releases

| バージョン | URL |
|-----------|-----|
| v0.4.7 | https://github.com/sayasaya8039/Open_Agents/releases/tag/v0.4.7 |
| v0.4.8 | https://github.com/sayasaya8039/Open_Agents/releases/tag/v0.4.8 |
| v0.4.9 | https://github.com/sayasaya8039/Open_Agents/releases/tag/v0.4.9 |
| v0.4.10 | https://github.com/sayasaya8039/Open_Agents/releases/tag/v0.4.10 |

---

## 未確認・残タスク

### TLS 修正の動作確認（最優先）
- コミット `b9ab2f0` で `native_tls::TlsConnector` を明示的に `AgentBuilder::tls_connector()` に渡す修正済み
- **ユーザーがアプリ再起動して Discover 検索が動くか未確認**
- もし依然エラーなら:
  - `ureq` のデフォルトフィーチャーから `tls` (rustls) を外す: `default-features = false, features = ["json", "native-tls", "gzip"]`
  - rustls を完全除外して native-tls だけにする

### v0.4.10 リリースのアセット更新
- GitHub Releases v0.4.10 のアセットは TLS 修正前のビルド
- 最終動作確認後、Portable ZIP + インストーラーを再ビルドしてアセットを更新すべき

### Evaluator LOW 指摘（任意改善）
- 検索デバウンス（400ms オートデバウンス未実装、Search ボタン/Enter のみ）
- VRAM 実測値との照合による推奨バリアント自動選択
- ディスク容量事前チェック (`GetDiskFreeSpaceExW`)
- ダウンロード再開 (Range ヘッダ)
- モデルロゴ画像取得
- README の Markdown リッチレンダリング

---

## ビルド手順

```bash
# プロセス停止
taskkill.exe /F /IM open_agents.exe 2>/dev/null || true

# リリースビルド
cd D:/NEXTCLOUD/Windows_app/Open_Agents
cargo zigbuild --release -p open-agents-gui

# Portable ZIP
bash scripts/build-portable.sh

# インストーラー
"/c/Users/Owner/AppData/Local/Programs/Inno Setup 6/ISCC.exe" installer/open_agents.iss

# GitHub Release
git tag vX.Y.Z && git push origin vX.Y.Z
gh release create vX.Y.Z --title "..." --notes "..."
gh release upload vX.Y.Z dist/OpenAgents-X.Y.Z-setup-win-x64.exe dist/OpenAgents-X.Y.Z-portable-win-x64.zip
```

---

## ファイル構成（変更されたもの）

```
crates/gui/src/
├── i18n.rs              ← 新規 (i18n)
├── hf_discover.rs       ← 新規 (HF API + DownloadManager)
├── discover_page.rs     ← 新規 (Discover UI)
├── main.rs              ← Page::Discover 追加、HF ハンドラ 7本
├── chat_page.rs         ← サイドバーに Discover ボタン
├── model_prefs.rs       ← KvCacheType に Planar3/Iso3/Planar4/Iso4
├── api_key_prefs.rs     ← translate_group(), not_set()
├── chat_session.rs      ← i18n 適用
├── chat_composer.rs     ← (変更なし、Enter submit は既存)
├── session_title_editor.rs ← i18n 適用
└── editor/
    ├── mod.rs           ← i18n 適用
    └── buffer.rs        ← i18n 適用

installer/
├── open_agents.iss      ← v0.4.10
└── open_agents.ico

scripts/
├── build-portable.sh
└── install-rotorquant-build.sh

third_party/llama.cpp/windows-x64/
├── manifest.json        ← b8724+rotorquant, commit 20efe75
├── llama-server.exe     ← 自前ビルド (CUDA 13.2, planar3/iso3/planar4/iso4 対応)
├── ggml-cuda.dll        ← Git LFS
├── cublasLt64_13.dll    ← Git LFS
└── ...
```

---

## 既知の技術的注意点

1. **gpui SharedString**: `&self` 由来の参照を渡さない。`.clone().into()` で所有化
2. **ureq TLS**: `native_tls::TlsConnector` を明示的に `tls_connector()` に渡すこと
3. **Git LFS**: 50MB 超の DLL は `.gitattributes` で LFS 追跡が必要
4. **RotorQuant ビルド**: 自前ビルドのパッチは `.tmp/` にのみ残り、git tracked ではない。再ビルドが必要なら `scripts/install-rotorquant-build.sh` 参照
5. **Inno Setup パス**: `C:\Users\Owner\AppData\Local\Programs\Inno Setup 6\ISCC.exe`
