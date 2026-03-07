use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use keyring::Entry;
use std::process::Command;

const SERVICE: &str = "claude-dock";
const ACTIVE_KEY: &str = "__active_profile__";
const MANIFEST_KEY: &str = "__manifest__";
const DEFAULT_IMAGE: &str = "claude-dock:latest";

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
}

#[derive(Subcommand)]
enum KeyAction {
    Add {
        name: String,
        #[arg(short, long)]
        key: Option<String>,
    },
    List,
    Use {
        name: String,
    },
    Remove {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ContainerState {
    Running,
    Stopped,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

struct LaunchInputs<'a> {
    image: &'a str,
    api_key: &'a str,
    project_dir: &'a str,
    base_name: &'a str,
    home: &'a str,
    extra_claude_args: &'a [String],
    nonce: u32,
}

fn store_secret(_profile: &str, _secret: &str) -> Result<()> {
    Entry::new(SERVICE, _profile)?
        .set_password(_secret)
        .context("storing secret")
}

fn load_secret(_profile: &str) -> Result<String> {
    Entry::new(SERVICE, _profile)?
        .get_password()
        .with_context(|| format!("no key for profile '{_profile}'"))
}

fn delete_secret(_profile: &str) -> Result<()> {
    Entry::new(SERVICE, _profile)?
        .delete_credential()
        .with_context(|| format!("deleting '{_profile}'"))
}

fn get_active() -> Result<String> {
    load_secret(ACTIVE_KEY).context("no active profile - run: claude-dock key use <name>")
}

fn register_profile(_name: &str) -> Result<()> {
    let manifest = load_secret(MANIFEST_KEY).unwrap_or_default();
    let mut profiles: Vec<&str> = manifest.split(',').filter(|s| !s.is_empty()).collect();

    if !profiles.contains(&_name) {
        profiles.push(_name);
    }

    store_secret(MANIFEST_KEY, &profiles.join(","))
}

fn new_container_args(
    _image: &str,
    _api_key: &str,
    _project_dir: &str,
    _cname: &str,
    _home: &str,
    _extra_claude_args: &[String],
) -> Vec<String> {
    let uid = std::process::id().to_string();
    let mut args = vec![
        "run".into(),
        "-it".into(),
        "--rm".into(),
        "--name".into(),
        _cname.into(),
        "-v".into(),
        format!("{_project_dir}:/app"),
        "-v".into(),
        format!("{_home}/.claude:/home/user/.claude"),
        "-v".into(),
        format!("{_home}/.gitconfig:/home/user/.gitconfig:ro"),
        "-v".into(),
        format!("{_home}/.git-credentials:/home/user/.git-credentials:ro"),
        "-v".into(),
        format!("{_home}/.jj:/home/user/.jj"),
        "-e".into(),
        format!("ANTHROPIC_API_KEY={_api_key}"),
        "-e".into(),
        format!("CONTAINER_USER_ID={}", uid),
        "-e".into(),
        format!("CONTAINER_GROUP_ID={}", uid),
        "-e".into(),
        "GIT_AUTHOR_NAME=Lewis.Prior".into(),
        "-e".into(),
        "GIT_AUTHOR_EMAIL=priorlewis43@gmail.com".into(),
        "-e".into(),
        "GIT_COMMITTER_NAME=Lewis.Prior".into(),
        "-e".into(),
        "GIT_COMMITTER_EMAIL=priorlewis43@gmail.com".into(),
        _image.into(),
    ];

    args.extend_from_slice(_extra_claude_args);
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

fn parse_container_state(_docker_ps_output: &str) -> ContainerState {
    let text = _docker_ps_output.trim();
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
                args: new_container_args(
                    inputs.image,
                    inputs.api_key,
                    inputs.project_dir,
                    &container_name,
                    inputs.home,
                    inputs.extra_claude_args,
                ),
            }
        }
    }
}

fn probe_container(_cname: &str) -> ContainerState {
    let output = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--format",
            "{{.Names}}\t{{.Status}}",
            "--filter",
            &format!("name=^{}$", _cname),
        ])
        .output();

    match output {
        Ok(result) => parse_container_state(&String::from_utf8_lossy(&result.stdout)),
        Err(_) => ContainerState::Missing,
    }
}

fn cmd_key_add(_name: &str, _key: Option<&str>) -> Result<()> {
    let secret = match _key {
        Some(key) => key.to_owned(),
        None => {
            eprint!("Enter ANTHROPIC_API_KEY for '{}': ", _name.cyan());
            read_password()
        }
    };

    if secret.trim().is_empty() {
        bail!("API key cannot be empty");
    }

    store_secret(_name, secret.trim())?;
    register_profile(_name)?;
    println!(
        "{} Profile '{}' saved to system keychain.",
        "OK".green(),
        _name.cyan()
    );
    Ok(())
}

fn cmd_key_list() -> Result<()> {
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
    Ok(())
}

fn cmd_key_use(_name: &str) -> Result<()> {
    load_secret(_name).with_context(|| {
        format!("Profile '{_name}' not found. Add it: claude-dock key add {_name}")
    })?;
    store_secret(ACTIVE_KEY, _name)?;
    println!("{} Active profile -> '{}'", "OK".green(), _name.cyan());
    Ok(())
}

