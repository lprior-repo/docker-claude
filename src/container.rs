use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

use crate::validation::{
    gpg_agent_socket_path, optional_file_mount, ssh_agent_socket_path, ssh_config_path,
    ssh_known_hosts_path, validate_mount, validate_port,
};

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum ContainerBackend {
    Docker,
    Podman,
}

impl ContainerBackend {
    pub fn binary_name(self) -> &'static str {
        match self {
            ContainerBackend::Docker => "docker",
            ContainerBackend::Podman => "podman",
        }
    }
}

pub fn detect_backend() -> Result<ContainerBackend> {
    if let Ok(be) = std::env::var("CLAUDE_BACKEND") {
        return match be.to_lowercase().as_str() {
            "docker" => Ok(ContainerBackend::Docker),
            "podman" => Ok(ContainerBackend::Podman),
            other => bail!("invalid CLAUDE_BACKEND '{other}' — expected 'docker' or 'podman'"),
        };
    }
    if which::which("docker").is_ok() {
        return Ok(ContainerBackend::Docker);
    }
    if which::which("podman").is_ok() {
        return Ok(ContainerBackend::Podman);
    }
    bail!("'docker' or 'podman' not found — is a container engine installed and running?")
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum ContainerState {
    Running,
    Stopped,
    Missing,
}

pub fn probe_container(backend: ContainerBackend, cname: &str) -> ContainerState {
    let output = Command::new(backend.binary_name())
        .args([
            "ps",
            "-a",
            "--format",
            "{{.Names}}\t{{.Status}}",
            "--filter",
            &format!("name=^{cname}$"),
        ])
        .output();
    output.map_or(ContainerState::Missing, |result| {
        let text = String::from_utf8_lossy(&result.stdout);
        let text = text.trim();
        if text.is_empty() {
            ContainerState::Missing
        } else if text.lines().any(|line: &str| line.contains("\tUp ")) {
            ContainerState::Running
        } else {
            ContainerState::Stopped
        }
    })
}

fn sanitise_name(folder: &str) -> String {
    let mapped: String = folder
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = mapped.trim_matches('-');
    if trimmed.is_empty() {
        "claude-project".to_string()
    } else {
        format!("claude-{trimmed}")
    }
}

fn env_arg(key: &str, value: &str) -> Vec<String> {
    vec!["-e".into(), format!("{key}={value}")]
}

fn reattach_args(cname: &str) -> Vec<String> {
    vec!["start".into(), "-ai".into(), cname.into()]
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum LaunchMode {
    New,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub mode: LaunchMode,
    pub container_name: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy, Default)]
pub enum HostAccess {
    #[default]
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy, Default)]
pub enum CacheMode {
    #[default]
    Persistent,
    Ephemeral,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy, Default)]
pub enum EnvMode {
    #[default]
    Loaded,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy, Default)]
pub enum GpuMode {
    #[default]
    None,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchInputs<'a> {
    pub image: &'a str,
    pub config: &'a crate::provider::ProfileConfig,
    pub project_dir: &'a str,
    pub base_name: &'a str,
    pub host_home: &'a str,
    pub uid: &'a str,
    pub gid: &'a str,
    pub extra_claude_args: &'a [String],
    pub ports: &'a [String],
    pub extra_mounts: &'a [String],
    pub memory: &'a str,
    pub cpus: &'a str,
    pub host_access: HostAccess,
    pub cache: CacheMode,
    pub env: EnvMode,
    pub gpus: GpuMode,
    pub nonce: u32,
    pub git_name: Option<&'a str>,
    pub git_email: Option<&'a str>,
}

#[allow(clippy::needless_pass_by_value)]
pub fn resolve_launch_plan(state: ContainerState, inputs: &LaunchInputs<'_>) -> LaunchPlan {
    match state {
        ContainerState::Stopped => LaunchPlan {
            mode: LaunchMode::Resume,
            container_name: inputs.base_name.to_owned(),
            args: reattach_args(inputs.base_name),
        },
        ContainerState::Missing | ContainerState::Running => {
            let container_name = if state == ContainerState::Running {
                format!("{}-{}", inputs.base_name, inputs.nonce)
            } else {
                inputs.base_name.to_owned()
            };
            LaunchPlan {
                mode: LaunchMode::New,
                container_name: container_name.clone(),
                args: new_container_args(inputs, &container_name),
            }
        }
    }
}

