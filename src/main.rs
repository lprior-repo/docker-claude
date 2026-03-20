use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::os::unix::process::CommandExt;
use std::process::Command;

const SERVICE: &str = "claude-dock";
const ACTIVE_KEY: &str = "__active_profile__";
const MANIFEST_KEY: &str = "__manifest__";
const DEFAULT_IMAGE: &str = "claude-dock:latest";

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ProfileConfig {
    key: String,
    provider: String, // "anthropic" or "minimax"
}

#[derive(Parser)]
#[command(
    name = "claude-dock",
    about = "Launch Claude Code in Docker - automatically",
    version
)]
struct Cli {
    #[arg(long, global = true, env = "CLAUDE_IMAGE", default_value = DEFAULT_IMAGE)]
    image: String,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Run {
        #[arg(short, long)]
        profile: Option<String>,
        #[arg(last = true)]
        claude_args: Vec<String>,
    },
    Key {
        #[command(subcommand)]
        action: KeyAction,
    },
    Config,
    Shell {
        #[arg(short, long)]
        profile: Option<String>,
        #[arg(last = true)]
        bash_args: Vec<String>,
    },
    #[command(hide = true, name = "__entrypoint")]
    InternalEntrypoint {
        #[arg(last = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum KeyAction {
    Add {
        name: String,
        #[arg(short, long)]
        key: Option<String>,
        #[arg(short, long, default_value = "anthropic")]
        provider: String,
    },
    List,
    Use {
        name: String,
    },
    Remove {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
enum ContainerState {
    Running,
    Stopped,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
enum LaunchMode {
    New,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchPlan {
    mode: LaunchMode,
    container_name: String,
    args: Vec<String>,
}

#[derive(Debug, Clone)]
struct LaunchInputs<'a> {
    image: &'a str,
    config: &'a ProfileConfig,
    project_dir: &'a str,
    base_name: &'a str,
    host_home: &'a str,
    uid: &'a str,
    gid: &'a str,
    extra_claude_args: &'a [String],
    nonce: u32,
    git_name: Option<String>,
    git_email: Option<String>,
}

fn load_profile(profile: &str) -> Result<ProfileConfig> {
    let raw = load_secret(profile)?;
    serde_json::from_str::<ProfileConfig>(&raw).map_or_else(
        |_| {
            Ok(ProfileConfig {
                key: raw,
                provider: "anthropic".to_string(),
            })
        },
        Ok,
    )
}

fn store_secret(profile: &str, secret: &str) -> Result<()> {
    Entry::new(SERVICE, profile)?
        .set_password(secret)
        .context("storing secret")
}

fn load_secret(profile: &str) -> Result<String> {
    Entry::new(SERVICE, profile)?
        .get_password()
        .with_context(|| format!("no key for profile '{profile}'"))
}

fn delete_secret(profile: &str) -> Result<()> {
    Entry::new(SERVICE, profile)?
        .delete_credential()
        .with_context(|| format!("deleting '{profile}'"))
}

fn get_active() -> Result<String> {
    load_secret(ACTIVE_KEY).context("no active profile - run: claude-dock key use <name>")
}

fn register_profile(name: &str) -> Result<()> {
    let manifest = load_secret(MANIFEST_KEY).unwrap_or_default();
    let mut profiles: Vec<&str> = manifest.split(',').filter(|s| !s.is_empty()).collect();

    if !profiles.contains(&name) {
        profiles.push(name);
    }

    store_secret(MANIFEST_KEY, &profiles.join(","))
}

fn new_container_args(inputs: &LaunchInputs<'_>, cname: &str) -> Vec<String> {
    let mut args = vec![
        "run".into(),
        "-it".into(),
        "--rm".into(),
        "--name".into(),
        cname.into(),
        "--entrypoint".into(),
        "/usr/local/bin/claude-dock".into(),
        "-v".into(),
        format!("{}:/app", inputs.project_dir),
        "-v".into(),
        format!("{}/.claude:/home/user/.claude", inputs.host_home),
        "-v".into(),
        format!("{}/.gitconfig:/home/user/.gitconfig:ro", inputs.host_home),
        "-v".into(),
        format!(
            "{}/.git-credentials:/home/user/.git-credentials:ro",
            inputs.host_home
        ),
        "-v".into(),
        format!("{}/.jj:/home/user/.jj", inputs.host_home),
        "-e".into(),
        format!("HOST_HOME={}", inputs.host_home),
        "-e".into(),
        format!("CONTAINER_USER_ID={}", inputs.uid),
        "-e".into(),
        format!("CONTAINER_GROUP_ID={}", inputs.gid),
    ];

    if let Some(ref name) = inputs.git_name {
        args.extend(["-e".into(), format!("GIT_AUTHOR_NAME={name}")]);
        args.extend(["-e".into(), format!("GIT_COMMITTER_NAME={name}")]);
    }

    if let Some(ref email) = inputs.git_email {
        args.extend(["-e".into(), format!("GIT_AUTHOR_EMAIL={email}")]);
        args.extend(["-e".into(), format!("GIT_COMMITTER_EMAIL={email}")]);
    }

    match inputs.config.provider.as_str() {
        "minimax" => {
            args.extend([
                "-e".into(),
                "ANTHROPIC_BASE_URL=https://api.minimax.io/anthropic".into(),
                "-e".into(),
                "ANTHROPIC_AUTH_TOKEN".into(),
                "-e".into(),
                "ANTHROPIC_MODEL=MiniMax-M2.5-highspeed".into(),
                "-e".into(),
                "ANTHROPIC_SMALL_FAST_MODEL=MiniMax-M2.5-highspeed".into(),
                "-e".into(),
                "ANTHROPIC_DEFAULT_SONNET_MODEL=MiniMax-M2.5-highspeed".into(),
                "-e".into(),
                "ANTHROPIC_DEFAULT_OPUS_MODEL=MiniMax-M2.5-highspeed".into(),
                "-e".into(),
                "ANTHROPIC_DEFAULT_HAIKU_MODEL=MiniMax-M2.5-highspeed".into(),
                "-e".into(),
                "API_TIMEOUT_MS=3000000".into(),
                "-e".into(),
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1".into(),
            ]);
        }
        _ => {}
    }

    args.push(inputs.image.into());
    args.push("__entrypoint".into());
    args.extend_from_slice(inputs.extra_claude_args);
    args
}

fn reattach_args(cname: &str) -> Vec<String> {
    vec!["start".into(), "-ai".into(), cname.into()]
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

fn get_git_identity(key: &str) -> Option<String> {
    Command::new("git")
        .args(["config", "--get", key])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                (!s.is_empty()).then_some(s)
            } else {
                None
            }
        })
}

fn parse_container_state(docker_ps_output: &str) -> ContainerState {
    let text = docker_ps_output.trim();
    if text.is_empty() {
        ContainerState::Missing
    } else if text.lines().any(|line| line.contains("\tUp ")) {
        ContainerState::Running
    } else {
        ContainerState::Stopped
    }
}

fn resolve_launch_plan(state: ContainerState, inputs: LaunchInputs<'_>) -> LaunchPlan {
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

fn probe_container(backend: &str, cname: &str) -> ContainerState {
    let output = Command::new(backend)
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
        parse_container_state(&String::from_utf8_lossy(&result.stdout))
    })
}

fn cmd_key_add(name: &str, key: Option<&str>, provider: &str) -> Result<()> {
    let secret_key = key.map_or_else(
        || {
            eprint!("Enter key for '{name}' (provider: {provider}): ");
            read_password()
        },
        std::borrow::ToOwned::to_owned,
    );

    if secret_key.trim().is_empty() {
        bail!("Key cannot be empty");
    }

    let config = ProfileConfig {
        key: secret_key.trim().to_owned(),
        provider: provider.to_owned(),
    };
    let secret_json = serde_json::to_string(&config).context("serialising config")?;
    store_secret(name, &secret_json)?;
    register_profile(name)?;
    println!(
        "{} Profile '{}' saved to system keychain.",
        "OK".green(),
        name.cyan()
    );
    Ok(())
}

fn cmd_key_list() {
    let manifest = load_secret(MANIFEST_KEY).unwrap_or_default();
    let active = get_active().unwrap_or_default();

    println!();
    println!("{}", "Saved profiles:".bold());
    if manifest.is_empty() {
        println!("  {}", "(none - run: claude-dock key add <name>)".dimmed());
    } else {
        for profile in manifest.split(',').filter(|s| !s.is_empty()) {
            if profile == active {
                println!(
                    "  {} {} {}",
                    "*".green(),
                    profile.green().bold(),
                    "(active)".dimmed()
                );
            } else {
                println!("    {profile}");
            }
        }
    }
    println!();
}

fn cmd_key_use(name: &str) -> Result<()> {
    load_secret(name).with_context(|| {
        format!("Profile '{name}' not found. Add it: claude-dock key add {name}")
    })?;
    store_secret(ACTIVE_KEY, name)?;
    println!("{} Active profile -> '{}'", "OK".green(), name.cyan());
    Ok(())
}

fn cmd_key_remove(name: &str) -> Result<()> {
    delete_secret(name)?;

    let manifest = load_secret(MANIFEST_KEY).unwrap_or_default();
    let updated = manifest
        .split(',')
        .filter(|profile| !profile.is_empty() && *profile != name)
        .collect::<Vec<_>>()
        .join(",");

    if updated.is_empty() {
        let _ = delete_secret(MANIFEST_KEY);
    } else {
        store_secret(MANIFEST_KEY, &updated)?;
    }

    if get_active().unwrap_or_default() == name {
        let _ = delete_secret(ACTIVE_KEY);
        println!(
            "{} That was the active profile. Set a new one: claude-dock key use <name>",
            "!".yellow()
        );
    }

    println!("{} Profile '{}' removed.", "OK".green(), name.red());
    Ok(())
}

fn print_banner(profile_name: &str, project_str: &str, image: &str) {
    println!();
    println!("  {}", "Claude Code  x  Docker".bold().bright_cyan());
    println!();
    println!("  {} {}", "Profile :".dimmed(), profile_name.cyan());
    println!("  {} {}", "Project :".dimmed(), project_str.yellow());
    println!("  {} {}", "Image   :".dimmed(), image.dimmed());
    println!();
    println!("  {}", "What is Claude Code?".bold());
    println!("  Claude Code is an AI coding agent that lives in your terminal.");
    println!("  It reads your whole codebase, runs commands, edits files, and");
    println!("  explains code - all through plain English conversation.");
    println!();
    println!("  {}", "How to use it:".bold());
    println!("  Just describe what you want. Examples:");
    println!(
        "    {} {}",
        ">".bright_cyan(),
        "\"Refactor this module to use async/await\"".green()
    );
    println!(
        "    {} {}",
        ">".bright_cyan(),
        "\"Add tests for the payment service\"".green()
    );
    println!(
        "    {} {}",
        ">".bright_cyan(),
        "\"Explain what this function does\"".green()
    );
    println!(
        "    {} {}",
        ">".bright_cyan(),
        "\"Fix the failing CI build\"".green()
    );
    println!();
    println!(
        "  Press {} to approve actions, {} to skip, {} to quit Claude Code.",
        "y".bold(),
        "n".bold(),
        "Ctrl-C".bold()
    );
    println!("  Type {} to leave the container entirely.", "exit".bold());
    println!();
}

fn setup_system_user(uid: &str, gid: &str) {
    // Group/User creation - errors are tolerated if they already exist
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

    // Chown home (suppress errors for RO mounts)
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

fn setup_host_home_symlink() -> Result<()> {
    let host_home = match std::env::var("HOST_HOME") {
        Ok(val) if val != "/home/user" => val,
        _ => return Ok(()),
    };

    let path = std::path::Path::new(&host_home);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
    }

    if !path.exists() {
        std::os::unix::fs::symlink("/home/user", path)
            .with_context(|| format!("failed to symlink /home/user to {}", path.display()))?;
    }

    Ok(())
}

fn setup_claude_binary() -> Result<()> {
    let local_bin = "/home/user/.local/bin";
    let _ = std::fs::create_dir_all(local_bin);
    let target_bin = format!("{local_bin}/claude");

    // Force link the binary to ~/.local/bin/claude to satisfy "installMethod: native"
    let sources = [
        "/usr/local/bin/claude",
        "/root/.local/bin/claude",
        "/usr/bin/claude",
    ];

    if let Some(src) = sources.iter().find(|s| std::path::Path::new(s).exists()) {
        if std::path::Path::new(&target_bin).exists() {
            let _ = std::fs::remove_file(&target_bin);
        }
        std::os::unix::fs::symlink(src, &target_bin)
            .with_context(|| format!("failed to symlink {src} to {target_bin}"))?;
    }

    let _ = Command::new("chown")
        .args(["claudeuser:claudegroup", &target_bin])
        .status();

    Ok(())
}

fn cmd_internal_entrypoint(args: &[String]) -> Result<()> {
    let uid = std::env::var("CONTAINER_USER_ID").unwrap_or_else(|_| "1000".to_string());
    let gid = std::env::var("CONTAINER_GROUP_ID").unwrap_or_else(|_| "1000".to_string());

    setup_system_user(&uid, &gid);
    setup_host_home_symlink()?;
    setup_claude_binary()?;

    // 4. Exec via gosu
    let mut exec_args = vec!["claudeuser".to_string()];
    let mut actual_args = args.to_vec();

    if actual_args.first().map(String::as_str) == Some("shell") {
        actual_args.remove(0);
        exec_args.push("/bin/bash".into());
    } else {
        // Use absolute path to avoid PATH issues
        exec_args.push("/usr/local/bin/claude".into());
    }
    exec_args.extend(actual_args);

    let err = Command::new("gosu").args(&exec_args).exec();
    Err(err).context("exec gosu")
}

fn cmd_shell(image: &str, profile: Option<&str>, bash_args: &[String]) -> Result<()> {
    let mut combined_args = vec!["shell".to_string()];
    combined_args.extend_from_slice(bash_args);
    cmd_run(image, profile, &combined_args)
}

fn cmd_config(image: &str) -> Result<()> {
    let active = get_active().unwrap_or_else(|_| "none".to_string());
    let project_dir = std::env::current_dir().context("cannot read current directory")?;
    let host_home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());