fn cmd_key_remove(_name: &str) -> Result<()> {
    delete_secret(_name)?;

    let manifest = load_secret(MANIFEST_KEY).unwrap_or_default();
    let updated = manifest
        .split(',')
        .filter(|profile| !profile.is_empty() && *profile != _name)
        .collect::<Vec<_>>()
        .join(",");
    store_secret(MANIFEST_KEY, &updated)?;

    if get_active().unwrap_or_default() == _name {
        let _ = delete_secret(ACTIVE_KEY);
        println!(
            "{} That was the active profile. Set a new one: claude-dock key use <name>",
            "!".yellow()
        );
    }

    println!("{} Profile '{}' removed.", "OK".green(), _name.red());
    Ok(())
}

fn cmd_run(_image: &str, _profile: Option<&str>, _claude_args: &[String]) -> Result<()> {
    which::which("docker").context("'docker' not found - is Docker installed and running?")?;

    let profile_name = match _profile {
        Some(profile) => profile.to_owned(),
        None => get_active()?,
    };

    let api_key = load_secret(&profile_name)
        .with_context(|| format!("Profile '{profile_name}' not found"))?;

    let project_dir = std::env::current_dir().context("cannot read current directory")?;
    let project_str = project_dir.to_string_lossy().into_owned();
    let folder = project_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_string());
    let container_base_name = sanitise_name(&folder);
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());

    println!();
    println!("  {}", "Claude Code  x  Docker".bold().bright_cyan());
    println!();
    println!("  {} {}", "Profile :".dimmed(), profile_name.cyan());
    println!("  {} {}", "Project :".dimmed(), project_str.yellow());
    println!("  {} {}", "Image   :".dimmed(), _image.dimmed());
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

    let plan = resolve_launch_plan(
        probe_container(&container_base_name),
        LaunchInputs {
            image: _image,
            api_key: &api_key,
            project_dir: &project_str,
            base_name: &container_base_name,
            home: &home,
            extra_claude_args: _claude_args,
            nonce: std::process::id(),
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

    use std::os::unix::process::CommandExt;

    let err = Command::new("docker").args(&plan.args).exec();
    Err(err).context("failed to exec docker")
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
            KeyAction::Add { name, key } => cmd_key_add(&name, key.as_deref()),
            KeyAction::List => cmd_key_list(),
            KeyAction::Use { name } => cmd_key_use(&name),
            KeyAction::Remove { name } => cmd_key_remove(&name),
        },
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn new_container_args_launches_claude_with_forwarded_args() {
        let args = new_container_args(
            "ghcr.io/example/claude:latest",
            "sk-ant-123",
            "/tmp/project",
            "claude-demo",
            "/home/tester",
            &["--dangerously-skip-permissions".into(), "--verbose".into()],
        );

        assert_eq!(args[0], "run");
        assert_eq!(args[1], "-it");
        assert_eq!(args[2], "--rm");
        assert_eq!(args[3], "--name");
        assert_eq!(args[4], "claude-demo");
        assert!(args.contains(&"-v".into()) && args.contains(&"/tmp/project:/app".into()));
        assert!(
            args.contains(&"-v".into())
                && args.contains(&"/home/tester/.claude:/home/user/.claude".into())
        );
        let has_api_key = args.iter().position(|a| a == &"-e").map_or(false, |i| {
            args.get(i + 1)
                .map_or(false, |v| v.starts_with("ANTHROPIC_API_KEY="))
        });
        assert!(has_api_key);
        assert!(args.iter().any(|a| a.starts_with("CONTAINER_USER_ID=")));
        assert!(args.iter().any(|a| a.starts_with("CONTAINER_GROUP_ID=")));
        assert!(args.contains(&"ghcr.io/example/claude:latest".into()));
        assert!(args.contains(&"--dangerously-skip-permissions".into()));
        assert!(args.contains(&"--verbose".into()));
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
        let plan = resolve_launch_plan(
            ContainerState::Stopped,
            LaunchInputs {
                image: "ghcr.io/example/claude:latest",
                api_key: "sk-ant-123",
                project_dir: "/tmp/project",
                base_name: "claude-demo",
                home: "/home/tester",
                extra_claude_args: &[],
                nonce: 4242,
            },
        );

        assert_eq!(plan.mode, LaunchMode::Resume);
        assert_eq!(plan.container_name, "claude-demo");
        assert_eq!(plan.args, reattach_args("claude-demo"));
    }

    #[test]
    fn resolve_launch_plan_uses_base_name_for_missing_container() {
        let plan = resolve_launch_plan(
            ContainerState::Missing,
            LaunchInputs {
                image: "ghcr.io/example/claude:latest",
                api_key: "sk-ant-123",
                project_dir: "/tmp/project",
                base_name: "claude-demo",
                home: "/home/tester",
                extra_claude_args: &["--print".into()],
                nonce: 4242,
            },
        );

        assert_eq!(plan.mode, LaunchMode::New);
        assert_eq!(plan.container_name, "claude-demo");
        assert_eq!(plan.args[0], "run");
        assert_eq!(plan.args.last().map(String::as_str), Some("--print"));
    }

    #[test]
    fn resolve_launch_plan_avoids_name_collision_for_running_container() {
        let plan = resolve_launch_plan(
            ContainerState::Running,
            LaunchInputs {
                image: "ghcr.io/example/claude:latest",
                api_key: "sk-ant-123",
                project_dir: "/tmp/project",
                base_name: "claude-demo",
                home: "/home/tester",
                extra_claude_args: &[],
                nonce: 4242,
            },
        );

        assert_eq!(plan.mode, LaunchMode::New);
        assert_eq!(plan.container_name, "claude-demo-4242");
        assert!(plan.args.contains(&"claude-demo-4242".to_string()));
    }

    #[test]
    fn sanitise_name_falls_back_for_empty_results() {
        assert_eq!(sanitise_name("...///***"), "claude-project");
    }
}
