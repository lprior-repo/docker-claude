use anyhow::{bail, Context, Result};
use colored::Colorize;

use crate::provider::{ProfileConfig, Provider};

const SERVICE: &str = "claude-dock";
const ACTIVE_KEY: &str = "__active_profile__";
const MANIFEST_KEY: &str = "__manifest__";

pub fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("profile name cannot be empty");
    }
    if name.contains(',') {
        bail!("profile name cannot contain commas (breaks manifest storage)");
    }
    if name.starts_with("__") || name.ends_with("__") {
        bail!("profile name cannot start or end with '__' (reserved for internal use)");
    }
    let reserved = [
        "run", "key", "config", "shell", "list", "add", "use", "remove",
    ];
    if reserved.contains(&name) {
        bail!(
            "profile name '{}' is reserved — choose another name\n  (reserved: {})",
            name,
            reserved.join(", ")
        );
    }
    Ok(())
}

pub fn store_secret(profile: &str, secret: &str) -> Result<()> {
    keyring::Entry::new(SERVICE, profile)?
        .set_password(secret)
        .context("storing secret")
}

pub fn load_secret(profile: &str) -> Result<String> {
    keyring::Entry::new(SERVICE, profile)?
        .get_password()
        .with_context(|| format!("no key for profile '{profile}'"))
}

fn delete_secret(profile: &str) -> Result<()> {
    keyring::Entry::new(SERVICE, profile)?
        .delete_credential()
        .with_context(|| format!("deleting '{profile}'"))
}

pub fn get_active() -> Result<String> {
    load_secret(ACTIVE_KEY).context("no active profile - run: claude-dock key use <name>")
}

fn register_profile(name: &str) -> Result<()> {
    let manifest = load_secret(MANIFEST_KEY).unwrap_or_else(|_| String::new());
    let mut profiles: Vec<&str> = manifest.split(',').filter(|s| !s.is_empty()).collect();
    if !profiles.contains(&name) {
        profiles.push(name);
    }
    store_secret(MANIFEST_KEY, &profiles.join(","))
}

fn read_password() -> String {
    use std::io::{self, Write};
    let _ = std::process::Command::new("stty").arg("-echo").status();
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
    let _ = std::process::Command::new("stty").arg("echo").status();
    println!();
    buf.trim().to_owned()
}

pub fn cmd_key_add(name: &str, key: Option<&str>, provider_str: &str) -> Result<()> {
    validate_profile_name(name)?;
    let provider = Provider::from_str_lossy(provider_str)?;
    let secret_key = key.map_or_else(
        || {
            eprint!("Enter key for '{name}' (provider: {}): ", provider.name());
            read_password()
        },
        std::borrow::ToOwned::to_owned,
    );
    if secret_key.trim().is_empty() {
        bail!("Key cannot be empty");
    }
    let config = ProfileConfig {
        key: secret_key.trim().to_owned(),
        provider,
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

pub fn cmd_key_list() {
    let manifest = load_secret(MANIFEST_KEY).unwrap_or_else(|_| String::new());
    let active = get_active().unwrap_or_else(|_| String::new());
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

pub fn cmd_key_use(name: &str) -> Result<()> {
    validate_profile_name(name)?;
    load_secret(name).with_context(|| {
        format!("Profile '{name}' not found. Add it: claude-dock key add {name}")
    })?;
    store_secret(ACTIVE_KEY, name)?;
    println!("{} Active profile -> '{}'", "OK".green(), name.cyan());
    Ok(())
}

pub fn cmd_key_remove(name: &str) -> Result<()> {
    validate_profile_name(name)?;
    delete_secret(name).with_context(|| format!("Profile '{name}' does not exist"))?;
    let manifest = load_secret(MANIFEST_KEY).unwrap_or_else(|_| String::new());
    let updated = manifest
        .split(',')
        .filter(|p| !p.is_empty() && *p != name)
        .collect::<Vec<_>>()
        .join(",");
    if updated.is_empty() {
        let _ = delete_secret(MANIFEST_KEY);
    } else {
        store_secret(MANIFEST_KEY, &updated)?;
    }
    if get_active().unwrap_or_else(|_| String::new()) == name {
        let _ = delete_secret(ACTIVE_KEY);
        println!(
            "{} That was the active profile. Set a new one: claude-dock key use <name>",
            "!".yellow()
        );
    }
    println!("{} Profile '{}' removed.", "OK".green(), name.red());
    Ok(())
}

pub fn load_profile(profile: &str) -> Result<ProfileConfig> {
    let raw = load_secret(profile)?;
    serde_json::from_str::<ProfileConfig>(&raw).map_or_else(
        |_| {
            let provider = Provider::from_str_lossy("anthropic")?;
            Ok(ProfileConfig { key: raw, provider })
        },
        Ok,
    )
}

use clap::Subcommand as ClapSubcommand;

#[derive(ClapSubcommand)]
pub enum KeyAction {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_profile_name_rejects_reserved_names() {
        for name in [
            "run", "key", "config", "shell", "list", "add", "use", "remove",
        ] {
            assert!(
                validate_profile_name(name).is_err(),
                "profile name '{name}' should be rejected as reserved"
            );
        }
    }

    #[test]
    fn validate_profile_name_accepts_regular_names() {
        assert!(validate_profile_name("glm1").is_ok());
        assert!(validate_profile_name("minimax25").is_ok());
        assert!(validate_profile_name("work").is_ok());
    }

    #[test]
    fn validate_profile_name_rejects_empty_and_comma() {
        assert!(validate_profile_name("").is_err());
        assert!(validate_profile_name("foo,bar").is_err());
    }
}
