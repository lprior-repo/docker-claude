use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    Minimax,
    Zai,
}

impl Provider {
    pub fn from_str_lossy(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" => Ok(Provider::Anthropic),
            "minimax" => Ok(Provider::Minimax),
            "zai" => Ok(Provider::Zai),
            other => bail!("unknown provider '{other}' — valid: anthropic, minimax, zai"),
        }
    }

    pub fn env_vars(&self) -> Vec<(&'static str, &'static str)> {
        match self {
            Provider::Anthropic => vec![],
            Provider::Minimax => vec![
                ("ANTHROPIC_BASE_URL", "https://api.minimax.io/anthropic"),
                ("ANTHROPIC_AUTH_TOKEN", ""),
                ("ANTHROPIC_MODEL", "MiniMax-M2.7-highspeed"),
                ("ANTHROPIC_SMALL_FAST_MODEL", "MiniMax-M2.7-highspeed"),
                ("ANTHROPIC_DEFAULT_SONNET_MODEL", "MiniMax-M2.7-highspeed"),
                ("ANTHROPIC_DEFAULT_OPUS_MODEL", "MiniMax-M2.7-highspeed"),
                ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "MiniMax-M2.7-highspeed"),
                ("API_TIMEOUT_MS", API_TIMEOUT),
                ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
            ],
            Provider::Zai => vec![
                ("ANTHROPIC_BASE_URL", "https://api.z.ai/api/anthropic"),
                ("ANTHROPIC_AUTH_TOKEN", ""),
                ("ANTHROPIC_DEFAULT_OPUS_MODEL", "GLM-5-Turbo"),
                ("API_TIMEOUT_MS", API_TIMEOUT),
                ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
            ],
        }
    }

    pub fn needs_auth_token(&self) -> bool {
        matches!(self, Provider::Minimax | Provider::Zai)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::Minimax => "minimax",
            Provider::Zai => "zai",
        }
    }
}

pub const API_TIMEOUT: &str = "3000000";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProfileConfig {
    pub key: String,
    pub provider: Provider,
}
