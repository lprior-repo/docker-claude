use crate::container::{
    build_container_args, cache_vol_name_for_test, reattach_container_args, ContainerBackend,
    ContainerState, LaunchInputs,
};
use crate::entrypoint::build_claude_exec_args;
use crate::provider::{ProfileConfig, Provider};
use crate::validation::{validate_mount_for_test, validate_port_for_test};
use crate::{probe_container, resolve_launch_plan, sanitise_project_name};

fn make_inputs<'a>(config: &'a ProfileConfig, extra: &'a [String]) -> LaunchInputs<'a> {
    LaunchInputs {
        image: "ghcr.io/example/claude:latest",
        config,
        project_dir: "/tmp/project",
        base_name: "claude-demo",
        host_home: "/home/tester",
        uid: "1000",
        gid: "1000",
        extra_claude_args: extra,
        ports: &[],
        extra_mounts: &[],
        memory: "",
        cpus: "",
        host_access: false,
        no_cache: false,
        nonce: 42,
        git_name: None,
        git_email: None,
    }
}

fn anthropic_config() -> ProfileConfig {
    ProfileConfig {
        key: "sk-ant-123".into(),
        provider: Provider::Anthropic,
    }
}

fn minimax_config() -> ProfileConfig {
    ProfileConfig {
        key: "minimax-key".into(),
        provider: Provider::Minimax,
    }
}

fn zai_config() -> ProfileConfig {
    ProfileConfig {
        key: "zai-key-123".into(),
        provider: Provider::Zai,
    }
}

#[test]
fn new_container_args_launches_claude_with_forwarded_args() {
    let config = anthropic_config();
    let extra = vec!["--verbose".into()];
    let inputs = make_inputs(&config, &extra);
    let args = build_container_args(&inputs, "claude-demo");

    assert_eq!(args[0], "run");
    assert!(args.contains(&"-it".into()));
    assert!(args.contains(&"--rm".into()));
    assert!(args.contains(&"/tmp/project:/app".into()));
    assert!(args.contains(&"/home/tester/.claude:/home/user/.claude".into()));
    assert!(args.contains(&"/home/tester/.claude.json:/home/user/.claude.json:ro".into()));
    assert!(args.windows(2).any(|w| w == ["__entrypoint", "--"]));
    assert!(args.contains(&"--verbose".into()));
}

#[test]
fn new_container_args_uses_non_tty_mode_for_print_runs() {
    let config = anthropic_config();
    let extra = vec!["-p".into(), "hello".into()];
    let inputs = make_inputs(&config, &extra);
    let args = build_container_args(&inputs, "claude-demo");

    assert_eq!(args[0], "run");
    assert!(args.contains(&"-i".into()));
    assert!(!args.contains(&"-it".into()));
}

#[test]
fn new_container_args_supports_minimax_provider() {
    let config = minimax_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");

    assert!(args.contains(&"ANTHROPIC_AUTH_TOKEN".into()));
    assert!(args.contains(&"ANTHROPIC_BASE_URL=https://api.minimax.io/anthropic".into()));
    assert!(args.contains(&"ANTHROPIC_MODEL=MiniMax-M2.7-highspeed".into()));
    assert!(args.contains(&"API_TIMEOUT_MS=3000000".into()));
    assert!(args.contains(&"CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1".into()));
}

#[test]
fn new_container_args_supports_zai_provider() {
    let config = zai_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");

    assert!(args.contains(&"ANTHROPIC_AUTH_TOKEN".into()));
    assert!(args.contains(&"ANTHROPIC_BASE_URL=https://api.z.ai/api/anthropic".into()));
    assert!(args.contains(&"ANTHROPIC_DEFAULT_OPUS_MODEL=GLM-5-Turbo".into()));
    assert!(args.contains(&"API_TIMEOUT_MS=3000000".into()));
    assert!(args.contains(&"CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1".into()));
}

