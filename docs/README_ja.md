# wezterm-agent-dashboard

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](../LICENSE)

WezTerm 上で AI コーディングエージェント（Claude Code など）の動作状況をリアルタイムに監視するダッシュボードプラグインです。

> [English](../README.md)

## 概要

wezterm-agent-dashboard は、[WezTerm](https://wezfurlong.org/wezterm/) 内にステータスバーとダッシュボードオーバーレイを表示し、AI コーディングエージェントのアクティビティを追跡します。トークン使用量、実行中のタスク、セッション時間などをターミナルから離れることなく確認できます。

## 機能

- AI エージェントセッションのリアルタイム監視
- トークン使用量・コストの追跡
- 実行中のタスク・ツール呼び出しの可視化
- WezTerm ステータスバーへの軽量な統合
- 非アクティブなタブへのエージェント状態アイコン・色付け
- キーバインドで切り替え可能なダッシュボードオーバーレイ
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

agent_dashboard.setup()
agent_dashboard.apply_to_config(config)

return config
```

## 使い方

インストール後、AI エージェントのセッションが検出されると自動的にダッシュボードのステータスバーが表示されます。

| キーバインド | 動作 |
|---|---|
| `LEADER+e` | ダッシュボードサイドバーの表示切り替え |

## 設定

`setup` にオプションを渡すことでカスタマイズできます：

```lua
agent_dashboard.setup({
  toggle_key = { key = "e", mods = "LEADER" },
  sidebar_percent = 20,
  sidebar_position = "Right",
  tab_status = {
    enabled = true,
    reset_on_active = true,
    states = {
      notification = { icon = "🔔", bg_color = "#3b2f00", fg_color = "#ffd75f" },
      error = { icon = "✕", bg_color = "#3a1f1f", fg_color = "#ff5f5f" },
      waiting = { icon = "◐", bg_color = "#332b12", fg_color = "#ffd75f" },
      running = { icon = "●", bg_color = "#16351f", fg_color = "#87d787" },
    },
  },
})
agent_dashboard.apply_to_config(config)
```

`tab_status.enabled` を true にすると、エージェント pane を含む非アクティブなタブに `agent_attention` / `agent_status` に応じたアイコンと色を表示できます。通知の表示は、そのタブがアクティブになった時点で既読扱いになります。

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

### Codex

利用している Codex で hook 機能の有効化が必要な場合は、`~/.codex/config.toml` に以下を設定してください:

```toml
[features]
codex_hooks = true
```

`~/.codex/hooks.json` に以下を追加してください:

```json
{
  "hooks": {
    "SessionStart": [
      { "hooks": [{ "type": "command", "command": "wezterm-agent-dashboard hook codex SessionStart" }] }
    ],
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command", "command": "wezterm-agent-dashboard hook codex UserPromptSubmit" }] }
    ],
    "PostToolUse": [
      { "hooks": [{ "type": "command", "command": "wezterm-agent-dashboard hook codex PostToolUse" }] }
    ],
    "Stop": [
      { "hooks": [{ "type": "command", "command": "wezterm-agent-dashboard hook codex Stop" }] }
    ]
  }
}
```

特定のツールだけを記録したい場合は、Codex の `PostToolUse` に `matcher` を追加できます。

### サポートされているイベント

`hook` サブコマンドの3番目の引数として以下のイベント名を渡せます:

| イベント / hook alias | 効果 |
|---|---|
| `user-prompt-submit` / `UserPromptSubmit` | エージェントを `running` に設定し、ユーザープロンプトを記録 |
| `notification` / `Notification` | エージェントを `waiting` に設定（権限要求など） |
| `stop` / `Stop` | エージェントを `idle` に設定し、最後の応答を記録 |
| `stop-failure` / `StopFailure` | エージェントを `error` に設定 |
| `session-start` / `SessionStart` | エージェント状態をリセット |
| `session-end` / `SessionEnd` | 状態とアクティビティログをクリア |
| `activity-log` / `PreToolUse` / `PostToolUse` / `PostToolUseFailure` | ツール使用のエントリをアクティビティログに追加 |
| `subagent-start` / `subagent-stop` | 起動中のサブエージェントを追跡 |

Codex で利用できる hook は Codex CLI のバージョンに依存します。上記の推奨設定では `SessionStart`、`UserPromptSubmit`、`PostToolUse`、`Stop` を使います。利用中の Codex が `Notification`、`SessionEnd`、`PostToolUseFailure` を提供していない場合、dashboard はそれらの遷移を Codex から受け取れません。その場合も次回の `SessionStart` で状態をリセットし、`Stop` で pane を idle に戻します。`PreToolUse` も alias として受け付けますが、`PostToolUse` と同時に設定すると activity が重複するため、標準例には含めていません。

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
