use crate::container::{
    build_container_args, security_args_for_test, ContainerState, LaunchInputs,
};
use crate::entrypoint::build_claude_exec_args;
use crate::provider::{ProfileConfig, Provider};
use crate::resolve_launch_plan;

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

fn vol_values(args: &[String]) -> Vec<&str> {
    args.iter()
        .zip(args.iter().skip(1))
        .filter(|(k, _)| **k == "-v")
        .map(|(_, v)| v.as_str())
        .collect()
}

fn env_values(args: &[String]) -> Vec<&str> {
    args.iter()
        .zip(args.iter().skip(1))
        .filter(|(k, _)| **k == "-e")
        .map(|(_, v)| v.as_str())
        .collect()
}

#[test]
fn security_read_only_always_present() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    assert!(
        args.contains(&"--read-only".into()),
        "root must always be read-only"
    );
}

#[test]
fn security_cap_drop_all_always_present() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    let cap_idx = args
        .iter()
        .position(|a| a == "--cap-drop")
        .expect("--cap-drop must exist");
    assert_eq!(args[cap_idx + 1], "ALL", "ALL capabilities must be dropped");
}

#[test]
fn security_no_new_privileges_always_present() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    assert!(
        args.contains(&"no-new-privileges:true".into()),
        "privilege escalation must be blocked"
    );
}

#[test]
fn security_never_adds_dangerously_skip_permissions() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    assert!(
        !args
            .iter()
            .any(|a| a.contains("--dangerously-skip-permissions")),
        "claude must always require approval"
    );
    let exec = build_claude_exec_args(&[]);
    assert!(
        !exec
            .iter()
            .any(|a| a.contains("--dangerously-skip-permissions")),
        "entrypoint must never skip permissions"
    );
}

#[test]
fn security_never_mounts_private_keys() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    for vol in vol_values(&args) {
        assert!(
            !vol.contains("id_rsa"),
            "id_rsa private key leaked in mount: {vol}"
        );
        assert!(
            !vol.contains("id_ed25519"),
            "id_ed25519 private key leaked in mount: {vol}"
        );
        assert!(
            !vol.contains("id_ecdsa"),
            "id_ecdsa private key leaked in mount: {vol}"
        );
        assert!(
            !vol.contains("id_dsa"),
            "id_dsa private key leaked in mount: {vol}"
        );
        assert!(
            !vol.contains(".pem"),
            "PEM private key leaked in mount: {vol}"
        );
    }
}

#[test]
fn security_never_mounts_git_credentials() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    for vol in vol_values(&args) {
        assert!(
            !vol.contains("git-credentials"),
            "git-credentials plaintext password leaked: {vol}"
        );
    }
}

#[test]
fn security_never_mounts_ssh_private_dir() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    for vol in vol_values(&args) {
        let src = vol.split(':').next().unwrap_or("");
        assert!(
            !src.contains("/.ssh/id_"),
            "private key directory leaked: {vol}"
        );
    }
}

#[test]
fn security_no_host_access_by_default() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    assert!(
        !args.iter().any(|a| a.contains("host.docker.internal")),
        "host network access must be opt-in"
    );
}

#[test]
fn security_memory_swap_equals_memory_prevents_host_swap() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    inputs.memory = "4g";
    let args = build_container_args(&inputs, "test");
    let mem_idx = args.iter().position(|a| a == "--memory").unwrap();
    let swap_idx = args.iter().position(|a| a == "--memory-swap").unwrap();
    assert_eq!(
        args[mem_idx + 1],
        args[swap_idx + 1],
        "swap must equal memory to prevent host swap thrashing"
    );
}

#[test]
fn security_pids_limit_always_set() {
    let sec = security_args_for_test();
    let pids_idx = sec
        .iter()
        .position(|a| a == "--pids-limit")
        .expect("--pids-limit must exist");
    let limit: u32 = sec[pids_idx + 1]
        .parse()
        .expect("pids-limit must be a number");
    assert!(
        limit <= 1024,
        "PID limit {limit} is dangerously high — fork bomb risk"
    );
}

#[test]
fn security_nofile_limit_always_set() {
    let sec = security_args_for_test();
    assert!(
        sec.iter().any(|a| a == "nofile=65536:65536"),
        "ulimit nofile must be set"
    );
}

#[test]
fn security_ssh_config_never_raw_mount() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    for vol in vol_values(&args) {
        if vol.contains("ssh") && vol.contains("config") {
            let src = vol.split(':').next().unwrap_or("");
            assert!(
                !src.contains("/home/tester/.ssh/config"),
                "raw SSH config must never be mounted directly: {vol}"
            );
            assert!(
                src.contains("/tmp/claude-dock-ssh-config-"),
                "SSH config must be a sanitized temp file: {vol}"
            );
        }
    }
}

#[test]
fn security_claude_dir_never_readonly() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    for vol in vol_values(&args) {
        if vol.contains("/.claude:") && !vol.contains(".claude.json") {
            assert!(
                !vol.ends_with(":ro"),
                ".claude must be writable for sessions/hooks: {vol}"
            );
        }
    }
}

#[test]
fn security_no_uid_flag_on_command_line() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    let u_idx = args.iter().position(|a| a == "-u");
    assert!(
        u_idx.is_none(),
        "-u flag must not be used — gosu handles it inside the container"
    );
}

