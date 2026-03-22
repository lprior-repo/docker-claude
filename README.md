# claude-dock

`claude-dock` runs Claude Code inside Docker with saved provider profiles.

It gives you a simple flow:

- save API keys in your system keyring
- switch between profiles
- launch Claude Code in a container from your current project

## What It Does

When you run `claude-dock`, it:

- mounts your current project into the container at `/app`
- mounts your Claude config and session files
- forwards your git identity when available
- injects provider-specific environment variables at runtime
- launches Claude Code inside Docker or Podman

## Supported Providers

### `anthropic`

Standard Claude setup. No provider env vars are injected by `claude-dock`.

### `minimax`

Uses the MiniMax Anthropic-compatible endpoint with these settings:

- `ANTHROPIC_BASE_URL=https://api.minimax.io/anthropic`
- `ANTHROPIC_AUTH_TOKEN`
- `ANTHROPIC_MODEL=MiniMax-M2.7-highspeed`
- `ANTHROPIC_SMALL_FAST_MODEL=MiniMax-M2.7-highspeed`
- `ANTHROPIC_DEFAULT_SONNET_MODEL=MiniMax-M2.7-highspeed`
- `ANTHROPIC_DEFAULT_OPUS_MODEL=MiniMax-M2.7-highspeed`
- `ANTHROPIC_DEFAULT_HAIKU_MODEL=MiniMax-M2.7-highspeed`
- `API_TIMEOUT_MS=3000000`
- `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`

### `zai`

Uses the Z.ai Anthropic-compatible endpoint with these settings:

- `ANTHROPIC_BASE_URL=https://api.z.ai/api/anthropic`
- `ANTHROPIC_AUTH_TOKEN`
- `ANTHROPIC_DEFAULT_OPUS_MODEL=GLM-5-Turbo`
- `API_TIMEOUT_MS=3000000`
- `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`

## Install

Build the binary:

```bash
cargo build --release
```

Install it locally:

```bash
mkdir -p ~/.local/bin
cp -f target/release/claude-dock ~/.local/bin/claude-dock
```

If `~/.local/bin` is on your `PATH`, you can run `claude-dock` directly.

## Quick Start

Add a profile:

```bash
claude-dock key add work -k <your-key> -p anthropic
claude-dock key add minimax25 -k <your-key> -p minimax
claude-dock key add glm2 -k <your-key> -p zai
```

Set the active profile:

```bash
claude-dock key use minimax25
```

Run Claude Code:

```bash
claude-dock run
```

Run a one-shot prompt:

```bash
claude-dock run -- -p "Explain this repository"
```

Open a shell in the container:

```bash
claude-dock shell
```

## Key Commands

List profiles:

```bash
claude-dock key list
```

Use a profile:

```bash
claude-dock key use <name>
```

Remove a profile:

```bash
claude-dock key remove <name>
```

Show current config:

```bash
claude-dock config
```

## Mounts

The container mounts:

- project directory -> `/app`
- `~/.claude` -> `/home/user/.claude`
- `~/.claude.json` -> `/home/user/.claude.json`
- `~/.gitconfig` -> `/home/user/.gitconfig:ro`
- `~/.git-credentials` -> `/home/user/.git-credentials:ro`
- `~/.jj` -> `/home/user/.jj`

## Security Notes

- keys are stored in your system keyring
- keys are not committed to the repo
- keys are injected into the container at runtime
- runtime env vars can still be visible through Docker inspection paths

## Dev Notes

Main files:

- `src/main.rs`
- `src/container.rs`
- `src/keyring.rs`
- `src/provider.rs`
- `src/entrypoint.rs`

Local tasks:

```bash
just install
just test
just lint
just fmt
just check
```

Run tests:

```bash
cargo test
```
