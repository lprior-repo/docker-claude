mod container;
mod entrypoint;
mod keyring;
mod provider;
mod validation;

use std::os::unix::process::CommandExt;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;

use container::{
    cmd_cleanup, cmd_ps, cmd_stop, detect_backend, probe_container, resolve_launch_plan,
    sanitise_project_name, CacheMode, EnvMode, GpuMode, HostAccess, LaunchInputs,
};
use keyring::{
    cmd_key_add, cmd_key_list, cmd_key_remove, cmd_key_use, get_active, load_profile, KeyAction,
};
use provider::Provider;

const DEFAULT_IMAGE: &str = "claude-dock:latest";

#[derive(Parser)]
#[command(
    name = "claude-dock",
    about = "Launch Claude Code in Docker - automatically",
    version,
    subcommand_required = false
)]
struct Cli {
    #[arg(long, global = true, env = "CLAUDE_IMAGE", default_value = DEFAULT_IMAGE)]
    image: String,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    Run {
        #[arg(short, long)]
        profile: Option<String>,
        #[arg(short = 'P', long = "port")]
        ports: Vec<String>,
        #[arg(short = 'm', long = "mount")]
        extra_mounts: Vec<String>,
        #[arg(short = 'M', long = "memory", default_value = "")]
        memory: String,
        #[arg(long = "cpus", default_value = "")]
        cpus: String,
        #[arg(long = "gpus")]
        gpus: bool,
        #[arg(long = "host-access")]
        host_access: bool,
        #[arg(long = "no-cache")]
        no_cache: bool,
        #[arg(long = "no-env")]
        no_env: bool,
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
        #[arg(short = 'P', long = "port")]
        ports: Vec<String>,
        #[arg(short = 'm', long = "mount")]
        extra_mounts: Vec<String>,
        #[arg(long = "host-access")]
        host_access: bool,
        #[arg(long = "no-env")]
        no_env: bool,
        #[arg(last = true)]
        bash_args: Vec<String>,
    },
    Stop {
        name: Option<String>,
    },
    Clean,
    Ps,
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

fn get_container_name() -> Option<String> {
    let project_dir = std::env::current_dir().ok()?;
    let folder = project_dir.file_name()?.to_string_lossy();
    Some(sanitise_project_name(&folder))
}

fn print_banner(profile_name: &str, project_str: &str, image: &str) {
    println!();
    println!("  {}", "Claude Code  x  Docker".bold().bright_cyan());
    println!();
    println!("  {} {}", "Profile :".dimmed(), profile_name.cyan());
    println!("  {} {}", "Project :".dimmed(), project_str.yellow());
    println!("  {} {}", "Image   :".dimmed(), image.dimmed());
    println!();
    println!("  {}", "Hello, gorgeous.".dimmed());
    println!("  Claude Code is ready to work.");
    println!("  It reads the codebase, runs commands, edits files, and");
    println!("  helps you steer everything through plain English conversation.");
    println!();
    println!("  {}", "Tell Claude what you want:".bold());
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
        "  Press {} to approve actions, {} to skip, {} to leave the moment.",
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

    println!("  UID={uid} GID={gid} (via gosu inside container)");
    println!();
    println!("{}", "Security:".bold());
    println!("  --read-only root filesystem");
    println!("  --cap-drop ALL");
    println!("  --memory 8g (default, claude OOMs itself not your host)");
    println!("  --pids-limit 512");
    println!("  --security-opt no-new-privileges");
    println!("  UID={uid} GID={gid} (via gosu inside container)");
    println!("  Config mounts read-only (.jj, .gitconfig, .claude.json)");
    println!("  .claude mounted writable (Claude writes sessions/hooks)");
    println!("  No --dangerously-skip-permissions (approval prompts active)");
    println!("  host.docker.internal: off by default (use --host-access)");
    Ok(())
}

struct RunConfig<'a> {
    image: &'a str,
    profile: Option<&'a str>,
    claude_args: &'a [String],
    ports: &'a [String],
    extra_mounts: &'a [String],
    memory: &'a str,
    cpus: &'a str,
    host_access: HostAccess,
    cache: CacheMode,
    env: EnvMode,
    gpus: GpuMode,
}

