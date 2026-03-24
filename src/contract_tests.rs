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
        no_env: false,
        gpus: false,
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
fn launch_with_forwarded_args() {
    let config = anthropic_config();
    let extra = vec!["--verbose".into()];
    let inputs = make_inputs(&config, &extra);
    let args = build_container_args(&inputs, "claude-demo");
    assert_eq!(args[0], "run");
    assert!(args.contains(&"-it".into()));
    assert!(args.contains(&"--rm".into()));
    assert!(args.contains(&"/tmp/project:/app".into()));
    assert!(args.windows(2).any(|w| w == ["__entrypoint", "--"]));
    assert!(args.contains(&"--verbose".into()));
}

#[test]
fn print_mode_uses_non_tty() {
    let config = anthropic_config();
    let extra = vec!["-p".into(), "hello".into()];
    let inputs = make_inputs(&config, &extra);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"-i".into()));
    assert!(!args.contains(&"-it".into()));
}

#[test]
fn minimax_provider_injects_env_vars() {
    let config = minimax_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"ANTHROPIC_AUTH_TOKEN".into()));
    assert!(args.contains(&"ANTHROPIC_BASE_URL=https://api.minimax.io/anthropic".into()));
    assert!(args.contains(&"ANTHROPIC_MODEL=MiniMax-M2.7-highspeed".into()));
    assert!(args.contains(&"API_TIMEOUT_MS=3000000".into()));
}

#[test]
fn zai_provider_injects_env_vars() {
    let config = zai_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"ANTHROPIC_AUTH_TOKEN".into()));
    assert!(args.contains(&"ANTHROPIC_BASE_URL=https://api.z.ai/api/anthropic".into()));
    assert!(args.contains(&"ANTHROPIC_DEFAULT_OPUS_MODEL=GLM-5-Turbo".into()));
}

#[test]
fn reattach_attaches_to_existing_container() {
    assert_eq!(
        reattach_container_args("claude-demo"),
        vec!["start", "-ai", "claude-demo"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
}

#[test]
fn sanitise_name() {
    assert_eq!(
        sanitise_project_name("my cool/project.v1"),
        "claude-my-cool-project-v1"
    );
}

#[test]
fn probe_missing_container() {
    assert_eq!(
        probe_container(ContainerBackend::Docker, "nonexistent-test-xyz"),
        ContainerState::Missing
    );
}

#[test]
fn resume_stopped_container() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let plan = resolve_launch_plan(ContainerState::Stopped, inputs);
    assert_eq!(plan.mode, crate::container::LaunchMode::Resume);
    assert_eq!(plan.container_name, "claude-demo");
}

#[test]
fn new_container_for_missing() {
    let config = anthropic_config();
    let extra = ["--print".to_string()];
    let inputs = make_inputs(&config, &extra);
    let plan = resolve_launch_plan(ContainerState::Missing, inputs);
    assert_eq!(plan.mode, crate::container::LaunchMode::New);
    assert_eq!(plan.container_name, "claude-demo");
    assert_eq!(plan.args.last().map(String::as_str), Some("--print"));
}

#[test]
fn suffixed_name_for_running_container() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let plan = resolve_launch_plan(ContainerState::Running, inputs);
    assert_eq!(plan.container_name, "claude-demo-42");
}

#[test]
fn forwards_git_identity() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    inputs.git_name = Some("Test User");
    inputs.git_email = Some("test@example.com");
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"GIT_AUTHOR_NAME=Test User".into()));
    assert!(args.contains(&"GIT_COMMITTER_EMAIL=test@example.com".into()));
}

#[test]
fn anthropic_injects_no_secrets() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"HOST_HOME=/home/tester".into()));
    assert!(!args.iter().any(|a| a.contains("ANTHROPIC_API_KEY")));
}

#[test]
fn named_cache_volumes_by_default() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"claude-dock-claude-demo-target:/app/target".into()));
    assert!(args.contains(&"claude-dock-claude-demo-cargo-registry:/root/.cargo/registry".into()));
    assert!(args.contains(&"claude-dock-claude-demo-cargo-git:/root/.cargo/git".into()));
    assert!(args.contains(&"claude-dock-claude-demo-rustup:/root/.rustup".into()));
}

#[test]
fn no_cache_uses_tmpfs() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    inputs.no_cache = true;
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.iter().any(|a| a == "/app/target:size=2g"));
    assert!(!args
        .iter()
        .any(|a| a.contains("claude-dock-claude-demo-target")));
}

#[test]
fn custom_memory_override() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    inputs.memory = "4g";
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"4g".into()));
    assert!(!args.contains(&"8g".into()));
}

