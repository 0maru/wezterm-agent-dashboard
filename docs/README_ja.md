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

agent_dashboard.apply_to_config(config)

return config
```

## 使い方

インストール後、AI エージェントのセッションが検出されると自動的にダッシュボードのステータスバーが表示されます。

| キーバインド | 動作 |
|---|---|
| `Ctrl+Shift+A` | ダッシュボードオーバーレイの表示切り替え |

## 設定

`apply_to_config` にオプションを渡すことでカスタマイズできます：

```lua
agent_dashboard.apply_to_config(config, {
  position = "bottom",    -- "top" または "bottom"
  update_interval = 1000, -- 更新間隔（ミリ秒）
})
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
