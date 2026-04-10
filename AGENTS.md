# AGENTS.md

このファイルは、Codex (Codex.ai/code) がこのリポジトリのコードを扱う際のガイダンスを提供します。

> 共通指示事項は ~/.Codex/AGENTS.md を参照

## プロジェクト概要

WezTerm 向けの AI エージェント監視ダッシュボード。tmux-agent-sidebar の WezTerm 版として、Codex と Codex のエージェントの状態をリアルタイムで監視する。

- **言語**: Rust (edition 2024) + Lua (WezTerm 設定)
- **TUI フレームワーク**: Ratatui 0.29 + Crossterm 0.29
- **状態管理**: OSC 1337 User Variables (WezTerm のペインごとの変数)

## アーキテクチャ

### Rust バイナリ (2つのモード)
1. **TUI モード** (引数なし): サイドバーペインでダッシュボードを描画
2. **CLI モード** (`hook`, `toggle`, `version`): エージェントフックやサイドバー操作を処理

### データフロー
```
Agent hooks → hook.sh → binary "hook" → OSC 1337 User Variables + activity log
TUI binary → wezterm cli list --format json → Ratatui rendering
Lua config → Status Bar summary + keybinding
```

### ディレクトリ構造
- `src/cli/` — CLI サブコマンド (hook, toggle, label)
- `src/ui/` — TUI レンダリング (agents, bottom, colors, text)
- `src/wezterm.rs` — WezTerm CLI 連携
- `src/state.rs` — AppState 中央管理
- `src/activity.rs` — アクティビティログ I/O
- `src/git.rs` — Git 情報取得 (background thread)
- `src/group.rs` — リポジトリ別グルーピング
- `src/user_vars.rs` — OSC 1337 エンコード/デコード
- `lua/` — WezTerm Lua 設定モジュール

## 開発コマンド

```bash
cargo build              # ビルド
cargo build --release     # リリースビルド
cargo test                # テスト実行
cargo clippy              # Lint
cargo fmt                 # フォーマット
```

## 重要な設計判断

- **状態保存**: tmux の `@pane_*` → WezTerm の OSC 1337 User Variables (stdout 直接出力、サブプロセス不要で高速)
- **ペイン一覧**: `wezterm cli list --format json` で JSON デシリアライズ (型安全)
- **即時更新**: SIGUSR1 シグナルなし、1秒ポーリング (WezTerm に相当する仕組みがないため)
- **階層**: tmux session → WezTerm workspace / tmux window → tab / tmux pane → pane
