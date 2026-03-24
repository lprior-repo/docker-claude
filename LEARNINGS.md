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

**Root Cause**: Through `strace`, we discovered Claude tries to write to or lock `.claude.json` on startup. Because `.claude.json` was mounted with `:ro` (read-only) and the root filesystem is also `--read-only`, Claude would get `EACCES` or `EROFS` when trying to manage its configuration, causing it to fall into an infinite retry loop or lockup.

**Fix**: Changed the bind mount in `src/container.rs` to mount `.claude.json` read/write (removed `:ro`).

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