#[test]
fn port_forwarding() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    let ports = ["3000:3000".to_string(), "8080:8080".to_string()];
    inputs.ports = &ports;
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"3000:3000".into()));
    assert!(args.contains(&"8080:8080".into()));
}

#[test]
fn extra_mounts_allowed() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    let extra = ["/data/other:/opt/other:ro".to_string()];
    inputs.extra_mounts = &extra;
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"/data/other:/opt/other:ro".into()));
}

#[test]
fn claude_config_mounted_writable() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    let claude_mounts: Vec<_> = args
        .iter()
        .zip(args.iter().skip(1))
        .filter(|(k, _)| **k == "-v")
        .map(|(_, v)| v.as_str())
        .filter(|v| v.contains("/.claude:") && !v.contains(".claude.json"))
        .collect();
    for m in &claude_mounts {
        assert!(!m.ends_with(":ro"), ".claude must be writable: {m}");
    }
}

#[test]
fn jj_mounted_readonly() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    let jj_mounts: Vec<_> = args
        .iter()
        .zip(args.iter().skip(1))
        .filter(|(k, _)| **k == "-v")
        .map(|(_, v)| v.as_str())
        .filter(|v| v.contains(".jj"))
        .collect();
    for m in &jj_mounts {
        assert!(m.ends_with(":ro"), ".jj must be read-only: {m}");
    }
}

#[test]
fn uid_gid_via_env_not_flag() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(!args.contains(&"-u".into()));
    assert!(args.contains(&"CONTAINER_USER_ID=1000".into()));
    assert!(args.contains(&"CONTAINER_GROUP_ID=1000".into()));
}

#[test]
fn home_env_set() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "claude-demo");
    assert!(args.contains(&"HOME=/home/user".into()));
}

#[test]
fn cache_volume_name_format() {
    assert_eq!(
        cache_vol_name_for_test("claude-demo", "target"),
        "claude-dock-claude-demo-target"
    );
}

#[test]
fn entrypoint_runs_claude_directly() {
    let result = build_claude_exec_args(&[]);
    assert!(!result
        .iter()
        .any(|a| a.contains("--dangerously-skip-permissions")));
    assert!(!result.iter().any(|a| a.contains("zsh -c")));
    assert!(result.contains(&"/home/user/.local/bin/claude".into()));
}

#[test]
fn entrypoint_shell_uses_zsh() {
    let result = build_claude_exec_args(&["shell".into()]);
    assert!(result.contains(&"/bin/zsh".into()));
    assert!(!result.iter().any(|a| a.contains("/claude")));
}

#[test]
fn entrypoint_forwards_all_args() {
    let result = build_claude_exec_args(&["--verbose".into()]);
    assert!(result.contains(&"--verbose".into()));
}

#[test]
fn provider_rejects_unknown() {
    assert!(Provider::from_str_lossy("unknown_provider").is_err());
}

#[test]
fn provider_needs_auth_token() {
    assert!(!Provider::Anthropic.needs_auth_token());
    assert!(Provider::Minimax.needs_auth_token());
    assert!(Provider::Zai.needs_auth_token());
}

#[test]
fn validate_port_normal() {
    assert!(validate_port_for_test("3000:3000").is_ok());
    assert!(validate_port_for_test("8080:8080").is_ok());
}

#[test]
fn validate_port_rejects_privileged() {
    assert!(validate_port_for_test("22:22").is_err());
    assert!(validate_port_for_test("80:80").is_err());
}

#[test]
fn validate_mount_rejects_root() {
    assert!(validate_mount_for_test("/:/host").is_err());
}

#[test]
fn validate_mount_rejects_etc() {
    assert!(validate_mount_for_test("/etc:/etc:ro").is_err());
}

#[test]
fn validate_mount_rejects_var() {
    assert!(validate_mount_for_test("/var/lib:/var/lib:ro").is_err());
}

#[test]
fn validate_mount_rejects_proc() {
    assert!(validate_mount_for_test("/proc:/proc").is_err());
}

#[test]
fn validate_mount_rejects_home() {
    assert!(validate_mount_for_test("/home:/home:ro").is_err());
    assert!(validate_mount_for_test("/home/other:/home/other:ro").is_err());
}

#[test]
fn validate_mount_rejects_opt() {
    assert!(validate_mount_for_test("/opt:/opt:ro").is_err());
}

#[test]
fn validate_mount_rejects_tmp() {
    assert!(validate_mount_for_test("/tmp:/tmp").is_err());
}

#[test]
fn validate_mount_allows_safe() {
    assert!(validate_mount_for_test("/data/project:/opt/project:ro").is_ok());
}

#[test]
fn validate_mount_rejects_root_subdir() {
    assert!(validate_mount_for_test("/root/.ssh:/root/.ssh:ro").is_err());
}