fn cmd_run(config: &RunConfig<'_>) -> Result<()> {
    let backend = detect_backend()?;
    let profile_name = config
        .profile
        .map_or_else(get_active, |p| Ok(p.to_owned()))?;
    let profile_config = load_profile(&profile_name)
        .with_context(|| format!("Profile '{profile_name}' not found"))?;

    let project_dir = std::env::current_dir().context("cannot read current directory")?;
    let project_str = project_dir.to_string_lossy().into_owned();
    let folder = project_dir.file_name().map_or_else(
        || "project".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let container_base_name = sanitise_project_name(&folder);
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

    print_banner(&profile_name, &project_str, config.image);

    let git_name = get_git_identity("user.name");
    let git_email = get_git_identity("user.email");

    let inputs = LaunchInputs {
        image: config.image,
        config: &profile_config,
        project_dir: &project_str,
        base_name: &container_base_name,
        host_home: &home,
        uid: &uid,
        gid: &gid,
        extra_claude_args: config.claude_args,
        ports: config.ports,
        extra_mounts: config.extra_mounts,
        memory: config.memory,
        cpus: config.cpus,
        host_access: config.host_access,
        cache: config.cache,
        env: config.env,
        gpus: config.gpus,
        nonce: std::process::id(),
        git_name: git_name.as_deref(),
        git_email: git_email.as_deref(),
    };

    let plan = resolve_launch_plan(probe_container(backend, &container_base_name), &inputs);

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

    if profile_config.provider.needs_auth_token() {
        cmd.env("ANTHROPIC_AUTH_TOKEN", &profile_config.key);
    }

    let err = cmd.exec();
    Err(err).context(format!("failed to exec {}", backend.binary_name()))
}

fn intercept_profile_shortcut() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return None;
    }
    let first = &args[1];
    let subcommands = [
        "run",
        "key",
        "config",
        "shell",
        "stop",
        "clean",
        "ps",
        "__entrypoint",
    ];
    if subcommands.contains(&first.as_str()) {
        return None;
    }
    if first.starts_with('-') {
        return None;
    }
    Some(first.clone())
}

fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        None => cmd_run(&RunConfig {
            image: &cli.image,
            profile: None,
            claude_args: &[],
            ports: &[],
            extra_mounts: &[],
            memory: "",
            cpus: "",
            host_access: HostAccess::Disabled,
            cache: CacheMode::Persistent,
            env: EnvMode::Loaded,
            gpus: GpuMode::None,
        }),
        Some(Cmd::Run {
            profile,
            ports,
            extra_mounts,
            memory,
            cpus,
            gpus,
            host_access,
            no_cache,
            no_env,
            claude_args,
        }) => cmd_run(&RunConfig {
            image: &cli.image,
            profile: profile.as_deref(),
            claude_args: &claude_args,
            ports: &ports,
            extra_mounts: &extra_mounts,
            memory: &memory,
            cpus: &cpus,
            host_access: bool_to_host_access(host_access),
            cache: bool_to_cache(no_cache),
            env: bool_to_env(no_env),
            gpus: bool_to_gpu(gpus),
        }),
        Some(Cmd::Key { action }) => match action {
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
        Some(Cmd::Config) => cmd_config(&cli.image),
        Some(Cmd::Shell {
            profile,
            ports,
            extra_mounts,
            host_access,
            no_env,
            bash_args,
        }) => {
            let combined: Vec<String> = std::iter::once("shell".to_string())
                .chain(bash_args)
                .collect();
            cmd_run(&RunConfig {
                image: &cli.image,
                profile: profile.as_deref(),
                claude_args: &combined,
                ports: &ports,
                extra_mounts: &extra_mounts,
                memory: "",
                cpus: "",
                host_access: bool_to_host_access(host_access),
                cache: CacheMode::Persistent,
                env: bool_to_env(no_env),
                gpus: GpuMode::None,
            })
        }
        Some(Cmd::Stop { name }) => {
            let backend = detect_backend()?;
            let cname = name.unwrap_or_else(|| {
                get_container_name().unwrap_or_else(|| "claude-project".to_string())
            });
            cmd_stop(backend, &cname)
        }
        Some(Cmd::Clean) => {
            let backend = detect_backend()?;
            cmd_cleanup(backend)
        }
        Some(Cmd::Ps) => {
            let backend = detect_backend()?;
            cmd_ps(backend)
        }
        Some(Cmd::InternalEntrypoint { args }) => entrypoint::cmd_internal_entrypoint(&args),
    }
}

fn main() -> Result<()> {
    if let Some(profile_name) = intercept_profile_shortcut() {
        cmd_key_use(&profile_name)?;
        let args: Vec<String> = std::env::args().collect();
        let mut claude_args = Vec::new();
        let mut found_separator = false;
        for arg in args.iter().skip(2) {
            if *arg == "--" {
                found_separator = true;
                continue;
            }
            if found_separator {
                claude_args.push(arg.clone());
            }
        }
        return cmd_run(&RunConfig {
            image: DEFAULT_IMAGE,
            profile: Some(&profile_name),
            claude_args: &claude_args,
            ports: &[],
            extra_mounts: &[],
            memory: "",
            cpus: "",
            host_access: HostAccess::Disabled,
            cache: CacheMode::Persistent,
            env: EnvMode::Loaded,
            gpus: GpuMode::None,
        });
    }
    dispatch(Cli::parse())
}

const fn bool_to_host_access(v: bool) -> HostAccess {
    if v {
        HostAccess::Enabled
    } else {
        HostAccess::Disabled
    }
}
const fn bool_to_cache(v: bool) -> CacheMode {
    if v {
        CacheMode::Ephemeral
    } else {
        CacheMode::Persistent
    }
}
const fn bool_to_env(v: bool) -> EnvMode {
    if v {
        EnvMode::Skipped
    } else {
        EnvMode::Loaded
    }
}
const fn bool_to_gpu(v: bool) -> GpuMode {
    if v {
        GpuMode::All
    } else {
        GpuMode::None
    }
}

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod security_tests;
#[cfg(test)]
mod validation_tests;
