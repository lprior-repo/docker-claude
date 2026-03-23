use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::Context;

pub fn setup_system_user(uid: &str, gid: &str) {
    let _ = Command::new("groupadd")
        .args(["-g", gid, "claudegroup"])
        .status();
    let _ = Command::new("useradd")
        .args([
            "-u",
            uid,
            "-g",
            gid,
            "-d",
            "/home/user",
            "-s",
            "/bin/zsh",
            "claudeuser",
        ])
        .status();
    let _ = Command::new("chown")
        .args(["claudeuser:claudegroup", "/home/user"])
        .status();
    let dirs = [
        "/.claude",
        "/.jj",
        "/.local",
        "/.local/bin",
        "/.ssh",
        "/.cache",
        "/.config",
        "/.gnupg",
    ];
    dirs.iter()
        .map(|dir| format!("/home/user{dir}"))
        .filter(|path| std::path::Path::new(path).exists())
        .for_each(|path| {
            let _ = Command::new("chown")
                .args(["-R", "claudeuser:claudegroup", &path])
                .status();
        });
}

pub fn setup_socket_forwarding() {
    if let Ok(sock_path) = std::env::var("SSH_AUTH_SOCK") {
        if sock_path == "/tmp/ssh-agent.sock" {
            let ssh_dir = "/home/user/.ssh";
            let _ = std::fs::create_dir_all(ssh_dir);
        }
    }
    if let Ok(sock_path) = std::env::var("GPG_AGENT_SOCK") {
        if sock_path == "/tmp/gpg-agent.sock" {
            let gpg_dir = "/home/user/.gnupg";
            let _ = std::fs::create_dir_all(gpg_dir);
            let agent_socket = format!("{gpg_dir}/S.gpg-agent");
            let _ = std::fs::remove_file(&agent_socket);
            let _ = std::os::unix::fs::symlink("/tmp/gpg-agent.sock", &agent_socket);
        }
    }
}

pub fn setup_host_home_symlink() -> anyhow::Result<()> {
    let host_home = match std::env::var("HOST_HOME") {
        Ok(val) if val != "/home/user" => val,
        _ => return Ok(()),
    };
    let path = std::path::Path::new(&host_home);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    if !path.exists() {
        std::os::unix::fs::symlink("/home/user", path)?;
    }
    Ok(())
}

pub fn setup_claude_binary() -> anyhow::Result<()> {
    let local_bin = "/home/user/.local/bin";
    let _ = std::fs::create_dir_all(local_bin);
    let target_bin = format!("{local_bin}/claude");
    let sources = [
        "/usr/local/bin/claude",
        "/root/.local/bin/claude",
        "/usr/bin/claude",
    ];
    if sources.iter().any(|s| std::path::Path::new(s).exists()) {
        let script = "#!/bin/sh\nexec /usr/local/bin/claude \"$@\"\n";
        std::fs::write(&target_bin, script)?;
        let mut perms = std::fs::metadata(&target_bin)?.permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&target_bin, perms)?;
    }
    let _ = Command::new("chown")
        .args(["claudeuser:claudegroup", &target_bin])
        .status();
    Ok(())
}

pub fn setup_path() {
    let profile = "/home/user/.profile";
    if std::path::Path::new(profile).exists() {
        return;
    }
    let content = r#"export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/root/.cargo/bin:$PATH"
"#;
    let _ = std::fs::write(profile, content);
}

pub fn cmd_internal_entrypoint(args: &[String]) -> anyhow::Result<()> {
    let uid = std::env::var("CONTAINER_USER_ID").unwrap_or_else(|_| "1000".to_string());
    let gid = std::env::var("CONTAINER_GROUP_ID").unwrap_or_else(|_| "1000".to_string());
    setup_system_user(&uid, &gid);
    setup_socket_forwarding();
    setup_host_home_symlink()?;
    setup_claude_binary()?;
    setup_path();
    let mut exec_args = vec!["claudeuser".to_string()];
    let mut actual_args = args.to_vec();
    if actual_args.first().map(String::as_str) == Some("shell") {
        actual_args.remove(0);
        exec_args.push("/bin/zsh".into());
    } else {
        exec_args.push("/home/user/.local/bin/claude".into());
        exec_args.extend(actual_args);
    }
    let err = Command::new("gosu").args(&exec_args).exec();
    Err(err).context("exec gosu")
}

#[cfg(test)]
pub fn build_claude_exec_args(args: &[String]) -> Vec<String> {
    let mut exec_args = vec!["claudeuser".to_string()];
    let mut actual_args = args.to_vec();
    if actual_args.first().map(String::as_str) == Some("shell") {
        actual_args.remove(0);
        exec_args.push("/bin/zsh".into());
    } else {
        exec_args.push("/home/user/.local/bin/claude".into());
        exec_args.extend(actual_args);
    }
    exec_args
}