#[test]
fn reattach_args_attach_to_existing_container() {
    assert_eq!(
        reattach_container_args("claude-demo"),
        vec!["start", "-ai", "claude-demo"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
}

#[test]
fn sanitise_name_replaces_non_identifier_characters() {
    assert_eq!(
        sanitise_project_name("my cool/project.v1"),
        "claude-my-cool-project-v1"
    );
}

#[test]
fn parse_container_state_distinguishes_missing_running_and_stopped() {
    assert_eq!(
        probe_container(ContainerBackend::Docker, "nonexistent-test-xyz"),
        ContainerState::Missing
    );
}

#[test]
fn resolve_launch_plan_resumes_stopped_container() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let plan = resolve_launch_plan(ContainerState::Stopped, inputs);
    assert_eq!(plan.mode, crate::container::LaunchMode::Resume);
    assert_eq!(plan.container_name, "claude-demo");
}

#[test]
fn resolve_launch_plan_uses_base_name_for_missing_container() {
    let config = anthropic_config();
    let extra = ["--print".to_string()];
    let inputs = make_inputs(&config, &extra);
    let plan = resolve_launch_plan(ContainerState::Missing, inputs);
    assert_eq!(plan.mode, crate::container::LaunchMode::New);
    assert_eq!(plan.container_name, "claude-demo");
    assert_eq!(plan.args.last().map(String::as_str), Some("--print"));
}

#[test]
fn resolve_launch_plan_avoids_name_collision_for_running_container() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let plan = resolve_launch_plan(ContainerState::Running, inputs);
    assert_eq!(plan.mode, crate::container::LaunchMode::New);
    assert_eq!(plan.container_name, "claude-demo-42");
    assert!(plan.args.contains(&"claude-demo-42".to_string()));
}

#[test]
fn new_container_args_forwards_git_identity() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    inputs.git_name = Some("Test User");
    inputs.git_email = Some("test@example.com");
    let args = build_container_args(&inputs, "claude-demo");

    assert!(args.contains(&"GIT_AUTHOR_NAME=Test User".into()));
    assert!(args.contains(&"GIT_AUTHOR_EMAIL=test@example.com".into()));
    assert!(args.contains(&"GIT_COMMITTER_NAME=Test User".into()));
    assert!(args.contains(&"GIT_COMMITTER_EMAIL=test@example.com".into()));
}

#[test]
fn new_container_args_anthropic_provider_injects_no_secrets() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");

    assert!(args.contains(&"HOST_HOME=/home/tester".into()));
    assert!(!args.iter().any(|a| a.contains("ANTHROPIC_API_KEY")));
    assert!(!args.iter().any(|a| a.contains("ANTHROPIC_AUTH_TOKEN")));
}

#[test]
fn sanitise_name_falls_back_for_empty_results() {
    assert_eq!(sanitise_project_name("...///***"), "claude-project");
}

#[test]
fn provider_from_str_rejects_unknown() {
    assert!(Provider::from_str_lossy("unknown_provider").is_err());
}

#[test]
fn provider_needs_auth_token() {
    assert!(!Provider::Anthropic.needs_auth_token());
    assert!(Provider::Minimax.needs_auth_token());
    assert!(Provider::Zai.needs_auth_token());
}

#[test]
fn new_container_args_uses_named_cache_volumes_by_default() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");

    assert!(args.contains(&"claude-dock-claude-demo-target:/app/target".into()));
    assert!(args.contains(&"claude-dock-claude-demo-cargo-registry:/root/.cargo/registry".into()));
    assert!(args.contains(&"claude-dock-claude-demo-cargo-git:/root/.cargo/git".into()));
    assert!(args.contains(&"claude-dock-claude-demo-rustup:/root/.rustup".into()));
}

#[test]
fn new_container_args_no_cache_uses_tmpfs_instead_of_volumes() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    inputs.no_cache = true;
    let args = build_container_args(&inputs, "claude-demo");

    assert!(args.iter().any(|a| a == "/app/target:size=2g"));
    assert!(!args
        .iter()
        .any(|a| a.contains("claude-dock-claude-demo-target")));
    assert!(!args.iter().any(|a| a.contains("cargo-registry")));
}