fn security_args() -> Vec<String> {
    [
        "--read-only",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges:true",
        "--ulimit",
        "nofile=65536:65536",
        "--pids-limit",
        "512",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn resource_args(memory: &str, cpus: &str) -> Vec<String> {
    let effective = if memory.is_empty() {
        "8g"
    } else if memory == "0" || memory.parse::<u64>().is_ok_and(|v| v < 1_073_741_824) {
        "1g"
    } else {
        memory
    };
    let base: Vec<String> = ["--memory", effective, "--memory-swap", effective]
        .into_iter()
        .map(String::from)
        .collect();
    let cpu_arg: Vec<String> = if cpus.is_empty() {
        Vec::new()
    } else {
        vec!["--cpus".to_string(), cpus.to_string()]
    };
    base.into_iter().chain(cpu_arg).collect()
}

fn validated_ports(ports: &[String]) -> Vec<String> {
    ports
        .iter()
        .filter_map(|port| {
            validate_port(port)
                .map(|()| ["-p".to_string(), port.clone()])
                .map(Vec::from)
                .map_err(|e| eprintln!("WARN: {e} — skipping"))
                .ok()
        })
        .flatten()
        .collect()
}

fn validated_mounts(mounts: &[String]) -> Vec<String> {
    mounts
        .iter()
        .filter_map(|mount| {
            validate_mount(mount)
                .map(|()| ["-v".to_string(), mount.clone()])
                .map(Vec::from)
                .map_err(|e| eprintln!("WARN: {e} — skipping"))
                .ok()
        })
        .flatten()
        .collect()
}

fn bind_mounts(inputs: &LaunchInputs<'_>) -> Vec<String> {
    let base_mounts: Vec<String> = vec![
        format!("{}:/app", inputs.project_dir),
        format!("{}/.claude:/home/user/.claude", inputs.host_home),
        format!("{}/.claude.json:/home/user/.claude.json", inputs.host_home),
        format!("{}/.gitconfig:/home/user/.gitconfig:ro", inputs.host_home),
        format!("{}/.jj:/home/user/.jj:ro", inputs.host_home),
    ];
    let optional_configs: Vec<String> = [
        (".zshrc", "/home/user/.zshrc"),
        (".zshenv", "/home/user/.zshenv"),
        (".zprofile", "/home/user/.zprofile"),
        (".config/starship.toml", "/home/user/.config/starship.toml"),
    ]
    .into_iter()
    .filter_map(|(rel, dest)| optional_file_mount(inputs.host_home, rel, dest))
    .collect();
    let ssh_mounts: Vec<String> = [
        ssh_known_hosts_path(inputs.host_home)
            .map(|p| format!("{p}:/home/user/.ssh/known_hosts:ro")),
        ssh_config_path(inputs.host_home).map(|p| format!("{p}:/home/user/.ssh/config:ro")),
    ]
    .into_iter()
    .flatten()
    .collect();

    base_mounts
        .into_iter()
        .chain(optional_configs)
        .chain(ssh_mounts)
        .flat_map(|m| ["-v".to_string(), m])
        .collect()
}

fn env_file_arg(inputs: &LaunchInputs<'_>) -> Vec<String> {
    if inputs.env == EnvMode::Skipped {
        return Vec::new();
    }
    let env_file = format!("{}/.env", inputs.project_dir);
    if Path::new(&env_file).is_file() {
        vec!["--env-file".into(), env_file]
    } else {
        Vec::new()
    }
}

fn tmpfs_args() -> Vec<String> {
    [
        "/tmp:size=500m",
        "/run:size=10m",
        "/home/user/.local:size=500m",
        "/home/user/.gnupg:size=10m",
        "/home/user/.config:size=10m",
        "/home/user/.cache:size=500m",
    ]
    .into_iter()
    .flat_map(|t| ["--tmpfs".into(), t.into()])
    .collect()
}

fn cache_args(cname: &str, cache: CacheMode) -> Vec<String> {
    if cache == CacheMode::Ephemeral {
        return vec!["--tmpfs".into(), "/app/target:size=2g".into()];
    }
    [
        ("target", "/app/target"),
        ("cargo-registry", "/root/.cargo/registry"),
        ("cargo-git", "/root/.cargo/git"),
        ("rustup", "/root/.rustup"),
    ]
    .into_iter()
    .map(|(suffix, dest)| format!("{}:{dest}", cache_volume_name(cname, suffix)))
    .flat_map(|v| ["-v".into(), v])
    .collect()
}

fn host_access_arg(host_access: HostAccess) -> Vec<String> {
    if host_access == HostAccess::Enabled {
        vec![
            "--add-host".to_string(),
            "host.docker.internal:host-gateway".to_string(),
        ]
    } else {
        Vec::new()
    }
}

fn agent_forwarding_args(uid: &str) -> Vec<String> {
    let ssh_args: Vec<String> = ssh_agent_socket_path()
        .map(|sock| {
            vec![
                "-v".into(),
                format!("{sock}:/tmp/ssh-agent.sock"),
                "-e".into(),
                "SSH_AUTH_SOCK=/tmp/ssh-agent.sock".into(),
            ]
        })
        .unwrap_or_default();
    let gpg_args: Vec<String> = gpg_agent_socket_path(uid)
        .map(|sock| {
            vec![
                "-v".into(),
                format!("{sock}:/tmp/gpg-agent.sock"),
                "-e".into(),
                "GPG_AGENT_SOCK=/tmp/gpg-agent.sock".into(),
                "-e".into(),
                "GPG_TTY=/dev/tty".into(),
            ]
        })
        .unwrap_or_default();
    ssh_args.into_iter().chain(gpg_args).collect()
}

fn container_env_args(inputs: &LaunchInputs<'_>) -> Vec<String> {
    let host_home = if inputs.host_home == "/root" {
        "/home/user"
    } else {
        inputs.host_home
    };
    [
        ("HOME", "/home/user"),
        ("HOST_HOME", host_home),
        ("CONTAINER_USER_ID", inputs.uid),
        ("CONTAINER_GROUP_ID", inputs.gid),
    ]
    .into_iter()
    .flat_map(|(k, v)| ["-e".to_string(), format!("{k}={v}")])
    .chain(
        inputs
            .git_name
            .map(|n| {
                [
                    "-e".to_string(),
                    format!("GIT_AUTHOR_NAME={n}"),
                    "-e".into(),
                    format!("GIT_COMMITTER_NAME={n}"),
                ]
            })
            .into_iter()
            .flatten(),
    )
    .chain(
        inputs
            .git_email
            .map(|e| {
                [
                    "-e".to_string(),
                    format!("GIT_AUTHOR_EMAIL={e}"),
                    "-e".into(),
                    format!("GIT_COMMITTER_EMAIL={e}"),
                ]
            })
            .into_iter()
            .flatten(),
    )
    .chain(
        inputs
            .config
            .provider
            .env_vars()
            .into_iter()
            .flat_map(|(key, value)| {
                if value.is_empty() {
                    vec!["-e".to_string(), key.to_string()]
                } else {
                    env_arg(key, value)
                }
            }),
    )
    .collect()
}

fn new_container_args(inputs: &LaunchInputs<'_>, cname: &str) -> Vec<String> {
    let uses_noninteractive = inputs.extra_claude_args.iter().any(|arg| {
        arg == "-p"
            || arg == "--print"
            || arg == "--version"
            || arg == "-v"
            || arg == "--help"
            || arg == "-h"
    });

    let header: Vec<String> = [
        "run",
        if uses_noninteractive { "-i" } else { "-it" },
        "--rm",
        "--network",
        "host",
        "--name",
        cname,
        "--entrypoint",
        "/usr/local/bin/claude-dock",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    let gpu_arg: Vec<String> = if inputs.gpus == GpuMode::All {
        vec!["--gpus".to_string(), "all".to_string()]
    } else {
        Vec::new()
    };

    let claude_args: Vec<String> = [inputs.image.into(), "__entrypoint".into(), "--".into()]
        .into_iter()
        .chain(inputs.extra_claude_args.iter().cloned())
        .collect();

    [
        header,
        security_args(),
        resource_args(inputs.memory, inputs.cpus),
        gpu_arg,
        validated_ports(inputs.ports),
        validated_mounts(inputs.extra_mounts),
        bind_mounts(inputs),
        env_file_arg(inputs),
        tmpfs_args(),
        cache_args(cname, inputs.cache),
        host_access_arg(inputs.host_access),
        agent_forwarding_args(inputs.uid),
        container_env_args(inputs),
        claude_args,
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn cache_volume_name(base_name: &str, suffix: &str) -> String {
    format!("claude-dock-{base_name}-{suffix}")
}

pub fn sanitise_project_name(folder: &str) -> String {
    sanitise_name(folder)
}

pub fn cmd_cleanup(backend: ContainerBackend) -> Result<()> {
    let output = Command::new(backend.binary_name())
        .args([
            "ps",
            "-a",
            "--filter",
            "label=claude-dock",
            "--format",
            "{{.Names}}",
        ])
        .output()?;
    let names = String::from_utf8_lossy(&output.stdout);
    names
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .for_each(|name| {
            let _ = Command::new(backend.binary_name())
                .args(["rm", "-f", name])
                .status();
        });
    let vol_output = Command::new(backend.binary_name())
        .args([
            "volume",
            "ls",
            "--filter",
            "name=claude-dock-",
            "--format",
            "{{.Name}}",
        ])
        .output()?;
    let vols = String::from_utf8_lossy(&vol_output.stdout);
    vols.lines()
        .map(str::trim)
        .filter(|vol| !vol.is_empty())
        .for_each(|vol| {
            let _ = Command::new(backend.binary_name())
                .args(["volume", "rm", "-f", vol])
                .status();
        });
    Ok(())
}

pub fn cmd_stop(backend: ContainerBackend, cname: &str) -> Result<()> {
    let state = probe_container(backend, cname);
    match state {
        ContainerState::Running => {
            Command::new(backend.binary_name())
                .args(["stop", cname])
                .status()?;
            println!("Stopped {cname}");
        }
        ContainerState::Stopped => println!("{cname} is already stopped"),
        ContainerState::Missing => println!("{cname} not found"),
    }
    Ok(())
}

pub fn cmd_ps(backend: ContainerBackend) -> Result<()> {
    let output = Command::new(backend.binary_name())
        .args([
            "ps",
            "-a",
            "--filter",
            "label=claude-dock",
            "--format",
            "table {{.Names}}\t{{.Status}}\t{{.Ports}}",
        ])
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    if text.is_empty() || text.lines().count() <= 1 {
        println!("No claude-dock containers found.");
    } else {
        println!("{text}");
    }
    Ok(())
}

#[cfg(test)]
pub fn cache_vol_name_for_test(cname: &str, suffix: &str) -> String {
    cache_volume_name(cname, suffix)
}

#[cfg(test)]
pub fn reattach_container_args(cname: &str) -> Vec<String> {
    reattach_args(cname)
}

#[cfg(test)]
pub fn build_container_args(inputs: &LaunchInputs<'_>, cname: &str) -> Vec<String> {
    new_container_args(inputs, cname)
}

#[cfg(test)]
pub fn security_args_for_test() -> Vec<String> {
    security_args()
}

#[cfg(test)]
pub fn validated_ports_for_test(ports: &[String]) -> Vec<String> {
    validated_ports(ports)
}

#[cfg(test)]
pub fn validated_mounts_for_test(mounts: &[String]) -> Vec<String> {
    validated_mounts(mounts)
}

#[cfg(test)]
pub fn bind_mounts_for_test(inputs: &LaunchInputs<'_>) -> Vec<String> {
    bind_mounts(inputs)
}

#[cfg(test)]
pub fn cache_args_for_test(cname: &str, cache: CacheMode) -> Vec<String> {
    cache_args(cname, cache)
}

#[cfg(test)]
pub fn host_access_args_for_test(host_access: HostAccess) -> Vec<String> {
    host_access_arg(host_access)
}

#[cfg(test)]
pub fn container_env_args_for_test(inputs: &LaunchInputs<'_>) -> Vec<String> {
    container_env_args(inputs)
}
