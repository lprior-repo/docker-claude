# claude-dock

`claude-dock` is a tiny Rust launcher that lets Claude Code step into Docker like she owns the room, touch up your codebase, and leave without dragging your whole host machine into the drama.

Think of it as a perfectly packed glam bag for terminal AI:

- one command to launch Claude Code in a container
- multiple saved API key profiles
- easy switching between providers
- your repo mounted in cleanly
- your Claude state and git identity carried in with style

It is practical, sparkly, and a little bit extra in exactly the right way.

## What This Does

Most people want Claude Code to have three things at the same time:

1. access to the current project
2. access to their Claude config and session state
3. a container boundary so the whole experience feels cleaner, safer, and easier to reproduce

That is the whole fantasy `claude-dock` delivers.

You save one or more provider profiles into your system keyring, pick the one you want, and then run Claude Code inside Docker or Podman. The tool mounts your project into `/app`, mounts your Claude state into the container home, forwards your git identity, and injects the right environment variables for the selected provider.

So instead of manually doing a long `docker run ...` spell every single time, you get a single command that already knows how to walk in wearing heels and carrying the right secrets.

## Why It Exists

Because the raw container command is ugly.

Because copying API keys around by hand is messy.

Because switching between Anthropic, MiniMax, and Z.ai should feel like changing lip gloss, not rebuilding your entire life.

And because sometimes you want Claude Code in a neat little containerized bubble while still keeping your host setup, git identity, Claude history, and workspace flow intact.

## Current Provider Support

`claude-dock` currently knows how to dress Claude for three different moods:

### `anthropic`

This is the vanilla Claude path.

- no provider env vars are injected by `claude-dock`
- you keep the standard Claude experience
- useful if you are already authenticated the usual way

### `minimax`

This profile injects the MiniMax Anthropic-compatible endpoint and model settings:

- `ANTHROPIC_BASE_URL=https://api.minimax.io/anthropic`
- `ANTHROPIC_AUTH_TOKEN` from the saved profile key
- `ANTHROPIC_MODEL=MiniMax-M2.7-highspeed`
- `ANTHROPIC_SMALL_FAST_MODEL=MiniMax-M2.7-highspeed`
- `ANTHROPIC_DEFAULT_SONNET_MODEL=MiniMax-M2.7-highspeed`
- `ANTHROPIC_DEFAULT_OPUS_MODEL=MiniMax-M2.7-highspeed`
- `ANTHROPIC_DEFAULT_HAIKU_MODEL=MiniMax-M2.7-highspeed`
- `API_TIMEOUT_MS=3000000`
- `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`

### `zai`

This profile injects the Z.ai Anthropic-compatible endpoint and model settings:

- `ANTHROPIC_BASE_URL=https://api.z.ai/api/anthropic`
- `ANTHROPIC_AUTH_TOKEN` from the saved profile key
- `ANTHROPIC_DEFAULT_OPUS_MODEL=GLM-5-Turbo`
- `API_TIMEOUT_MS=3000000`
- `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`

## The Vibe Check: How It Works

When you run `claude-dock run`, it does the following:

- detects `docker` or `podman`
- loads the active profile from your system keyring
- reads your current directory and turns it into the project mount
- mounts your project at `/app`
- mounts your local Claude folders and files so Claude feels at home
- forwards your git author and committer identity when available
- launches the container and hands control to Claude Code

It also supports reattaching to an existing stopped session container, which is cute, efficient, and slightly emotionally mature.

## What Gets Mounted Into The Container

The launcher currently mounts these paths:

- your project directory -> `/app`
- `~/.claude` -> `/home/user/.claude`
- `~/.claude.json` -> `/home/user/.claude.json`
- `~/.gitconfig` -> `/home/user/.gitconfig:ro`
- `~/.git-credentials` -> `/home/user/.git-credentials:ro`
- `~/.jj` -> `/home/user/.jj`

That means your Claude history, settings, plugins, and git identity can follow you into the container without you rebuilding the universe every time.

## Security Notes, But Make It Honest

This tool stores profile secrets in your system keyring, not in the repo and not in plaintext config files.

That is the pretty part.

Here is the blunt part:

- secrets are not compiled into the binary
- secrets are not written into this repository
- secrets are passed into the container at runtime
- runtime env vars can still be visible through container inspection mechanisms like `docker inspect`

So yes, this is cleaner than sprinkling keys across shell history and ad hoc scripts. No, it is not magic fairy dust that makes Docker environment variables invisible forever. It is simply a more polished and sensible setup.

## Installation

Build the binary:

```bash
cargo build --release
```

Install it to your local path:

```bash
mkdir -p ~/.local/bin
cp -f target/release/claude-dock ~/.local/bin/claude-dock
```

If `~/.local/bin` is already on your `PATH`, you are ready to twirl.

## Quick Start

### 1. Add a profile

Anthropic:

```bash
claude-dock key add work -k <your-key> -p anthropic
```

MiniMax:

```bash
claude-dock key add minimax25 -k <your-key> -p minimax
```

Z.ai:

```bash
claude-dock key add glm2 -k <your-key> -p zai
```

If you skip `-k`, the binary prompts you securely.

### 2. Pick the active profile

```bash
claude-dock key use minimax25
```

### 3. Run Claude

```bash
claude-dock run
```

### 4. Or run Claude in print mode

```bash
claude-dock run -- -p "Explain this repository in one paragraph"
```

### 5. Or open a shell in the container

```bash
claude-dock shell
```

## Key Management Commands

List saved profiles:

```bash
claude-dock key list
```

Switch active profile:

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

## Example Flows

### Normal Anthropic energy

```bash
claude-dock key add personal -p anthropic
claude-dock key use personal
claude-dock run
```

### Fast MiniMax energy

```bash
claude-dock key add minimax25 -p minimax
claude-dock key use minimax25
claude-dock run -- -p "Reply with exactly: hello"
```

### Z.ai / GLM energy

```bash
claude-dock key add glm2 -p zai
claude-dock key use glm2
claude-dock run -- -p "Reply with exactly: hi"
```

## Developer Notes

This project is written in Rust and currently split into focused modules:

- `src/main.rs` for CLI orchestration
- `src/container.rs` for Docker/Podman launch planning
- `src/keyring.rs` for profile storage and selection
- `src/provider.rs` for provider-specific env rules
- `src/entrypoint.rs` for in-container user setup and exec behavior

The release build is optimized with:

- `lto = true`
- `codegen-units = 1`
- `strip = true`

## Local Tasks

If you like your commands tidy and your repo feeling moisturized:

```bash
just install
just test
just lint
just fmt
just check
```

## Testing

Run the tests:

```bash
cargo test
```

Run linting:

```bash
cargo clippy -- -D warnings
```

## Known Real-World Gotchas

- if your Claude user config contains broken hooks or plugins, Claude itself may fail even when your provider key is valid
- `claude-dock` mounts your local Claude config, which is usually what you want, but it also means your personal setup comes along for the ride
- if a provider expects an Anthropic-compatible API but has strict auth header behavior, the key must be passed exactly right at runtime

In other words: sometimes the problem is not the girl, it is the accessories.

## In One Sentence

`claude-dock` is a polished little Rust launcher for running Claude Code inside Docker with multiple provider profiles, keyring-backed secrets, and enough style to make containerized AI feel less like sysadmin punishment and more like a main-character entrance.
