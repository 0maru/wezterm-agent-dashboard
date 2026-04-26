# wezterm-agent-dashboard

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](../LICENSE)

WezTerm 上で AI コーディングエージェント（Claude Code など）の動作状況をリアルタイムに監視するダッシュボードプラグインです。

> [English](../README.md)

## 概要

wezterm-agent-dashboard は、[WezTerm](https://wezfurlong.org/wezterm/) 内に軽量なステータスバー集計と分割ペインのダッシュボードを表示します。Claude Code と Codex の hook イベントを受け取り、エージェント状態、直近のツール利用、タスク進捗、リポジトリグルーピング、Git 状態をターミナルから離れずに確認できます。

## 機能

- Claude Code / Codex ペインのリアルタイム監視
- running / waiting / idle / error の件数を WezTerm ステータスバーに表示
- キーバインドで切り替え可能な分割ペインのダッシュボード
- プロンプトまたは最終応答、待機理由、経過時間、permission mode、subagent の表示
- file、shell、search、web、task、skill、messaging 系ツールのラベル付き activity log
- `TaskCreate` / `TaskUpdate` に基づくタスク進捗表示
- リポジトリ単位のグルーピング、リポジトリフィルタ、Git ブランチと worktree 表示
- ブランチ、ahead/behind、変更ファイル、diff 統計、remote URL、GitHub PR 番号を表示する Git パネル
- 複数エージェントセッションの同時監視に対応

## 必要環境

- [WezTerm](https://wezfurlong.org/wezterm/)（nightly または v20240101 以降）
- Rust 1.75 以上（ソースからビルドする場合）

## インストール

### Homebrew（推奨）

```sh
brew tap 0maru/formulae https://github.com/0maru/homebrew-formulae
brew install wezterm-agent-dashboard
```

macOS（arm64/x86_64）と Linux（x86_64）向けのプリビルドバイナリを配信しています。

### ソースからビルド

```sh
git clone https://github.com/0maru/wezterm-agent-dashboard.git
cd wezterm-agent-dashboard
cargo build --release
```

### WezTerm の設定

`~/.wezterm.lua` に以下を追加してください：

```lua
local wezterm = require("wezterm")
local config = wezterm.config_builder()

-- エージェントダッシュボードプラグインを読み込み
local agent_dashboard = wezterm.plugin.require("https://github.com/0maru/wezterm-agent-dashboard")

agent_dashboard.apply_to_config(config)

return config
```

## 使い方

インストール後、AI エージェントのセッションが検出されると自動的にステータスバー集計が表示されます。

| キーバインド | 動作 |
|---|---|
| `LEADER+e` | ダッシュボードサイドバーの表示切り替え |

ダッシュボード内では以下のキーを使用できます。

| キー | 動作 |
|---|---|
| `Tab` | filter、agent list、bottom panel のフォーカス切り替え |
| `Shift+Tab` | bottom panel の Activity / Git 切り替え |
| `h` / `l`, `Left` / `Right` | filter bar にフォーカスがあるときにステータスフィルタを変更 |
| `j` / `k`, `Down` / `Up` | エージェント選択または repo popup 選択を移動 |
| `Enter` | 選択中エージェントペインへ移動、または選択中 repo filter を適用 |
| `r` | リポジトリフィルタ popup の表示切り替え |
| `Esc` | repo popup を閉じる、または repo filter を解除 |
| `q`, `Ctrl+C` | ダッシュボードペインを終了 |

## 設定

`apply_to_config()` を呼ぶ前に `setup()` で Lua プラグインをカスタマイズできます。

```lua
agent_dashboard.setup({
  toggle_key = { key = "e", mods = "LEADER" },
  sidebar_percent = 20,
  sidebar_position = "Right",
  show_status_bar = true,
  binary_name = "wezterm-agent-dashboard",
})

agent_dashboard.apply_to_config(config)
```

`apply_to_config()` は WezTerm config に status handler と toggle keybinding を追加します。

## CLI

バイナリは引数なしで起動すると TUI モードで動作します。補助サブコマンドも提供しています。

| コマンド | 効果 |
|---|---|
| `wezterm-agent-dashboard hook <agent> <event>` | Claude Code / Codex hook イベントを受け取る |
| `wezterm-agent-dashboard toggle [percent]` | ダッシュボードサイドバーを切り替え、開くときは `percent` を分割幅として使う |
| `wezterm-agent-dashboard version` / `--version` | パッケージバージョンを表示 |

## エージェントフック

ダッシュボードは Claude Code / Codex からのフック呼び出しでエージェントの状態を受け取ります。`wezterm-agent-dashboard` が `PATH` に入っている場合（Homebrew 経由のインストールなど）、シェルラッパーを経由せずに直接バイナリを呼び出せます。

### Claude Code

`~/.claude/settings.json` に以下を追加してください:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command", "command": "wezterm-agent-dashboard hook claude user-prompt-submit" }] }
    ],
    "Notification": [
      { "hooks": [{ "type": "command", "command": "wezterm-agent-dashboard hook claude notification" }] }
    ],
    "Stop": [
      { "hooks": [{ "type": "command", "command": "wezterm-agent-dashboard hook claude stop" }] }
    ],
    "SessionStart": [
      { "hooks": [{ "type": "command", "command": "wezterm-agent-dashboard hook claude session-start" }] }
    ],
    "SessionEnd": [
      { "hooks": [{ "type": "command", "command": "wezterm-agent-dashboard hook claude session-end" }] }
    ],
    "PostToolUse": [
      { "hooks": [{ "type": "command", "command": "wezterm-agent-dashboard hook claude activity-log" }] }
    ]
  }
}
```

### サポートされているイベント

`hook` サブコマンドの3番目の引数として以下のイベント名を渡せます:

| イベント | 効果 |
|---|---|
| `user-prompt-submit` | エージェントを `running` に設定し、ユーザープロンプトを記録 |
| `notification` | エージェントを `waiting` に設定（権限要求など） |
| `stop` | エージェントを `idle` に設定し、最後の応答を記録 |
| `stop-failure` | エージェントを `error` に設定 |
| `session-start` | エージェント状態をリセット |
| `session-end` | 状態とアクティビティログをクリア |
| `activity-log` | ツール使用のエントリをアクティビティログに追加 |
| `subagent-start` / `subagent-stop` | 起動中のサブエージェントを追跡 |

Codex のフックでは `claude` の代わりに `codex` を渡してください。

### レガシー: `hook.sh` ラッパー

バイナリが `PATH` に入っていない場合（`cargo install` を使わずソースビルドした環境など）は、同梱の `hook.sh` を経由できます。これは一般的なインストール先を探索して見つけたバイナリに委譲します:

```json
{ "type": "command", "command": "bash /path/to/wezterm-agent-dashboard/hook.sh claude user-prompt-submit" }
```

## 開発

```sh
# テストの実行
cargo test

# デバッグログ付きで実行
RUST_LOG=debug cargo run
```

## コントリビュート

コントリビュートを歓迎します！Issue の作成やプルリクエストの送信をお気軽にどうぞ。

1. リポジトリをフォーク
2. フィーチャーブランチを作成（`git checkout -b feature/amazing-feature`）
3. 変更をコミット（`git commit -m 'Add amazing feature'`）
4. ブランチにプッシュ（`git push origin feature/amazing-feature`）
5. プルリクエストを作成

## ライセンス

このプロジェクトは MIT ライセンスの下で公開されています。詳細は [LICENSE](../LICENSE) を参照してください。
