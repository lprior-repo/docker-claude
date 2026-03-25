# docker-claude Learnings

## 2026-03-24: Container Startup Fixes

### Problems Fixed

#### 1. Args Passing Bug (Profile Shortcuts)
When using profile shortcut syntax like `claude-dock glm1 -- claude --version`, args after `--` were being lost because `claude_args` was hardcoded to `&[]` in `main()` when using profile shortcuts.

**Fix**: Extract args from `std::env::args()` after the `--` separator.

**File**: `src/main.rs`

#### 2. TTY Issue (Non-Interactive Mode)
The `-it` flag caused "the input device is not a TTY" errors when running non-interactive commands like `claude --version`.

**Fix**: Detect non-interactive flags (`--version`, `-v`, `--help`, `-h`, `-p`, `--print`) and use `-i` instead of `-it`.

**File**: `src/container.rs`

#### 3. su/sudo Blocked by Container Security
Container security restrictions (`--cap-drop ALL`, `--security-opt no-new-privileges`) prevented `su` and `sudo` from working, causing `su: cannot set groups: Operation not permitted`.

**Fix**: Removed user switching entirely. The entrypoint now execs the command directly (as root, which is fine for a disposable container).

**File**: `src/entrypoint.rs`

#### 4. Wrapper Script Creation Fails on Read-Only FS
The `/home/user/.local/bin/claude` wrapper script couldn't be created because the filesystem is read-only.

**Fix**: Use `/usr/local/bin/claude` directly instead of a wrapper script.

**File**: `src/entrypoint.rs`

#### 5. Chown Errors on Read-Only Mounts
Thousands of `chown: changing ownership of '/home/user/...': Operation not permitted` errors appeared because:
- tmpfs mounts show `rw` in `findmnt` but `CAP_CHOWN` is dropped
- bind mounts from host are read-only inside container

**Fix**: Removed all chown calls from `setup_system_user()` since we no longer switch users.

**File**: `src/entrypoint.rs`

#### 6. Claude Code Hangs in Interactive Mode (The `.claude.json` Issue)
When running `claude-dock glm1` interactively, Claude Code would hang indefinitely without showing the prompt.

**Root Cause**: Claude uses `write-file-atomic` to update `~/.claude.json`, which works by writing to `~/.claude.json.tmp...` and then `rename()`-ing it over the original file. Since the container runs with a `--read-only` root filesystem (meaning `/home/user` is read-only), creating the tmp file in `~` fails with `EROFS`.
Even if we mounted a file directly (`-v ~/.claude.json:/home/user/.claude.json`), Docker prevents `rename()` over bind-mounted files with `EBUSY`.

**Fix**:
1. Added a migration in `src/container.rs` that runs on the host before launching the container. It moves `~/.claude.json` to `~/.claude/.claude.json` and creates a symlink in its place (`~/.claude.json -> .claude/.claude.json`).
2. Added `-e CLAUDE_CONFIG_DIR=/home/user/.claude` to the container env vars.
3. Removed the file-level bind mount for `.claude.json`.
Now, Claude inside the container writes atomic config updates natively into the bind-mounted directory `/home/user/.claude`, bypassing the read-only root FS and avoiding `EBUSY`. Because the host has a symlink, native `claude` executions share the exact same config file.

#### 7. Claude Hangs Mid-Sentence During Large Output
When generating large blocks of text (like markdown tables), Claude would completely freeze mid-sentence.

**Root Cause**: Docker Bridge network MTU mismatch. When a proxy API (like Z.ai or Minimax) sends a large burst of data, the packet size can exceed the Docker virtual bridge's MTU limit (often 1500, but lower if using a VPN). The packets are silently dropped, and Claude's Node `fetch` client waits forever because the TCP connection isn't cleanly closed.

**Fix**: Switched to using `--network host` mode in `src/container.rs` (`--network host`). This attaches the container directly to the Linux host's network interface, eliminating Docker's virtual bridge and preventing MTU packet drops.

**File**: `src/container.rs`

### Security Model Change

**Before**: Container exec'd as `claudeuser` via gosu for "least privilege"

**After**: Container runs as root, execs directly with `setsid()`. This is acceptable because:
- Container is disposable (`--rm`)
- Root filesystem is read-only
- All capabilities are dropped
- No new privileges allowed
- Network access is restricted

### Files Modified

- `src/main.rs` - Profile shortcut args extraction
- `src/container.rs` - Non-interactive mode detection, TTY flag handling, removed `:ro` on `.claude.json`
- `src/entrypoint.rs` - Removed user switching, removed chown calls, added setsid, fixed shell arg forwarding
- `Cargo.toml` - Added `libc` dependency
