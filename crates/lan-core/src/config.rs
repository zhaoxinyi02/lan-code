use std::{env, fs, path::Path};

use anyhow::{Context, Result, bail};
use lan_protocol::ApprovalMode;
use serde::Deserialize;

use crate::OpenAiCompatibleProvider;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub base_url: String,
    pub model: String,
    pub api_key_env: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanConfig {
    pub provider: Option<ProviderConfig>,
    pub database: Option<String>,
    pub approval_mode: Option<ApprovalMode>,
}

impl LanConfig {
    pub fn load() -> Result<Self> {
        if let Ok(path) = env::var("LAN_CONFIG") {
            return Self::load_path(path);
        }
        if Path::new("lan.toml").is_file() {
            return Self::load_path("lan.toml");
        }
        Ok(Self::default())
    }

    pub fn load_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)
            .with_context(|| format!("read configuration {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parse configuration {}", path.display()))
    }

    pub fn provider(&self) -> Result<Option<OpenAiCompatibleProvider>> {
        if let Some(config) = &self.provider {
            let api_key = env::var(&config.api_key_env).with_context(|| {
                format!("{} is required by provider config", config.api_key_env)
            })?;
            let model = env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| config.model.clone());
            return OpenAiCompatibleProvider::new(config.base_url.clone(), api_key, model)
                .map(Some);
        }
        let Ok(api_key) = env::var("DEEPSEEK_API_KEY") else {
            return Ok(None);
        };
        let model = env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-v4-pro".into());
        OpenAiCompatibleProvider::new("https://api.deepseek.com".into(), api_key, model).map(Some)
    }

    pub fn database(&self) -> Option<String> {
        env::var("LAN_DATABASE")
            .ok()
            .or_else(|| self.database.clone())
    }

    pub fn approval_mode(&self) -> Result<ApprovalMode> {
        let Ok(mode) = env::var("LAN_APPROVAL_MODE") else {
            return Ok(self.approval_mode.unwrap_or(ApprovalMode::ReadOnly));
        };
        match mode.as_str() {
            "read-only" => Ok(ApprovalMode::ReadOnly),
            "ask" => Ok(ApprovalMode::Ask),
            "workspace" => Ok(ApprovalMode::Workspace),
            "full-access" => Ok(ApprovalMode::FullAccess),
            _ => bail!("LAN_APPROVAL_MODE must be read-only, ask, workspace, or full-access"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use lan_protocol::ApprovalMode;
    use uuid::Uuid;

    use super::LanConfig;

    #[test]
    fn loads_strict_configuration() {
        let path = std::env::temp_dir().join(format!("lan-config-{}.toml", Uuid::new_v4()));
        fs::write(
            &path,
            r#"
database = ".lan-code.sqlite"
approval_mode = "workspace"

[provider]
base_url = "https://api.example.com"
model = "coding-model"
api_key_env = "EXAMPLE_API_KEY"
"#,
        )
        .unwrap();

        let config = LanConfig::load_path(&path).unwrap();
        assert_eq!(config.database.as_deref(), Some(".lan-code.sqlite"));
        assert_eq!(config.approval_mode, Some(ApprovalMode::Workspace));
        assert_eq!(config.provider.unwrap().api_key_env, "EXAMPLE_API_KEY");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_unknown_configuration_fields() {
        let path = std::env::temp_dir().join(format!("lan-config-{}.toml", Uuid::new_v4()));
        fs::write(&path, "mystery = true").unwrap();
        assert!(LanConfig::load_path(&path).is_err());
        fs::remove_file(path).unwrap();
    }
}
