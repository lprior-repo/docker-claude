use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::Context;

pub fn setup_system_user(_uid: &str, _gid: &str) {
    let _user_claude_exists = Command::new("id")
        .args(["claudeuser"])
        .output()
        .is_ok_and(|s| s.status.success());
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
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("WARN: failed to create parent dir: {}", e);
                return Ok(());
            }
        }
    }
    match std::os::unix::fs::symlink("/home/user", path) {
        Ok(_) => Ok(()),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::ReadOnlyFilesystem {
                // Silently skip, as read-only filesystem is expected in secure containers
                Ok(())
            } else {
                Err(e.into())
            }
        }
    }
}

pub fn setup_claude_binary() -> anyhow::Result<()> {
    let local_bin = "/home/user/.local/bin";
    if let Err(e) = std::fs::create_dir_all(local_bin) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            eprintln!("WARN: could not create {}: {}", local_bin, e);
        }
    }
    let target_bin = format!("{local_bin}/claude");
    let sources = [
        "/usr/local/bin/claude",
        "/root/.local/bin/claude",
        "/usr/bin/claude",
    ];
    if let Some(source) = sources.iter().find(|s| std::path::Path::new(s).exists()) {
        let _ = std::fs::remove_file(&target_bin);
        if let Err(e) = std::os::unix::fs::symlink(source, &target_bin) {
            eprintln!("WARN: could not symlink {source} to {target_bin}: {e}");
        }
    }
    Ok(())
}

pub fn setup_path() {
    let profile = "/home/user/.profile";
    if std::path::Path::new(profile).exists() {
        return;
    }
    let content = r#"export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/root/.cargo/bin:$PATH"
"#;
    if let Err(e) = std::fs::write(profile, content) {
        if e.kind() != std::io::ErrorKind::ReadOnlyFilesystem {
            eprintln!("WARN: could not write /home/user/.profile: {}", e);
        }
    }
}

pub fn cmd_internal_entrypoint(args: &[String]) -> anyhow::Result<()> {
    let uid = std::env::var("CONTAINER_USER_ID").unwrap_or_else(|_| "1000".to_string());
    let gid = std::env::var("CONTAINER_GROUP_ID").unwrap_or_else(|_| "1000".to_string());
    setup_system_user(&uid, &gid);
    setup_socket_forwarding();
    setup_host_home_symlink()?;
    setup_claude_binary()?;
    setup_path();
    let mut actual_args: Vec<String> = args.iter().skip_while(|s| *s == "--").cloned().collect();
    let shell_mode = actual_args.first().map(String::as_str) == Some("shell");
    if shell_mode {
        actual_args.remove(0);
    }
    let cmd_args: Vec<String> = if shell_mode {
        let mut zsh_cmd = vec!["/bin/zsh".to_string()];
        zsh_cmd.extend(actual_args);
        zsh_cmd
    } else if actual_args.first().map(String::as_str) == Some("claude") {
        actual_args.remove(0);
        let mut claude_cmd = vec!["/usr/local/bin/claude".to_string()];
        claude_cmd.extend(actual_args);
        claude_cmd
    } else {
        let mut claude_cmd = vec!["/usr/local/bin/claude".to_string()];
        claude_cmd.extend(actual_args);
        claude_cmd
    };
    let err = Command::new(&cmd_args[0]).args(&cmd_args[1..]).exec();
    eprintln!("exec failed: {}", err);
    Err(err).context("exec")
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