    println!("{}: {}", "Active Profile".bold(), active.cyan());
    println!(
        "{}: {}",
        "Project Root  ".bold(),
        project_dir.display().to_string().yellow()
    );
    println!("{}: {}", "Host Home     ".bold(), host_home.yellow());
    println!("{}: {}", "Docker Image  ".bold(), image.dimmed());
    println!();
    println!("{}", "Container Environment:".bold());

    if let Ok(config) = load_profile(&active) {
        println!("  PROVIDER={}", config.provider);
        match config.provider.as_str() {
            "minimax" => println!("  ANTHROPIC_AUTH_TOKEN=[REDACTED]"),
            "anthropic" => println!("  (Vanilla Install - No API keys injected)"),
            _ => println!("  ANTHROPIC_API_KEY=[REDACTED]"),
        }
    }

    let uid = Command::new("id").arg("-u").output().map_or_else(
        |_| "1000".to_string(),
        |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
    );
    let gid = Command::new("id").arg("-g").output().map_or_else(
        |_| "1000".to_string(),
        |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
    );

    println!("  CONTAINER_USER_ID={uid}");
    println!("  CONTAINER_GROUP_ID={gid}");
    Ok(())
}

fn cmd_run(image: &str, profile: Option<&str>, claude_args: &[String]) -> Result<()> {
    let backend = if let Ok(be) = std::env::var("CLAUDE_BACKEND") {
        be
    } else if which::which("docker").is_ok() {
        "docker".to_string()
    } else if which::which("podman").is_ok() {
        "podman".to_string()
    } else {
        bail!("'docker' or 'podman' not found - is a container engine installed and running?");
    };

    let profile_name = profile.map_or_else(get_active, |p| Ok(p.to_owned()))?;

    let config = load_profile(&profile_name)
        .with_context(|| format!("Profile '{profile_name}' not found"))?;

    let project_dir = std::env::current_dir().context("cannot read current directory")?;
    let project_str = project_dir.to_string_lossy().into_owned();
    let folder = project_dir.file_name().map_or_else(
        || "project".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let container_base_name = sanitise_name(&folder);
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());

    let uid = Command::new("id").arg("-u").output().map_or_else(
        |_| "1000".to_string(),
        |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
    );

    let gid = Command::new("id").arg("-g").output().map_or_else(
        |_| "1000".to_string(),
        |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
    );

    print_banner(&profile_name, &project_str, image);

    let git_name = get_git_identity("user.name");
    let git_email = get_git_identity("user.email");

    let plan = resolve_launch_plan(
        probe_container(&backend, &container_base_name),
        LaunchInputs {
            image,
            config: &config,
            project_dir: &project_str,
            base_name: &container_base_name,
            host_home: &home,
            uid: &uid,
            gid: &gid,
            extra_claude_args: claude_args,
            nonce: std::process::id(),
            git_name,
            git_email,
        },
    );

    if plan.mode == LaunchMode::Resume {
        println!(
            "  {} Resuming previous session for '{}'...",
            "~".yellow(),
            folder.cyan()
        );
        println!();
    }

    let mut cmd = Command::new(&backend);
    cmd.args(&plan.args);

    // Set sensitive environment variables on the command itself to avoid leaking them in ps/logs
    match config.provider.as_str() {
        "minimax" => {
            cmd.env("ANTHROPIC_AUTH_TOKEN", &config.key);
        }
        "anthropic" => {
            // For anthropic, we don't pass any env vars, keeping it a "vanilla" install.
            // The user can manage login/keys inside the container or via ~/.claude/settings.json.
        }
        _ => {
            cmd.env("ANTHROPIC_API_KEY", &config.key);
        }
    }

    let err = cmd.exec();
    Err(err).context(format!("failed to exec {backend}"))
}

