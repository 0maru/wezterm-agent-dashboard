# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

wezterm-agent-dashboard is a Rust project (not yet initialized with Cargo). Currently contains only skeleton files — no source code, build configuration, or tests exist yet.

## Build Commands

No build infrastructure is set up yet. Once a `Cargo.toml` is created:

- `cargo build` — compile the project
- `cargo run` — build and run
- `cargo test` — run all tests
- `cargo test <test_name>` — run a single test
- `cargo clippy` — lint
- `cargo fmt` — format code
- `cargo fmt -- --check` — check formatting without modifying files
