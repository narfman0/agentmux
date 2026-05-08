use agentmux_core::config::AppConfig;
use directories::BaseDirs;
use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    BaseDirs::new()
        .map(|b| b.config_dir().join("agentmux"))
        .unwrap_or_else(|| PathBuf::from("agentmux"))
}

/// Load config from %APPDATA%\agentmux\agents.toml (or default if missing).
pub fn load() -> AppConfig {
    let path = config_dir().join("agents.toml");
    if let Ok(text) = std::fs::read_to_string(&path) {
        toml::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("config parse error in {}: {e}", path.display());
            default_config()
        })
    } else {
        default_config()
    }
}

fn default_config() -> AppConfig {
    AppConfig::default()
}

/// Write a starter config if none exists.
pub fn ensure_example_config() {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let agents_path = dir.join("agents.toml");
    if !agents_path.exists() {
        let example = r#"# agentmux agent configuration
# Each [[agent]] block defines a command available in the Ctrl+A n picker.

[[agent]]
name = "claude-code"
command = "claude"
args = []

[[agent]]
name = "opencode"
command = "opencode"
args = []
"#;
        let _ = std::fs::write(&agents_path, example);
    }
}