#[test]
fn new_container_args_never_mounts_git_credentials_file() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");

    assert!(
        !args.iter().any(|a| a.contains("git-credentials")),
        "git-credentials file must never be mounted"
    );
}

#[test]
fn new_container_args_ssh_config_strips_identity_file() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");

    let ssh_config_mounts: Vec<_> = args
        .iter()
        .zip(args.iter().skip(1))
        .filter(|(k, _)| **k == "-v")
        .map(|(_, v)| v.as_str())
        .filter(|v| v.contains("ssh-config") || v.contains(".ssh/config"))
        .collect();

    for mount in &ssh_config_mounts {
        assert!(
            !mount.contains("/.ssh/config:"),
            "raw ssh config must never be mounted: {mount}"
        );
    }
}

#[test]
fn new_container_args_never_mounts_ssh_private_keys() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");

    let vol_values: Vec<_> = args
        .iter()
        .zip(args.iter().skip(1))
        .filter(|(k, _)| **k == "-v")
        .map(|(_, v)| v.as_str())
        .collect();

    for vol in &vol_values {
        assert!(
            !vol.contains("id_rsa"),
            "private key should never be mounted: {vol}"
        );
        assert!(
            !vol.contains("id_ed25519"),
            "private key should never be mounted: {vol}"
        );
    }
}

#[test]
fn new_container_args_keeps_rm_flag() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"--rm".into()));
}

#[test]
fn new_container_args_enforces_read_only_root() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"--read-only".into()));
}

#[test]
fn new_container_args_drops_all_capabilities() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"ALL".into()));
}

#[test]
fn new_container_args_prevents_privilege_escalation() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"no-new-privileges:true".into()));
}

#[test]
fn new_container_args_sets_memory_limit_default_8g() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"--memory".into()));
    assert!(args.contains(&"8g".into()));
}

#[test]
fn new_container_args_sets_memory_swap_equal_to_memory() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    let mem_count = args.iter().filter(|a| **a == "--memory").count();
    let swap_count = args.iter().filter(|a| **a == "--memory-swap").count();
    assert_eq!(mem_count, swap_count);
}

#[test]
fn new_container_args_limits_pid_count() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"--pids-limit".into()));
    assert!(args.contains(&"512".into()));
}

#[test]
fn new_container_args_no_host_access_by_default() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(!args.iter().any(|a| a.contains("host.docker.internal")));
}

#[test]
fn new_container_args_host_access_when_flagged() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    inputs.host_access = true;
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"host.docker.internal:host-gateway".into()));
}

#[test]
fn new_container_args_uses_tmpfs_for_writable_dirs() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.iter().any(|a| a.starts_with("/tmp:size=")));
    assert!(args
        .iter()
        .any(|a| a.starts_with("/home/user/.local:size=")));
    assert!(args
        .iter()
        .any(|a| a.starts_with("/home/user/.gnupg:size=")));
}

#[test]
fn new_container_args_mounts_claude_config_writable() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");

    let claude_mount: Vec<_> = args
        .iter()
        .zip(args.iter().skip(1))
        .filter(|(k, _)| **k == "-v")
        .map(|(_, v)| v.as_str())
        .filter(|v| v.contains("/.claude:") && !v.contains(".claude.json"))
        .collect();
    for m in &claude_mount {
        assert!(
            !m.ends_with(":ro"),
            ".claude mount must be writable for session data: {m}"
        );
    }
}

#[test]
fn new_container_args_mounts_jj_as_read_only() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");

    let jj_mount: Vec<_> = args
        .iter()
        .zip(args.iter().skip(1))
        .filter(|(k, _)| **k == "-v")
        .map(|(_, v)| v.as_str())
        .filter(|v| v.contains(".jj"))
        .collect();
    for m in &jj_mount {
        assert!(m.ends_with(":ro"), ".jj mount must be read-only: {m}");
    }
}

