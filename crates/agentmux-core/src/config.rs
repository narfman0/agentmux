use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_prefix")]
    pub prefix: String,
    #[serde(default)]
    pub agent: Vec<AgentConfig>,
    #[serde(default)]
    pub bind: Vec<KeybindDef>,
    #[serde(default = "default_true")]
    pub dangerously_skip_permissions: bool,
}

fn default_prefix() -> String {
    "ctrl+a".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            prefix: default_prefix(),
            agent: Vec::new(),
            bind: default_binds(),
            dangerously_skip_permissions: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeybindDef {
    pub key: String,
    pub action: String,
}

pub fn config_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.config_dir().join("agentmux"))
        .unwrap_or_else(|| PathBuf::from("agentmux"))
}

pub fn load_config() -> AppConfig {
    let path = config_dir().join("agents.toml");
    if let Ok(text) = std::fs::read_to_string(&path) {
        toml::from_str(&text).unwrap_or_default()
    } else {
        AppConfig::default()
    }
}

pub fn ensure_example_config() {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("agents.toml");
    if !path.exists() {
        let example = r#"# agentmux agent configuration
# Each [[agent]] block defines a command available in the agent picker.

[[agent]]
name = "claude-code"
command = "claude"
args = ["--dangerously-skip-permissions"]

[[agent]]
name = "opencode"
command = "opencode"
args = []
"#;
        let _ = std::fs::write(&path, example);
    }
}

fn default_binds() -> Vec<KeybindDef> {
    vec![
        KeybindDef { key: "|".into(), action: "split_vertical".into() },
        KeybindDef { key: "-".into(), action: "split_horizontal".into() },
        KeybindDef { key: "h".into(), action: "focus_left".into() },
        KeybindDef { key: "j".into(), action: "focus_down".into() },
        KeybindDef { key: "k".into(), action: "focus_up".into() },
        KeybindDef { key: "l".into(), action: "focus_right".into() },
        KeybindDef { key: "n".into(), action: "new_pane".into() },
        KeybindDef { key: "x".into(), action: "close_pane".into() },
        KeybindDef { key: "d".into(), action: "detach".into() },
        KeybindDef { key: "b".into(), action: "toggle_broadcast".into() },
        KeybindDef { key: ":".into(), action: "command_prompt".into() },
        KeybindDef { key: "1".into(), action: "focus_pane_1".into() },
        KeybindDef { key: "2".into(), action: "focus_pane_2".into() },
        KeybindDef { key: "3".into(), action: "focus_pane_3".into() },
        KeybindDef { key: "?".into(), action: "help".into() },
        KeybindDef { key: "q".into(), action: "quit".into() },
    ]
}
