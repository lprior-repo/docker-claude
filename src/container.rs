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
    pub fn binary_name(&self) -> &'static str {
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
    pub host_access: bool,
    pub no_cache: bool,
    pub nonce: u32,
    pub git_name: Option<&'a str>,
    pub git_email: Option<&'a str>,
}

pub fn resolve_launch_plan(state: ContainerState, inputs: LaunchInputs<'_>) -> LaunchPlan {
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
                args: new_container_args(&inputs, &container_name),
            }
        }
    }
}

fn new_container_args(inputs: &LaunchInputs<'_>, cname: &str) -> Vec<String> {
    let uses_print_mode = inputs
        .extra_claude_args
        .iter()
        .any(|arg| arg == "-p" || arg == "--print");
    let mut args = vec![
        "run".into(),
        if uses_print_mode {
            "-i".into()
        } else {
            "-it".into()
        },
        "--rm".into(),
        "--name".into(),
        cname.into(),
    ];

    args.extend(["--read-only".into()]);
    args.extend(["--cap-drop".into(), "ALL".into()]);
    args.extend(["--security-opt".into(), "no-new-privileges:true".into()]);
    args.extend(["--ulimit".into(), "nofile=65536:65536".into()]);
    args.extend(["--pids-limit".into(), "512".into()]);

    let memory = if inputs.memory.is_empty() {
        "8g"
    } else {
        inputs.memory
    };
    args.extend(["--memory".into(), memory.into()]);
    args.extend(["--memory-swap".into(), memory.into()]);

    if !inputs.cpus.is_empty() {
        args.extend(["--cpus".into(), inputs.cpus.into()]);
    }

    args.extend(["--entrypoint".into(), "/usr/local/bin/claude-dock".into()]);

    for port in inputs.ports {
        if let Err(e) = validate_port(port) {
            eprintln!("WARN: {e} — skipping");
            continue;
        }
        args.extend(["-p".into(), port.clone()]);
    }

    for mount in inputs.extra_mounts {
        if let Err(e) = validate_mount(mount) {
            eprintln!("WARN: {e} — skipping");
            continue;
        }
        args.extend(["-v".into(), mount.clone()]);
    }

    let mut bind_mounts: Vec<String> = [
        format!("{}:/app", inputs.project_dir),
        format!("{}/.claude:/home/user/.claude", inputs.host_home),
        format!(
            "{}/.claude.json:/home/user/.claude.json:ro",
            inputs.host_home
        ),
        format!("{}/.gitconfig:/home/user/.gitconfig:ro", inputs.host_home),
        format!("{}/.jj:/home/user/.jj:ro", inputs.host_home),
    ]
    .into_iter()
    .collect();

    if let Some(m) = optional_file_mount(inputs.host_home, ".zshrc", "/home/user/.zshrc") {
        bind_mounts.push(m);
    }
    if let Some(m) = optional_file_mount(inputs.host_home, ".zshenv", "/home/user/.zshenv") {
        bind_mounts.push(m);
    }
    if let Some(m) = optional_file_mount(inputs.host_home, ".zprofile", "/home/user/.zprofile") {
        bind_mounts.push(m);
    }
    if let Some(m) = optional_file_mount(
        inputs.host_home,
        ".config/starship.toml",
        "/home/user/.config/starship.toml",
    ) {
        bind_mounts.push(m);
    }
    if let Some(known_hosts) = ssh_known_hosts_path(inputs.host_home) {
        bind_mounts.push(format!("{known_hosts}:/home/user/.ssh/known_hosts:ro"));
    }
    if let Some(ssh_config) = ssh_config_path(inputs.host_home) {
        bind_mounts.push(format!("{ssh_config}:/home/user/.ssh/config:ro"));
    }

    let env_file = format!("{}/.env", inputs.project_dir);
    if Path::new(&env_file).is_file() {
        args.extend(["--env-file".into(), env_file]);
    }

    for mount in &bind_mounts {
        args.extend(["-v".into(), mount.clone()]);
    }

    let tmpfs_mounts = [
        "/tmp:size=500m",
        "/run:size=10m",
        "/home/user/.local:size=500m",
        "/home/user/.gnupg:size=10m",
        "/home/user/.config:size=10m",
        "/home/user/.cache:size=500m",
    ];
    for tmpfs in &tmpfs_mounts {
        args.extend(["--tmpfs".into(), (*tmpfs).into()]);
    }

    if inputs.no_cache {
        args.extend(["--tmpfs".into(), "/app/target:size=2g".into()]);
    } else {
        let vol_name = cache_volume_name(cname, "target");
        args.extend(["-v".into(), format!("{vol_name}:/app/target")]);
        let cargo_vols = [
            format!(
                "{}:/root/.cargo/registry",
                cache_volume_name(cname, "cargo-registry")
            ),
            format!("{}:/root/.cargo/git", cache_volume_name(cname, "cargo-git")),
            format!("{}:/root/.rustup", cache_volume_name(cname, "rustup")),
        ];
        for vol in &cargo_vols {
            args.extend(["-v".into(), vol.clone()]);
        }
    }

    if inputs.host_access {
        args.extend([
            "--add-host".into(),
            "host.docker.internal:host-gateway".into(),
        ]);
    }

    if let Some(sock) = ssh_agent_socket_path() {
        args.extend([
            "-v".into(),
            format!("{sock}:/tmp/ssh-agent.sock"),
            "-e".into(),
            "SSH_AUTH_SOCK=/tmp/ssh-agent.sock".into(),
        ]);
    }

    if let Some(sock) = gpg_agent_socket_path(inputs.uid) {
        args.extend([
            "-v".into(),
            format!("{sock}:/tmp/gpg-agent.sock"),
            "-e".into(),
            "GPG_AGENT_SOCK=/tmp/gpg-agent.sock".into(),
            "-e".into(),
            "GPG_TTY=/dev/tty".into(),
        ]);
    }

    args.extend(["-e".into(), "HOME=/home/user".to_string()]);
    args.extend(["-e".into(), format!("HOST_HOME={}", inputs.host_home)]);
    args.extend(["-e".into(), format!("CONTAINER_USER_ID={}", inputs.uid)]);
    args.extend(["-e".into(), format!("CONTAINER_GROUP_ID={}", inputs.gid)]);

    if let Some(name) = inputs.git_name {
        args.extend(["-e".into(), format!("GIT_AUTHOR_NAME={name}")]);
        args.extend(["-e".into(), format!("GIT_COMMITTER_NAME={name}")]);
    }
    if let Some(email) = inputs.git_email {
        args.extend(["-e".into(), format!("GIT_AUTHOR_EMAIL={email}")]);
        args.extend(["-e".into(), format!("GIT_COMMITTER_EMAIL={email}")]);
    }

    for (key, value) in inputs.config.provider.env_vars() {
        if value.is_empty() {
            args.extend(["-e".into(), key.into()]);
        } else {
            args.extend(env_arg(key, value));
        }
    }

    args.push(inputs.image.into());
    args.push("__entrypoint".into());
    args.push("--".into());
    args.extend_from_slice(inputs.extra_claude_args);
    args
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
    for name in names.lines() {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let _ = Command::new(backend.binary_name())
            .args(["rm", "-f", name])
            .status();
    }
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
    for vol in vols.lines() {
        let vol = vol.trim();
        if vol.is_empty() {
            continue;
        }
        let _ = Command::new(backend.binary_name())
            .args(["volume", "rm", "-f", vol])
            .status();
    }
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
