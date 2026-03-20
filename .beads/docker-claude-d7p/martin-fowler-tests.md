# Martin Fowler Test Plan: fastest-claude-dock (Arch-Native / Bun)

## Happy Path Tests
- `test_maps_host_project_to_container_app_directory`
  - Given: A valid project directory.
  - When: `new_container_args` is called.
  - Then: The host project directory is mapped to the internal `/app` directory.
- `test_maps_host_config_files_to_container_home`
  - Given: A host home directory with `.claude`, `.gitconfig`, and `.jj`.
  - When: `new_container_args` is called.
  - Then: Each host config file/directory is projected to `/home/user/` inside the container.
- `test_appends_user_arguments_to_claude_process`
  - Given: User-provided arguments `["--verbose", "--print"]`.
  - When: `new_container_args` is called.
  - Then: These arguments are correctly positioned as inputs to the Claude process.
- `test_container_runs_with_host_user_identity`
  - Given: Host UID and GID retrieved via `id -u` and `id -g`.
  - When: `new_container_args` is called.
  - Then: `CONTAINER_USER_ID` and `CONTAINER_GROUP_ID` environment variables are correctly set.
- `test_arch_linux_docker_image_builds_successfully`
  - Given: The local `Dockerfile` with `archlinux:latest` base.
  - When: `docker build .` is executed.
  - Then: The build completes successfully with exit code 0.
- `test_bun_performance_symlink_exists`
  - Given: A running container.
  - When: `node --version` is executed.
  - Then: The version string contains "bun" or indicates the Bun runtime.

## Error Path Tests
- `test_returns_error_when_docker_missing`
  - Given: `docker` binary is not in `PATH`.
  - When: `cmd_run` is executed.
  - Then: Returns `Err(Error::DockerNotFound)`.
- `test_returns_error_when_docker_daemon_unavailable`
  - Given: Docker daemon is stopped.
  - When: `cmd_run` is executed.
  - Then: Returns `Err(Error::DockerDaemonUnavailable)`.
- `test_returns_error_when_image_build_fails`
  - Given: A `Dockerfile` with syntax errors.
  - When: `docker build` is executed.
  - Then: Returns `Err(Error::ImageBuildFailed)`.
- `test_returns_error_when_id_command_fails`
  - Given: The `id` command is not available or returns an error.
  - When: `cmd_run` is executed.
  - Then: Returns `Err(Error::IdCommandFailed)`.

## Edge Case Tests
- `test_handles_empty_extra_claude_args`
  - Given: An empty list of extra arguments.
  - When: `new_container_args` is called.
  - Then: The command is constructed without trailing arguments beyond the project target.
- `test_sanitises_unusual_project_folder_names`
  - Given: Project folder named `...///***`.
  - When: `sanitise_name` is called.
  - Then: Returns `claude-project`.

## Contract Verification Tests
- `test_precondition_config_mounts_to_claude_home`
  - Given: Host home directory path.
  - When: `new_container_args` is called.
  - Then: Verifies that `.claude`, `.gitconfig`, and `.jj` are projected to `/home/user/`.
- `test_postcondition_interactive_prompt_speed_under_100ms`
  - Given: A warm Docker image cache.
  - When: The tool is executed and time-to-first-prompt is measured.
  - Then: The elapsed time to reach an interactive Claude prompt is < 100ms.

## Property-Based Tests
- `proptest_full_command_vector_construction`
  - Given: Arbitrary host paths, valid UID/GID values, and lists of extra arguments.
  - When: `new_container_args` is called.
  - Then: Validates volume mapping, UID/GID mapping, and argument ordering across arbitrary input permutations.

## Given-When-Then Scenarios (Outcome-Focused DSL)
### Scenario 1: Launching a new Claude session on Arch Linux
Given: An Arch Linux host with Docker installed.
When: I run the Claude dock tool in a project directory.
Then: 
- A Claude session starts with native performance using the Arch base image.
- The session uses Bun as the runtime for 2-4x faster execution.
- Claude has access to `ripgrep`, `fzf`, and `bat` for optimized operations.
- Files created by Claude are owned by the host user (UID/GID mapping).
- The session is interactive within 100 milliseconds.

### Scenario 2: Security and Privilege Dropping
Given: A running container.
When: I check the user inside the session.
Then:
- The process is running as a non-privileged `claudeuser`.
- The user's UID and GID inside the container match the host user's UID and GID.
- Root access is only used temporarily for session initialization.
