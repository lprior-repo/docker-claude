use std::path::Path;

use anyhow::{bail, Context, Result};

pub(crate) fn validate_port(port: &str) -> Result<()> {
    if port.is_empty() {
        bail!("empty port specification");
    }
    let parts: Vec<&str> = port.split(':').collect();
    let container_port = parts.last().unwrap_or(&parts[0]);
    let container_port = container_port.split('/').next().unwrap_or(container_port);
    let port_num: u16 = container_port
        .parse()
        .with_context(|| format!("invalid port number in '{port}'"))?;
    if (1..=1023).contains(&port_num) {
        bail!("privileged container port rejected: '{port}' — ports 1-1023 are not allowed");
    }
    Ok(())
}

pub(crate) fn validate_mount(mount: &str) -> Result<()> {
    let parts: Vec<&str> = mount.split(':').collect();
    if parts.len() < 2 {
        bail!("invalid mount format: {mount}");
    }
    let host_path = parts[0];
    let dangerous = [
        "/", "/root", "/etc", "/var", "/usr", "/bin", "/sbin", "/lib", "/sys", "/dev", "/proc",
    ];
    for d in &dangerous {
        if host_path == *d || host_path.starts_with(&format!("{d}/")) {
            bail!(
                "dangerous mount rejected: '{mount}' — host system directory mount is not allowed"
            );
        }
    }
    Ok(())
}

pub(crate) fn ssh_agent_socket_path() -> Option<String> {
    std::env::var("SSH_AUTH_SOCK")
        .ok()
        .filter(|p| Path::new(p).exists())
}

pub(crate) fn ssh_known_hosts_path(host_home: &str) -> Option<String> {
    let p = format!("{host_home}/.ssh/known_hosts");
    Path::new(&p).is_file().then_some(p)
}

pub(crate) fn ssh_config_path(host_home: &str) -> Option<String> {
    let src = format!("{host_home}/.ssh/config");
    if !Path::new(&src).is_file() {
        return None;
    }
    let content = match std::fs::read_to_string(&src) {
        Ok(c) => c,
        Err(_) => return None,
    };
    let sanitized: String = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim().to_lowercase();
            !trimmed.starts_with("identityfile")
        })
        .collect::<Vec<_>>()
        .join("\n");
    if sanitized.trim().is_empty() {
        return None;
    }
    let tmp = format!("/tmp/claude-dock-ssh-config-{host_home}.conf");
    let _ = std::fs::write(&tmp, sanitized);
    Some(tmp)
}

pub(crate) fn gpg_agent_socket_path(uid: &str) -> Option<String> {
    let sock = std::env::var("GPG_AGENT_SOCK")
        .unwrap_or_else(|_| format!("/run/user/{uid}/gnupg/S.gpg-agent"));
    Path::new(&sock).exists().then_some(sock)
}

pub(crate) fn optional_file_mount(
    host_home: &str,
    relative: &str,
    container: &str,
) -> Option<String> {
    let src = format!("{host_home}/{relative}");
    Path::new(&src)
        .is_file()
        .then(|| format!("{src}:{container}:ro"))
}

#[cfg(test)]
pub(crate) fn validate_mount_for_test(mount: &str) -> Result<()> {
    validate_mount(mount)
}

#[cfg(test)]
pub(crate) fn validate_port_for_test(port: &str) -> Result<()> {
    validate_port(port)
}
