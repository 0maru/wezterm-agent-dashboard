# wezterm-agent-dashboard

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A WezTerm plugin that provides a real-time dashboard for monitoring AI coding agents (e.g. Claude Code) running in your terminal.

> [日本語ドキュメント (Japanese)](docs/README_ja.md)

## Overview

wezterm-agent-dashboard displays a status bar and dashboard overlay inside [WezTerm](https://wezfurlong.org/wezterm/) that tracks the activity of AI coding agents. Monitor token usage, active tasks, session duration, and more — all without leaving your terminal.

## Features

- Real-time monitoring of AI agent sessions
- Token usage and cost tracking
- Active task and tool call visualization
- Lightweight status bar integration with WezTerm
- Optional inactive tab icon/color styling for agent status
- Dashboard overlay toggled via keybinding
- Support for multiple concurrent agent sessions

## Requirements

- [WezTerm](https://wezfurlong.org/wezterm/) (nightly or v20240101+)
- Rust 1.75+ (for building from source)

## Installation

### Homebrew (recommended)

```sh
brew tap 0maru/formulae https://github.com/0maru/homebrew-formulae
brew install wezterm-agent-dashboard
```

Pre-built binaries are available for macOS (arm64/x86_64) and Linux (x86_64).

### From source

```sh
git clone https://github.com/0maru/wezterm-agent-dashboard.git
cd wezterm-agent-dashboard
cargo build --release
```

### WezTerm configuration

Add the following to your `~/.wezterm.lua`:

```lua
local wezterm = require("wezterm")
local config = wezterm.config_builder()

-- Load the agent dashboard plugin
local agent_dashboard = wezterm.plugin.require("https://github.com/0maru/wezterm-agent-dashboard")

agent_dashboard.setup()
agent_dashboard.apply_to_config(config)

return config
```

## Usage

Once installed, the dashboard status bar appears automatically when an AI agent session is detected.

| Keybinding | Action |
|---|---|
| `LEADER+e` | Toggle dashboard sidebar |

## Configuration

You can customize the plugin by passing options to `setup`:

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

When `tab_status.enabled` is true, inactive tabs containing agent panes can show an icon and tab color based on `agent_attention` / `agent_status`. Notification styling is marked as seen when the tab becomes active.

## Agent Hooks

The dashboard reflects agent state via hook calls from Claude Code / Codex. When `wezterm-agent-dashboard` is in your `PATH` (e.g. installed via Homebrew), you can invoke it directly from your agent's hook configuration — no shell wrapper needed.

### Claude Code

Add the following to `~/.claude/settings.json`:

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

If your Codex build requires the hooks feature flag, enable it in `~/.codex/config.toml`:

```toml
[features]
codex_hooks = true
```

Add the following to `~/.codex/hooks.json`:

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

You can add a Codex `matcher` to `PostToolUse` if you only want to log specific tools.

### Supported events

The third argument passed to `hook` maps to the following agent state transitions:

| Event / hook alias | Effect |
|---|---|
| `user-prompt-submit` / `UserPromptSubmit` | Mark agent as `running`; record the user prompt |
| `notification` / `Notification` | Mark agent as `waiting` (e.g. permission request) |
| `stop` / `Stop` | Mark agent as `idle`; record the last response |
| `stop-failure` / `StopFailure` | Mark agent as `error` |
| `session-start` / `SessionStart` | Reset agent state |
| `session-end` / `SessionEnd` | Clear state and activity log |
| `activity-log` / `PreToolUse` / `PostToolUse` / `PostToolUseFailure` | Append a tool-use entry to the activity log |
| `subagent-start` / `subagent-stop` | Track active subagents |

Codex hook availability depends on the Codex CLI version. The recommended Codex setup above uses `SessionStart`, `UserPromptSubmit`, `PostToolUse`, and `Stop`. If your Codex build does not expose `Notification`, `SessionEnd`, or `PostToolUseFailure`, the dashboard cannot receive those transitions from Codex; it will reset on the next `SessionStart` and mark the pane idle on `Stop`. `PreToolUse` is accepted as an alias, but it is not included in the default example to avoid duplicate activity entries when `PostToolUse` is also configured.

### Legacy: `hook.sh` wrapper

If the binary is not in `PATH` (e.g. building from source without `cargo install`), use the included `hook.sh`, which probes common install locations:

```json
{ "type": "command", "command": "bash /path/to/wezterm-agent-dashboard/hook.sh claude user-prompt-submit" }
```

## Development

```sh
# Run tests
cargo test

# Run with debug logging
RUST_LOG=debug cargo run
```

## Contributing

Contributions are welcome! Please open an issue or submit a pull request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.
