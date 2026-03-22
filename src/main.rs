mod container;
mod entrypoint;
mod keyring;
mod provider;

use std::os::unix::process::CommandExt;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;

use container::{detect_backend, probe_container, resolve_launch_plan, LaunchInputs};
use keyring::{
    cmd_key_add, cmd_key_list, cmd_key_remove, cmd_key_use, get_active, load_profile, KeyAction,
};
use provider::Provider;

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

fn get_git_identity(key: &str) -> Option<String> {
    std::process::Command::new("git")
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
        println!("  PROVIDER={}", config.provider.name());
        match config.provider {
            Provider::Anthropic => println!("  (Vanilla Install - No API keys injected)"),
            _ => println!("  ANTHROPIC_AUTH_TOKEN=[REDACTED]"),
        }
    }

    let uid = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map_or_else(
            |_| "1000".to_string(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
        );
    let gid = std::process::Command::new("id")
        .arg("-g")
        .output()
        .map_or_else(
            |_| "1000".to_string(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
        );

    println!("  CONTAINER_USER_ID={uid}");
    println!("  CONTAINER_GROUP_ID={gid}");
    Ok(())
}

fn cmd_run(image: &str, profile: Option<&str>, claude_args: &[String]) -> Result<()> {
    let backend = detect_backend()?;
    let profile_name = profile.map_or_else(get_active, |p| Ok(p.to_owned()))?;
    let config = load_profile(&profile_name)
        .with_context(|| format!("Profile '{profile_name}' not found"))?;

    let project_dir = std::env::current_dir().context("cannot read current directory")?;
    let project_str = project_dir.to_string_lossy().into_owned();
    let folder = project_dir.file_name().map_or_else(
        || "project".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let container_base_name = container::sanitise_project_name(&folder);
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());

    let uid = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map_or_else(
            |_| "1000".to_string(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
        );
    let gid = std::process::Command::new("id")
        .arg("-g")
        .output()
        .map_or_else(
            |_| "1000".to_string(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
        );

    print_banner(&profile_name, &project_str, image);

    let git_name = get_git_identity("user.name");
    let git_email = get_git_identity("user.email");

    let inputs = LaunchInputs {
        image,
        config: &config,
        project_dir: &project_str,
        base_name: &container_base_name,
        host_home: &home,
        uid: &uid,
        gid: &gid,
        extra_claude_args: claude_args,
        nonce: std::process::id(),
        git_name: git_name.as_deref(),
        git_email: git_email.as_deref(),
    };

    let plan = resolve_launch_plan(probe_container(backend, &container_base_name), inputs);

    if plan.mode == container::LaunchMode::Resume {
        println!(
            "  {} Resuming previous session for '{}'...",
            "~".yellow(),
            folder.cyan()
        );
        println!();
    }

    let mut cmd = std::process::Command::new(backend.binary_name());
    cmd.args(&plan.args);

    // NOTE: Secrets set via Command::env() avoid /proc/PID/cmdline leakage.
    // However, they remain visible via `docker inspect` and /proc/PID/environ
    // inside the container. This is an inherent limitation of Docker env var injection.
    if config.provider.needs_auth_token() {
        cmd.env("ANTHROPIC_AUTH_TOKEN", &config.key);
    }

    let err = cmd.exec();
    Err(err).context(format!("failed to exec {}", backend.binary_name()))
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
        Cmd::Shell { profile, bash_args } => {
            let mut combined = vec!["shell".to_string()];
            combined.extend_from_slice(&bash_args);
            cmd_run(&cli.image, profile.as_deref(), &combined)
        }
        Cmd::InternalEntrypoint { args } => entrypoint::cmd_internal_entrypoint(&args),
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use container::{build_container_args, ContainerBackend, ContainerState, LaunchInputs};
    use provider::{ProfileConfig, Provider};

    fn make_inputs<'a>(config: &'a ProfileConfig, extra: &'a [String]) -> LaunchInputs<'a> {
        LaunchInputs {
            image: "ghcr.io/example/claude:latest",
            config,
            project_dir: "/tmp/project",
            base_name: "claude-demo",
            host_home: "/home/tester",
            uid: "1000",
            gid: "1000",
            extra_claude_args: extra,
            nonce: 42,
            git_name: None,
            git_email: None,
        }
    }

    fn anthropic_config() -> ProfileConfig {
        ProfileConfig {
            key: "sk-ant-123".into(),
            provider: Provider::Anthropic,
        }
    }

    fn minimax_config() -> ProfileConfig {
        ProfileConfig {
            key: "minimax-key".into(),
            provider: Provider::Minimax,
        }
    }

    fn zai_config() -> ProfileConfig {
        ProfileConfig {
            key: "zai-key-123".into(),
            provider: Provider::Zai,
        }
    }

    #[test]
    fn new_container_args_launches_claude_with_forwarded_args() {
        let config = anthropic_config();
        let extra = vec!["--dangerously-skip-permissions".into(), "--verbose".into()];
        let inputs = make_inputs(&config, &extra);
        let args = build_container_args(&inputs, "claude-demo");

        assert_eq!(args[0], "run");
        assert_eq!(args[1], "-it");
        assert_eq!(args[2], "--rm");
        assert_eq!(args[3], "--name");
        assert_eq!(args[4], "claude-demo");
        assert!(args.contains(&"/tmp/project:/app".into()));
        assert!(args.contains(&"/home/tester/.claude:/home/user/.claude".into()));
        assert!(args.contains(&"/home/tester/.claude.json:/home/user/.claude.json".into()));
        assert!(args.windows(2).any(|w| w == ["__entrypoint", "--"]));
        assert!(!args.iter().any(|a| a.starts_with("ANTHROPIC_API_KEY")));
        assert!(args.contains(&"CONTAINER_USER_ID=1000".into()));
        assert!(args.contains(&"--dangerously-skip-permissions".into()));
        assert!(args.contains(&"--verbose".into()));
    }

    #[test]
    fn new_container_args_uses_non_tty_mode_for_print_runs() {
        let config = anthropic_config();
        let extra = vec!["-p".into(), "hello".into()];
        let inputs = make_inputs(&config, &extra);
        let args = build_container_args(&inputs, "claude-demo");

        assert_eq!(args[0], "run");
        assert_eq!(args[1], "-i");
        assert_ne!(args[1], "-it");
    }

    #[test]
    fn new_container_args_supports_minimax_provider() {
        let config = minimax_config();
        let inputs = make_inputs(&config, &[]);
        let args = build_container_args(&inputs, "claude-demo");

        assert!(args.contains(&"ANTHROPIC_AUTH_TOKEN".into()));
        assert!(args.contains(&"ANTHROPIC_BASE_URL=https://api.minimax.io/anthropic".into()));
        assert!(args.contains(&"ANTHROPIC_MODEL=MiniMax-M2.7-highspeed".into()));
        assert!(args.contains(&"API_TIMEOUT_MS=3000000".into()));
        assert!(args.contains(&"CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1".into()));
    }

    #[test]
    fn new_container_args_supports_zai_provider() {
        let config = zai_config();
        let inputs = make_inputs(&config, &[]);
        let args = build_container_args(&inputs, "claude-demo");

        assert!(args.contains(&"ANTHROPIC_AUTH_TOKEN".into()));
        assert!(args.contains(&"ANTHROPIC_BASE_URL=https://api.z.ai/api/anthropic".into()));
        assert!(args.contains(&"ANTHROPIC_DEFAULT_OPUS_MODEL=GLM-5-Turbo".into()));
        assert!(args.contains(&"API_TIMEOUT_MS=3000000".into()));
        assert!(args.contains(&"CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1".into()));
    }

    #[test]
    fn reattach_args_attach_to_existing_container() {
        assert_eq!(
            container::reattach_container_args("claude-demo"),
            vec!["start", "-ai", "claude-demo"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn sanitise_name_replaces_non_identifier_characters() {
        assert_eq!(
            container::sanitise_project_name("my cool/project.v1"),
            "claude-my-cool-project-v1"
        );
    }

    #[test]
    fn parse_container_state_distinguishes_missing_running_and_stopped() {
        assert_eq!(
            probe_container(ContainerBackend::Docker, "nonexistent-test-xyz"),
            ContainerState::Missing
        );
    }

    #[test]
    fn resolve_launch_plan_resumes_stopped_container() {
        let config = anthropic_config();
        let inputs = make_inputs(&config, &[]);
        let plan = resolve_launch_plan(ContainerState::Stopped, inputs);
        assert_eq!(plan.mode, container::LaunchMode::Resume);
        assert_eq!(plan.container_name, "claude-demo");
    }

    #[test]
    fn resolve_launch_plan_uses_base_name_for_missing_container() {
        let config = anthropic_config();
        let extra = ["--print".to_string()];
        let inputs = make_inputs(&config, &extra);
        let plan = resolve_launch_plan(ContainerState::Missing, inputs);
        assert_eq!(plan.mode, container::LaunchMode::New);
        assert_eq!(plan.container_name, "claude-demo");
        assert_eq!(plan.args.last().map(String::as_str), Some("--print"));
    }

    #[test]
    fn resolve_launch_plan_avoids_name_collision_for_running_container() {
        let config = anthropic_config();
        let inputs = make_inputs(&config, &[]);
        let plan = resolve_launch_plan(ContainerState::Running, inputs);
        assert_eq!(plan.mode, container::LaunchMode::New);
        assert_eq!(plan.container_name, "claude-demo-42");
        assert!(plan.args.contains(&"claude-demo-42".to_string()));
    }

    #[test]
    fn new_container_args_forwards_git_identity() {
        let config = anthropic_config();
        let mut inputs = make_inputs(&config, &[]);
        inputs.git_name = Some("Test User");
        inputs.git_email = Some("test@example.com");
        let args = build_container_args(&inputs, "claude-demo");

        assert!(args.contains(&"GIT_AUTHOR_NAME=Test User".into()));
        assert!(args.contains(&"GIT_AUTHOR_EMAIL=test@example.com".into()));
        assert!(args.contains(&"GIT_COMMITTER_NAME=Test User".into()));
        assert!(args.contains(&"GIT_COMMITTER_EMAIL=test@example.com".into()));
    }

    #[test]
    fn new_container_args_anthropic_provider_injects_no_secrets() {
        let config = anthropic_config();
        let inputs = make_inputs(&config, &[]);
        let args = build_container_args(&inputs, "claude-demo");

        assert!(args.contains(&"HOST_HOME=/home/tester".into()));
        assert!(!args.iter().any(|a| a.contains("ANTHROPIC_API_KEY")));
        assert!(!args.iter().any(|a| a.contains("ANTHROPIC_AUTH_TOKEN")));
    }

    #[test]
    fn sanitise_name_falls_back_for_empty_results() {
        assert_eq!(
            container::sanitise_project_name("...///***"),
            "claude-project"
        );
    }

    #[test]
    fn provider_from_str_rejects_unknown() {
        assert!(Provider::from_str_lossy("unknown_provider").is_err());
    }

    #[test]
    fn provider_needs_auth_token() {
        assert!(!Provider::Anthropic.needs_auth_token());
        assert!(Provider::Minimax.needs_auth_token());
        assert!(Provider::Zai.needs_auth_token());
    }
}
