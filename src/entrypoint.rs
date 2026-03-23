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
            "/bin/bash",
            "claudeuser",
        ])
        .status();
    let _ = Command::new("chown")
        .args(["claudeuser:claudegroup", "/home/user"])
        .status();
    ["/.claude", "/.jj", "/.local", "/.local/bin"]
        .iter()
        .map(|dir| format!("/home/user{dir}"))
        .filter(|path| std::path::Path::new(path).exists())
        .for_each(|path| {
            let _ = Command::new("chown")
                .args(["-R", "claudeuser:claudegroup", &path])
                .status();
        });
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
    if let Some(src) = sources.iter().find(|s| std::path::Path::new(s).exists()) {
        if std::path::Path::new(&target_bin).exists() {
            let _ = std::fs::remove_file(&target_bin);
        }
        std::os::unix::fs::symlink(src, &target_bin)?;
    }
    let _ = Command::new("chown")
        .args(["claudeuser:claudegroup", &target_bin])
        .status();
    Ok(())
}

pub fn cmd_internal_entrypoint(args: &[String]) -> anyhow::Result<()> {
    let uid = std::env::var("CONTAINER_USER_ID").unwrap_or_else(|_| "1000".to_string());
    let gid = std::env::var("CONTAINER_GROUP_ID").unwrap_or_else(|_| "1000".to_string());
    setup_system_user(&uid, &gid);
    setup_host_home_symlink()?;
    setup_claude_binary()?;
    let mut exec_args = vec!["claudeuser".to_string()];
    let mut actual_args = args.to_vec();
    if actual_args.first().map(String::as_str) == Some("shell") {
        actual_args.remove(0);
        exec_args.push("/bin/bash".into());
    } else {
        exec_args.push("/usr/local/bin/claude".into());
        exec_args.push("--dangerously-skip-permissions".into());
    }
    exec_args.extend(actual_args);
    let err = Command::new("gosu").args(&exec_args).exec();
    Err(err).context("exec gosu")
}
