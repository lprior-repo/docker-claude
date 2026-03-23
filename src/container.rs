use std::process::Command;

use anyhow::{bail, Result};

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
        "--entrypoint".into(),
        "/usr/local/bin/claude-dock".into(),
    ];

    let mounts = [
        format!("{}:/app", inputs.project_dir),
        format!("{}/.claude:/home/user/.claude", inputs.host_home),
        format!("{}/.claude.json:/home/user/.claude.json", inputs.host_home),
        format!("{}/.gitconfig:/home/user/.gitconfig:ro", inputs.host_home),
        format!(
            "{}/.git-credentials:/home/user/.git-credentials:ro",
            inputs.host_home
        ),
        format!("{}/.jj:/home/user/.jj", inputs.host_home),
    ];
    for mount in &mounts {
        args.extend(["-v".into(), mount.clone()]);
    }

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

pub fn sanitise_project_name(folder: &str) -> String {
    sanitise_name(folder)
}

#[cfg(test)]
pub fn reattach_container_args(cname: &str) -> Vec<String> {
    reattach_args(cname)
}

#[cfg(test)]
pub fn build_container_args(inputs: &LaunchInputs<'_>, cname: &str) -> Vec<String> {
    new_container_args(inputs, cname)
}
