use crate::errors::{AgentError, Result};
use crate::types::PermissionMode;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_BASE_URL: &str = "http://localhost:8081/v1";
pub const DEFAULT_MODEL: &str = "gemini-3.5-flash";
pub const DEFAULT_MAX_ITERATIONS: usize = 50;
pub const DEFAULT_COMMAND_TIMEOUT_SECS: u64 = 180;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<usize>,
    #[serde(default)]
    pub command_timeout_secs: Option<u64>,
}

/// Fully resolved runtime configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub permission_mode: PermissionMode,
    pub max_iterations: usize,
    pub command_timeout_secs: u64,
    pub verbose: bool,
    pub auto: bool,
}

impl Config {
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("saqlain-code").join("config.toml"))
    }

    pub fn load_file_config() -> FileConfig {
        if let Some(path) = Self::config_path() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(cfg) = toml::from_str::<FileConfig>(&text) {
                    return cfg;
                }
            }
        }
        FileConfig {
            base_url: None,
            model: None,
            api_key: None,
            permission_mode: None,
            max_iterations: None,
            command_timeout_secs: None,
        }
    }

    /// Build config from CLI args, environment variables, then config file, then defaults.
    #[allow(clippy::too_many_arguments)]
    pub fn resolve(
        cli_base_url: Option<String>,
        cli_model: Option<String>,
        cli_api_key: Option<String>,
        cli_permission_mode: Option<String>,
        cli_max_iterations: Option<usize>,
        cli_command_timeout_secs: Option<u64>,
        auto: bool,
        verbose: bool,
    ) -> Result<Self> {
        let file_cfg = Self::load_file_config();

        let base_url = cli_base_url
            .or_else(|| std::env::var("SAQLAIN_CODE_BASE_URL").ok())
            .or(file_cfg.base_url)
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        let model = cli_model
            .or_else(|| std::env::var("SAQLAIN_CODE_MODEL").ok())
            .or(file_cfg.model)
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());

        let api_key = cli_api_key
            .or_else(|| std::env::var("SAQLAIN_CODE_API_KEY").ok())
            .or(file_cfg.api_key)
            .unwrap_or_else(|| "anything".to_string());

        let mut permission_mode = cli_permission_mode
            .or(file_cfg.permission_mode)
            .map(|s| s.parse::<PermissionMode>())
            .transpose()
            .map_err(AgentError::Config)?
            .unwrap_or_default();

        if auto {
            permission_mode = PermissionMode::Auto;
        }

        let max_iterations = cli_max_iterations
            .or(file_cfg.max_iterations)
            .unwrap_or(DEFAULT_MAX_ITERATIONS);

        let command_timeout_secs = cli_command_timeout_secs
            .or(file_cfg.command_timeout_secs)
            .unwrap_or(DEFAULT_COMMAND_TIMEOUT_SECS);

        Ok(Self {
            base_url,
            model,
            api_key,
            permission_mode,
            max_iterations,
            command_timeout_secs,
            verbose,
            auto,
        })
    }
}
