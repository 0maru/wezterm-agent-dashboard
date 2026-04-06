#!/usr/bin/env bash
# Thin wrapper: delegates to the Rust binary.
# Called by Claude Code / Codex hooks (settings.json / hooks.json).
#
# Usage in Claude Code hooks:
#   bash /path/to/wezterm-agent-dashboard/hook.sh claude <event>
#
# The binary reads JSON from stdin (piped by the agent hook system).

PLUGIN_DIR="$(cd "$(dirname "$0")" && pwd -P)"

if command -v wezterm-agent-dashboard &>/dev/null; then
    BIN="wezterm-agent-dashboard"
elif [ -x "$PLUGIN_DIR/target/release/wezterm-agent-dashboard" ]; then
    BIN="$PLUGIN_DIR/target/release/wezterm-agent-dashboard"
elif [ -x "$HOME/.local/bin/wezterm-agent-dashboard" ]; then
    BIN="$HOME/.local/bin/wezterm-agent-dashboard"
else
    # Binary not found — silently exit
    exit 0
fi

exec "$BIN" hook "$@"