fn read_password() -> String {
    use std::io::{self, Write};

    let _ = Command::new("stty").arg("-echo").status();
    let _ = io::stdout().flush();

    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);

    let _ = Command::new("stty").arg("echo").status();
    println!();
    buf.trim().to_owned()
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Cmd::Run {
            profile,
            claude_args,
        } => cmd_run(&cli.image, profile.as_deref(), &claude_args),
        Cmd::Key { action } => match action {
            KeyAction::Add {
                name,
                key,
                provider,
            } => cmd_key_add(&name, key.as_deref(), &provider),
            KeyAction::List => {
                cmd_key_list();
                Ok(())
            }
            KeyAction::Use { name } => cmd_key_use(&name),
            KeyAction::Remove { name } => cmd_key_remove(&name),
        },
        Cmd::Config => cmd_config(&cli.image),
        Cmd::Shell { profile, bash_args } => cmd_shell(&cli.image, profile.as_deref(), &bash_args),
        Cmd::InternalEntrypoint { args } => cmd_internal_entrypoint(&args),
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn new_container_args_launches_claude_with_forwarded_args() {
        let config = ProfileConfig {
            key: "sk-ant-123".into(),
            provider: "anthropic".into(),
        };
        let extra_claude_args = ["--dangerously-skip-permissions".into(), "--verbose".into()];
        let inputs = LaunchInputs {
            image: "ghcr.io/example/claude:latest",
            config: &config,
            project_dir: "/tmp/project",
            base_name: "claude-demo",
            host_home: "/home/tester",
            uid: "1000",
            gid: "1000",
            extra_claude_args: &extra_claude_args,
            nonce: 42,
            git_name: None,
            git_email: None,
        };
        let args = new_container_args(&inputs, "claude-demo");

        assert_eq!(args[0], "run");
        assert_eq!(args[1], "-it");
        assert_eq!(args[2], "--rm");
        assert_eq!(args[3], "--name");
        assert_eq!(args[4], "claude-demo");
        assert_eq!(args[5], "--entrypoint");
        assert_eq!(args[6], "/usr/local/bin/claude-dock");
        assert!(args.contains(&"-v".into()) && args.contains(&"/tmp/project:/app".into()));
        assert!(
            args.contains(&"-v".into())
                && args.contains(&"/home/tester/.claude:/home/user/.claude".into())
        );
        // NO API KEY for vanilla anthropic
        assert!(!args.contains(&"ANTHROPIC_API_KEY".into()));
        assert!(args.contains(&"CONTAINER_USER_ID=1000".into()));
        assert!(args.contains(&"CONTAINER_GROUP_ID=1000".into()));
        assert!(args.contains(&"ghcr.io/example/claude:latest".into()));
        assert!(args.contains(&"--dangerously-skip-permissions".into()));
        assert!(args.contains(&"--verbose".into()));
    }

    #[test]
    fn new_container_args_supports_minimax_provider() {
        let config = ProfileConfig {
            key: "minimax-key".into(),
            provider: "minimax".into(),
        };
        let inputs = LaunchInputs {
            image: "ghcr.io/example/claude:latest",
            config: &config,
            project_dir: "/tmp/project",
            base_name: "claude-demo",
            host_home: "/home/tester",
            uid: "1000",
            gid: "1000",
            extra_claude_args: &[],
            nonce: 42,
            git_name: None,
            git_email: None,
        };
        let args = new_container_args(&inputs, "claude-demo");

        assert!(args.contains(&"-e".into()));
        assert!(args.contains(&"ANTHROPIC_AUTH_TOKEN".into()));
        assert!(args.contains(&"ANTHROPIC_BASE_URL=https://api.minimax.io/anthropic".into()));
        assert!(args.contains(&"ANTHROPIC_MODEL=MiniMax-M2.5-highspeed".into()));
        assert!(args.contains(&"API_TIMEOUT_MS=3000000".into()));
        assert!(args.contains(&"CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1".into()));
    }

    #[test]
    fn reattach_args_attach_to_existing_container() {
        assert_eq!(
            reattach_args("claude-demo"),
            vec!["start", "-ai", "claude-demo"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn sanitise_name_replaces_non_identifier_characters() {
        assert_eq!(
            sanitise_name("my cool/project.v1"),
            "claude-my-cool-project-v1"
        );
    }

    #[test]
    fn parse_container_state_distinguishes_missing_running_and_stopped() {
        assert_eq!(parse_container_state(""), ContainerState::Missing);
        assert_eq!(
            parse_container_state("claude-demo\tUp 3 minutes\n"),
            ContainerState::Running
        );
        assert_eq!(
            parse_container_state("claude-demo\tExited (0) 4 hours ago\n"),
            ContainerState::Stopped
        );
    }

    #[test]
    fn parse_container_state_treats_non_up_status_as_stopped() {
        assert_eq!(
            parse_container_state("claude-demo\tCreated\n"),
            ContainerState::Stopped
        );
    }

    #[test]
    fn resolve_launch_plan_resumes_stopped_container() {
        let config = ProfileConfig {
            key: "sk-ant-123".into(),
            provider: "anthropic".into(),
        };
        let plan = resolve_launch_plan(
            ContainerState::Stopped,
            LaunchInputs {
                image: "ghcr.io/example/claude:latest",
                config: &config,
                project_dir: "/tmp/project",
                base_name: "claude-demo",
                host_home: "/home/tester",
                uid: "1000",
                gid: "1000",
                extra_claude_args: &[],
                nonce: 4242,
                git_name: None,
                git_email: None,
            },
        );

        assert_eq!(plan.mode, LaunchMode::Resume);
        assert_eq!(plan.container_name, "claude-demo");
        assert_eq!(plan.args, reattach_args("claude-demo"));
    }

    #[test]
    fn resolve_launch_plan_uses_base_name_for_missing_container() {
        let config = ProfileConfig {
            key: "sk-ant-123".into(),
            provider: "anthropic".into(),
        };
        let plan = resolve_launch_plan(
            ContainerState::Missing,
            LaunchInputs {
                image: "ghcr.io/example/claude:latest",
                config: &config,
                project_dir: "/tmp/project",
                base_name: "claude-demo",
                host_home: "/home/tester",
                uid: "1000",
                gid: "1000",
                extra_claude_args: &["--print".into()],
                nonce: 4242,
                git_name: None,
                git_email: None,
            },
        );

        assert_eq!(plan.mode, LaunchMode::New);
        assert_eq!(plan.container_name, "claude-demo");
        assert_eq!(plan.args[0], "run");
        assert_eq!(plan.args.last().map(String::as_str), Some("--print"));
    }

    #[test]
    fn resolve_launch_plan_avoids_name_collision_for_running_container() {
        let config = ProfileConfig {
            key: "sk-ant-123".into(),
            provider: "anthropic".into(),
        };
        let plan = resolve_launch_plan(
            ContainerState::Running,
            LaunchInputs {
                image: "ghcr.io/example/claude:latest",
                config: &config,
                project_dir: "/tmp/project",
                base_name: "claude-demo",
                host_home: "/home/tester",
                uid: "1000",
                gid: "1000",
                extra_claude_args: &[],
                nonce: 4242,
                git_name: None,
                git_email: None,
            },
        );

        assert_eq!(plan.mode, LaunchMode::New);
        assert_eq!(plan.container_name, "claude-demo-4242");
        assert!(plan.args.contains(&"claude-demo-4242".to_string()));
    }

    #[test]
    fn new_container_args_forwards_git_identity() {
        let config = ProfileConfig {
            key: "sk-ant-123".into(),
            provider: "anthropic".into(),
        };
        let inputs = LaunchInputs {
            image: "ghcr.io/example/claude:latest",
            config: &config,
            project_dir: "/tmp/project",
            base_name: "claude-demo",
            host_home: "/home/tester",
            uid: "1000",
            gid: "1000",
            extra_claude_args: &[],
            nonce: 42,
            git_name: Some("Test User".into()),
            git_email: Some("test@example.com".into()),
        };
        let args = new_container_args(&inputs, "claude-demo");

        assert!(args.contains(&"GIT_AUTHOR_NAME=Test User".into()));
        assert!(args.contains(&"GIT_AUTHOR_EMAIL=test@example.com".into()));
        assert!(args.contains(&"GIT_COMMITTER_NAME=Test User".into()));
        assert!(args.contains(&"GIT_COMMITTER_EMAIL=test@example.com".into()));
    }

    #[test]
    fn new_container_args_anthropic_provider_injects_no_secrets() {
        let config = ProfileConfig {
            key: "sk-ant-123".into(),
            provider: "anthropic".into(),
        };
        let inputs = LaunchInputs {
            image: "ghcr.io/example/claude:latest",
            config: &config,
            project_dir: "/tmp/project",
            base_name: "claude-demo",
            host_home: "/home/tester",
            uid: "1000",
            gid: "1000",
            extra_claude_args: &[],
            nonce: 42,
            git_name: None,
            git_email: None,
        };
        let args = new_container_args(&inputs, "claude-demo");

        // Should still contain infrastructure env vars like HOST_HOME, but NO Anthropic secrets
        assert!(args.contains(&"HOST_HOME=/home/tester".into()));
        assert!(!args.contains(&"ANTHROPIC_API_KEY".into()));
        assert!(!args.contains(&"ANTHROPIC_AUTH_TOKEN".into()));
    }

    #[test]
    fn sanitise_name_falls_back_for_empty_results() {
        assert_eq!(sanitise_name("...///***"), "claude-project");
    }
}