#[test]
fn security_anthropic_provider_never_exposes_keys() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    let dangerous_patterns = [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "API_KEY=",
        "SECRET=",
        "TOKEN=",
    ];
    for arg in &args {
        for pattern in &dangerous_patterns {
            assert!(
                !arg.contains(pattern),
                "Anthropic provider must never inject secrets — found '{pattern}' in arg: {arg}"
            );
        }
    }
}

#[test]
fn security_no_env_prevents_env_file_leak() {
    let config = minimax_config();
    let mut inputs = make_inputs(&config, &[]);
    inputs.no_env = true;
    let args = build_container_args(&inputs, "test");
    assert!(
        !args.iter().any(|a| a.contains("--env-file")),
        "--no-env must prevent .env auto-loading"
    );
    assert!(
        !args.iter().any(|a| a.contains(".env")),
        "no .env references should appear when --no-env is set"
    );
}

#[test]
fn security_dangerous_docker_flags_never_appear_in_args() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    let banned_flags = [
        "--privileged",
        "--cap-add",
        "--pid=host",
        "--ipc=host",
        "--network=host",
        "--userns=host",
        "--uts=host",
        "--security-opt=seccomp=unconfined",
        "--security-opt=apparmor=unconfined",
        "-v /:/host",
        "--rootfs",
        "--mount type=bind,source=/,target=",
    ];
    for arg in &args {
        for banned in &banned_flags {
            assert!(
                !arg.contains(banned),
                "banned docker flag '{banned}' found in args: {arg}"
            );
        }
    }
}

#[test]
fn security_entrypoint_is_always_our_wrapper() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    let ep_idx = args
        .iter()
        .position(|a| a == "--entrypoint")
        .expect("--entrypoint must exist");
    assert_eq!(
        args[ep_idx + 1],
        "/usr/local/bin/claude-dock",
        "entrypoint must always be our wrapper script"
    );
}

#[test]
fn security_entrypoint_never_calls_raw_claude() {
    let exec = build_claude_exec_args(&[]);
    assert!(
        exec.contains(&"/home/user/.local/bin/claude".into()),
        "claude binary path must go through our wrapper"
    );
    assert!(
        !exec.contains(&"/usr/local/bin/claude".into()),
        "must never exec raw claude binary directly"
    );
}

#[test]
fn security_entrypoint_never_passes_shell_escape() {
    let exec = build_claude_exec_args(&["--verbose".into()]);
    assert!(
        !exec.iter().any(|a| a.contains("zsh -c")),
        "must never shell-escape claude args through zsh -c"
    );
    assert!(
        !exec.iter().any(|a| a.contains("bash -c")),
        "must never shell-escape claude args through bash -c"
    );
    assert!(
        !exec.iter().any(|a| a.contains("sh -c")),
        "must never shell-escape claude args through sh -c"
    );
}

#[test]
fn security_no_root_dotfiles_mounted() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    for vol in vol_values(&args) {
        let src = vol.split(':').next().unwrap_or("");
        assert!(
            !src.starts_with("/root"),
            "host root files must never be mounted: {vol}"
        );
    }
}

#[test]
fn security_container_always_runs_as_nonroot() {
    let exec = build_claude_exec_args(&[]);
    assert!(
        exec.first().map(String::as_str) == Some("claudeuser"),
        "must always exec as claudeuser via gosu, not root"
    );
}

#[test]
fn security_provider_keys_never_in_container_args_vector() {
    let config = minimax_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    assert!(
        !args.iter().any(|a| a.contains("minimax-key")),
        "provider key must not appear in container args (passed via Command::env)"
    );
    assert!(
        !args.iter().any(|a| a.contains("sk-ant")),
        "API key must not appear in container args vector"
    );
}

#[test]
fn security_gitconfig_always_readonly() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    for vol in vol_values(&args) {
        if vol.contains("gitconfig") {
            assert!(vol.ends_with(":ro"), "gitconfig must be read-only: {vol}");
        }
    }
}

#[test]
fn security_jj_always_readonly() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    for vol in vol_values(&args) {
        if vol.contains(".jj") {
            assert!(vol.ends_with(":ro"), ".jj must be read-only: {vol}");
        }
    }
}

#[test]
fn security_resume_does_not_bypass_security() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let plan = resolve_launch_plan(ContainerState::Stopped, inputs);
    assert_eq!(plan.mode, crate::container::LaunchMode::Resume);
    let resume_args = plan.args.join(" ");
    assert!(
        resume_args.contains("-ai"),
        "resume must use interactive attach, not raw exec"
    );
}

#[test]
fn security_no_tmpfs_overlay_for_claude_dir() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    assert!(
        !args
            .iter()
            .any(|a| a.starts_with("/home/user/.claude:size=")),
        "tmpfs overlay for .claude was removed — bind mount is correct"
    );
}

#[test]
fn security_host_home_never_root() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    inputs.host_home = "/root";
    let args = build_container_args(&inputs, "test");
    for env in env_values(&args) {
        if env.starts_with("HOST_HOME=") {
            assert!(
                env != "HOST_HOME=/root",
                "HOST_HOME=/root would symlink /root to /home/user — dangerous"
            );
        }
    }
}
