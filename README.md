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
- Dashboard overlay toggled via keybinding
- Support for multiple concurrent agent sessions

## Requirements

- [WezTerm](https://wezfurlong.org/wezterm/) (nightly or v20240101+)
- Rust 1.75+ (for building from source)

## Installation

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

agent_dashboard.apply_to_config(config)

return config
```

## Usage

Once installed, the dashboard status bar appears automatically when an AI agent session is detected.

| Keybinding | Action |
|---|---|
| `Ctrl+Shift+A` | Toggle dashboard overlay |

## Configuration

You can customize the plugin by passing options to `apply_to_config`:

```lua
agent_dashboard.apply_to_config(config, {
  position = "bottom",    -- "top" or "bottom"
  update_interval = 1000, -- refresh interval in ms
})
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
