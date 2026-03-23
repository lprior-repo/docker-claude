# claude-dock

`claude-dock` runs Claude Code inside Docker with saved provider profiles.

It gives you a simple flow:

- save API keys in your system keyring
- switch between profiles
- launch Claude Code in a container from your current project

## What It Does

When you run `claude-dock`, it:

- mounts your current project into the container at `/app`
- mounts your Claude config and session files (read-only where possible)
- forwards git identity via SSH agent forwarding
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

Run with port forwarding (e.g., a dev server):

```bash
claude-dock run -P 3000:3000 -P 8080:80
```

Run a one-shot prompt:

```bash
claude-dock run -- -p "Explain this repository"
```

Open a shell in the container:

```bash
claude-dock shell
```

With GPU passthrough:

```bash
claude-dock run --gpus
```

Backdoor-safe mode (no persistent build caches):

```bash
claude-dock run --no-cache
```

## Key Commands

### Profiles

```bash
claude-dock key list
claude-dock key use <name>
claude-dock key remove <name>
claude-dock config
```

### Container Management

```bash
claude-dock ps                  # List all claude-dock containers
claude-dock stop [name]         # Stop a running container
claude-dock clean               # Remove all containers and prune stale volumes
```

### `run` and `shell` Flags

| Flag | Description |
|------|-------------|
| `-P, --port PORT` | Forward ports (e.g., `-P 3000:3000 -P 8080:80`) |
| `-m, --mount MOUNT` | Mount extra directories (e.g., `-m /other/repo:/opt/other:ro`) |
| `-M, --memory SIZE` | Memory limit (default: `8g`) |
| `--cpus N` | CPU limit |
| `--gpus` | GPU passthrough |
| `--host-access` | Allow container to reach host network (host.docker.internal) |
| `--no-cache` | Use tmpfs for build dirs instead of persistent volumes |

## Mounts

**Project & config (bind mounts):**

- project directory → `/app` (only writable persistent path)
- `~/.claude` → `/home/user/.claude:ro` (with tmpfs overlay for claude's own writes)
- `~/.claude.json` → `/home/user/.claude.json:ro`
- `~/.gitconfig` → `/home/user/.gitconfig:ro`
- `~/.jj` → `/home/user/.jj:ro`
- `~/.zshrc` → `/home/user/.zshrc:ro` (auto-detected)
- `~/.zshenv` → `/home/user/.zshenv:ro` (auto-detected)
- `~/.config/starship.toml` → `/home/user/.config/starship.toml:ro` (auto-detected)

**Git auth (agent forwarding):**

- `SSH_AUTH_SOCK` → `/tmp/ssh-agent.sock`
- `~/.ssh/known_hosts` → `/home/user/.ssh/known_hosts:ro`
- `~/.ssh/config` → sanitized copy (IdentityFile stripped)
- `GPG_AGENT_SOCK` → `/tmp/gpg-agent.sock`

**Shell:**

- Default shell: zsh
- PATH includes `~/.local/bin`, `~/.cargo/bin`

**Build caches (named volumes by default, tmpfs with `--no-cache`):**

- `/app/target`
- `/root/.cargo/registry`
- `/root/.cargo/git`
- `/root/.rustup`

**tmpfs (ephemeral, no persistence):**

- `/tmp` (500m), `/run` (10m)
- `~/.local` (500m), `~/.gnupg` (10m), `~/.config` (10m), `~/.cache` (500m)
- `~/.claude` (50m)

## Security Notes

- API keys stored in system keyring, not on disk
- Root filesystem is `--read-only`
- All Linux capabilities dropped (`--cap-drop ALL`), `no-new-privileges: true`
- Container runs as host user via `-u uid:gid` (not root + gosu)
- Memory capped at 8g (swap=memory, no host swap thrashing), PID limit 512
- SSH private keys never mounted — authentication uses agent forwarding
- SSH config `IdentityFile` directives stripped before mounting
- `.git-credentials` is not mounted
- Config files mounted read-only; `.claude` has tmpfs overlay for claude's writes
- `--dangerously-skip-permissions` is not used — approval prompts are active
- `host.docker.internal` off by default (opt-in via `--host-access`)
- Extra mount validation rejects `/`, `/root`, `/etc`, `/var`, `/usr`, `/proc`, `/dev`, `/sys`, `/bin`, `/lib`
- Port validation rejects privileged ports (1-1023)
- `--no-cache` mode uses tmpfs for build artifacts (no persistence)
- Multi-stage Dockerfile (build tools not in final image)
- `.dockerignore` prevents leaking sensitive files into image
- API tokens set via `Command::env()` avoid `/proc/PID/cmdline` but remain visible via `docker inspect` and `/proc/PID/environ` — inherent Docker limitation

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
