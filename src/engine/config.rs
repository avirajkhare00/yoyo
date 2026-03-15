use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub(crate) const YOYO_CONFIG_FILE: &str = "yoyo.json";

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct YoyoProjectConfig {
    #[serde(default)]
    pub(crate) notes: Vec<String>,
    #[serde(default)]
    pub(crate) conventions: ProjectConventions,
    #[serde(default)]
    pub(crate) runtime: RuntimePolicy,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ProjectConventions {
    #[serde(default)]
    pub(crate) languages: Vec<String>,
    #[serde(default)]
    pub(crate) frameworks: Vec<String>,
    #[serde(default)]
    pub(crate) style_rules: Vec<String>,
    #[serde(default)]
    pub(crate) commands: BTreeMap<String, Vec<String>>,
}

impl ProjectConventions {
    pub(crate) fn is_empty(&self) -> bool {
        self.languages.is_empty()
            && self.frameworks.is_empty()
            && self.style_rules.is_empty()
            && self.commands.is_empty()
    }
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct RuntimePolicy {
    #[serde(default)]
    pub(crate) checks: Vec<RuntimeSmokeCheck>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct RuntimeSmokeCheck {
    pub(crate) language: String,
    pub(crate) command: Vec<String>,
    #[serde(default)]
    pub(crate) sandbox_prefix: Vec<String>,
    #[serde(default)]
    pub(crate) allow_unsandboxed: bool,
    pub(crate) kind: Option<String>,
    pub(crate) timeout_ms: Option<u64>,
}

pub(crate) fn config_path(root: &Path) -> PathBuf {
    root.join(YOYO_CONFIG_FILE)
}

pub(crate) fn existing_config_path(root: &Path) -> Option<PathBuf> {
    root.ancestors()
        .map(|ancestor| ancestor.join(YOYO_CONFIG_FILE))
        .find(|path| path.exists())
}

pub(crate) fn load_yoyo_project_config(root: &Path) -> Result<Option<YoyoProjectConfig>> {
    let Some(path) = existing_config_path(root) else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path)
        .with_context(|| format!("Failed to read yoyo config {}", path.display()))?;
    let config = serde_json::from_slice::<YoyoProjectConfig>(&bytes)
        .with_context(|| format!("Failed to parse yoyo config {}", path.display()))?;
    Ok(Some(config))
}

pub(crate) fn write_yoyo_project_config(
    root: &Path,
    config: &YoyoProjectConfig,
) -> Result<PathBuf> {
    let path = config_path(root);
    let bytes = serde_json::to_vec_pretty(config)?;
    std::fs::write(&path, bytes)
        .with_context(|| format!("Failed to write yoyo config {}", path.display()))?;
    Ok(path)
}
