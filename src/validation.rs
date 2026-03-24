use std::path::Path;

use anyhow::{bail, Result};

pub(crate) fn validate_port(port: &str) -> Result<()> {
    if port.is_empty() {
        bail!("empty port specification");
    }
    port.split(':').try_for_each(|segment| {
        let port_str = segment.split('/').next().map_or(segment, |s| s);
        let port_num: u16 = port_str
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid port number in '{port}'"))?;
        if (0..=1023).contains(&port_num) {
            bail!("privileged port rejected: '{port}' — ports 0-1023 are not allowed");
        }
        Ok(())
    })
}

fn normalize_path(path: &str) -> String {
    let components: Vec<&str> =
        path.split('/')
            .filter(|p| !p.is_empty() && *p != ".")
            .fold(Vec::new(), |mut acc, part| {
                if part == ".." {
                    acc.pop();
                } else {
                    acc.push(part);
                }
                acc
            });
    if components.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", components.join("/"))
    }
}

pub(crate) fn validate_mount(mount: &str) -> Result<()> {
    const DANGEROUS: &[&str] = &[
        "/", "/root", "/etc", "/var", "/usr", "/bin", "/sbin", "/lib", "/sys", "/dev", "/proc",
        "/home", "/opt", "/tmp",
    ];
    let parts: Vec<&str> = mount.split(':').collect();
    if parts.len() < 2 {
        bail!("invalid mount format: {mount}");
    }
    if parts[0].is_empty() {
        bail!("invalid mount format: empty host path in '{mount}'");
    }
    let host_path = normalize_path(parts[0]);
    if DANGEROUS
        .iter()
        .any(|d| host_path == **d || host_path.starts_with(&format!("{d}/")))
    {
        bail!("dangerous mount rejected: '{mount}' — host system directory mount is not allowed");
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
    let Ok(content) = std::fs::read_to_string(&src) else {
        return None;
    };
    let stripped = [
        "identityfile",
        "proxycommand",
        "proxyjump",
        "include",
        "localcommand",
        "permitlocalcommand",
        "remotecommand",
        "match",
        "forwardagent",
        "sendenv",
        "setenv",
        "certificatefile",
        "pkcs11provider",
    ];
    let sanitized: String = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim().to_lowercase();
            stripped.iter().all(|d| !trimmed.starts_with(*d))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if sanitized.trim().is_empty() {
        return None;
    }
    let safe_name = host_home.replace('/', "_");
    let tmp = format!("/tmp/claude-dock-ssh-config-{safe_name}.conf");
    if let Err(e) = std::fs::write(&tmp, &sanitized) {
        eprintln!("WARN: failed to write sanitized SSH config: {e}");
        return None;
    }
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
