# Contract Specification: fastest-claude-dock (Arch-Native / Bun)

## Context
- **Feature**: Optimize `docker-claude` for Arch Linux using an `archlinux` base image and `bun`.
- **Domain terms**:
    - **Container User**: A non-root user named `claudeuser`.
    - **Base Image**: `archlinux:latest`.
    - **Claude Binary**: `@anthropic-ai/claude-code` installed via `bun`.
- **Assumptions**:
    - Native Linux performance (no Docker Desktop virtualization).
    - Host user UID/GID mapping is critical for strict Arch Linux permissions.
    - `bun` is symlinked to `node` for maximum execution speed.
- **Open questions**:
    - None.

## Preconditions
### Dockerfile
- [ ] Base image MUST be `archlinux:latest`.
- [ ] Packages `bun`, `git`, `ripgrep`, `fzf`, `bat`, `gosu`, `unzip` MUST be installed via `pacman`.
- [ ] `bun` MUST be symlinked to `/usr/bin/node`.
- [ ] `claude-code` MUST be installed globally via `bun install -g`.
- [ ] A non-root user `claudeuser` MUST be created dynamically in the entrypoint based on host UID/GID.
- [ ] `ENTRYPOINT` MUST be `["/entrypoint.sh"]`.
- [ ] Directory `/app` MUST exist and be accessible by `claudeuser`.

### Rust CLI (`src/main.rs`)
- [ ] `DEFAULT_IMAGE` MUST be `claude-dock:latest`.
- [ ] `new_container_args` MUST retrieve host UID and GID via `id -u` and `id -g`.
- [ ] Volume mounts MUST use `/home/user/` as the target home directory for consistency with the entrypoint script.
- [ ] MUST pass `CONTAINER_USER_ID` and `CONTAINER_GROUP_ID` as environment variables.

## Postconditions
### Container Execution
- [ ] Running the container results in a Claude session for the current directory.
- [ ] Host configuration files (`.claude`, `.gitconfig`, `.jj`) are correctly projected into the container's `/home/user/` directory.
- [ ] **Startup Speed**: The container MUST reach the **interactive Claude prompt** in < 100ms.
- [ ] **File Ownership**: Files created by Claude in `/app` MUST be owned by the host user (UID/GID mapping).
- [ ] **Native Tools**: Claude MUST have access to `ripgrep`, `fzf`, and `bat` for optimized operations.

## Invariants
- [ ] The `claude-dock` binary always uses `docker` as the backend.
- [ ] `claude-dock run` always maps the host's current working directory to the container's `/app`.
- [ ] Container user is always non-root and matches the host user's UID/GID.

## Error Taxonomy
- `Error::DockerNotFound` - when `docker` binary is not in PATH.
- `Error::DockerDaemonUnavailable` - when `docker` fails to connect.
- `Error::IdCommandFailed` - when `id -u` or `id -g` fails.
- `Error::ImageBuildFailed` - when `docker build` fails.

## Contract Signatures
### Rust CLI
- `fn new_container_args(image: &str, api_key: &str, project_dir: &str, cname: &str, home: &str, uid: &str, gid: &str, extra_claude_args: &[String]) -> Vec<String>`
    - Returns the full `docker run` command vector.
    - Postcondition: Host project directory is mapped to `/app`.
    - Postcondition: Host home configs are mapped to `/home/user/`.
    - Postcondition: `CONTAINER_USER_ID` and `CONTAINER_GROUP_ID` are set.

## Violation Examples
- VIOLATES <UidGidMapping>: `new_container_args` uses process ID instead of actual user UID.
- VIOLATES <ArchNative>: `Dockerfile` uses `debian` or `alpine` instead of `archlinux`.
- VIOLATES <BunSpeed>: `Dockerfile` does not symlink `bun` to `node`.