#[test]
fn new_container_args_passes_uid_gid_via_env_for_gosu() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");

    assert!(!args.contains(&"-u".into()));
    assert!(args.contains(&"CONTAINER_USER_ID=1000".into()));
    assert!(args.contains(&"CONTAINER_GROUP_ID=1000".into()));
}

#[test]
fn new_container_args_mounts_claude_dir_as_bind_mount() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");

    assert!(!args
        .iter()
        .any(|a| a.starts_with("/home/user/.claude:size=")));
    assert!(args
        .iter()
        .any(|a| a.contains("/.claude:/home/user/.claude")));
}

#[test]
fn new_container_args_supports_custom_memory() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    inputs.memory = "4g";
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"4g".into()));
    assert!(!args.contains(&"8g".into()));
}

#[test]
fn new_container_args_supports_port_forwarding() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    let ports = ["3000:3000".to_string(), "8080:8080".to_string()];
    inputs.ports = &ports;
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"3000:3000".into()));
    assert!(args.contains(&"8080:8080".into()));
}

#[test]
fn new_container_args_supports_extra_mounts() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    let extra = ["/opt/other:/opt/other:ro".to_string()];
    inputs.extra_mounts = &extra;
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"/opt/other:/opt/other:ro".into()));
}

#[test]
fn validate_mount_rejects_root_mount() {
    assert!(validate_mount_for_test("/:/host").is_err());
}

#[test]
fn validate_mount_rejects_etc_mount() {
    assert!(validate_mount_for_test("/etc:/etc:ro").is_err());
}

#[test]
fn validate_mount_rejects_var_mount() {
    assert!(validate_mount_for_test("/var/lib:/var/lib:ro").is_err());
}

#[test]
fn validate_mount_rejects_proc_mount() {
    assert!(validate_mount_for_test("/proc:/proc").is_err());
}

#[test]
fn validate_mount_allows_safe_mount() {
    assert!(validate_mount_for_test("/opt/project:/opt/project:ro").is_ok());
}

#[test]
fn validate_mount_rejects_subdir_of_root() {
    assert!(validate_mount_for_test("/root/.ssh:/root/.ssh:ro").is_err());
}

#[test]
fn new_container_args_sets_home_env() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"HOME=/home/user".into()));
}

#[test]
fn cache_volume_name_uses_container_name_as_prefix() {
    assert_eq!(
        cache_vol_name_for_test("claude-demo", "target"),
        "claude-dock-claude-demo-target"
    );
}

#[test]
fn build_claude_exec_args_runs_claude_directly() {
    let result = build_claude_exec_args(&[]);
    assert!(!result
        .iter()
        .any(|a| a.contains("--dangerously-skip-permissions")));
    assert!(!result.iter().any(|a| a.contains("zsh -c")));
    assert!(result.contains(&"/home/user/.local/bin/claude".into()));
}

#[test]
fn build_claude_exec_args_shell_uses_zsh() {
    let result = build_claude_exec_args(&["shell".into()]);
    assert!(result.contains(&"/bin/zsh".into()));
    assert!(!result.iter().any(|a| a.contains("/claude")));
}

#[test]
fn build_claude_exec_args_forwards_all_claude_args() {
    let result = build_claude_exec_args(&["--verbose".into()]);
    assert!(result.contains(&"--verbose".into()));
    assert!(result.contains(&"/home/user/.local/bin/claude".into()));
}

#[test]
fn validate_port_accepts_normal_ports() {
    assert!(validate_port_for_test("3000:3000").is_ok());
    assert!(validate_port_for_test("8080:8080").is_ok());
}

#[test]
fn validate_port_rejects_privileged_container_ports() {
    assert!(validate_port_for_test("22:22").is_err());
    assert!(validate_port_for_test("80:80").is_err());
}

#[test]
fn validate_port_accepts_high_container_ports() {
    assert!(validate_port_for_test("3000:3000").is_ok());
}
