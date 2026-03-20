use easyterm_remote::{ProfileStore, SshProfile};
use easyterm_render::RendererPreference;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub font: FontConfig,
    #[serde(default)]
    pub shell: ShellConfig,
    #[serde(default)]
    pub renderer: RendererConfig,
    #[serde(default = "default_scrollback_limit")]
    pub scrollback_limit: usize,
    #[serde(default)]
    pub keybindings: BTreeMap<String, String>,
    #[serde(default)]
    pub ssh_profiles: Vec<SshProfile>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut keybindings = BTreeMap::new();
        keybindings.insert("new_tab".into(), "ctrl+shift+t".into());
        keybindings.insert("split_horizontal".into(), "ctrl+shift+d".into());
        keybindings.insert("command_palette".into(), "ctrl+shift+p".into());

        Self {
            window: WindowConfig::default(),
            theme: ThemeConfig::default(),
            font: FontConfig::default(),
            shell: ShellConfig::default(),
            renderer: RendererConfig::default(),
            scrollback_limit: default_scrollback_limit(),
            keybindings,
            ssh_profiles: vec![SshProfile {
                name: "example-dev".into(),
                host: "dev.internal".into(),
                port: 22,
                user: Some("alice".into()),
                auth: Default::default(),
                startup_command: Some("tmux attach || tmux".into()),
                tags: vec!["example".into()],
            }],
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
        let config: AppConfig = toml::from_str(&contents).map_err(ConfigError::Toml)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.font.size == 0 {
            return Err(ConfigError::InvalidFontSize(self.font.size));
        }
        if self.window.width == 0 || self.window.height == 0 {
            return Err(ConfigError::InvalidWindowSize(
                self.window.width,
                self.window.height,
            ));
        }
        if self.scrollback_limit == 0 {
            return Err(ConfigError::InvalidScrollback(self.scrollback_limit));
        }

        ProfileStore {
            profiles: self.ssh_profiles.clone(),
        }
        .validate()
        .map_err(ConfigError::Remote)?;

        Ok(())
    }

    pub fn sample_toml() -> String {
        toml::to_string_pretty(&Self::default()).expect("default config should serialize")
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    InvalidFontSize(u16),
    InvalidWindowSize(u32, u32),
    InvalidScrollback(usize),
    Remote(easyterm_remote::ValidationError),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(err) => write!(f, "failed to read config: {err}"),
            ConfigError::Toml(err) => write!(f, "failed to parse config: {err}"),
            ConfigError::InvalidFontSize(size) => {
                write!(f, "font size must be greater than 0, got {size}")
            }
            ConfigError::InvalidWindowSize(width, height) => {
                write!(
                    f,
                    "window width and height must be greater than 0, got {width}x{height}"
                )
            }
            ConfigError::InvalidScrollback(limit) => {
                write!(f, "scrollback limit must be greater than 0, got {limit}")
            }
            ConfigError::Remote(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowConfig {
    #[serde(default = "default_window_width")]
    pub width: u32,
    #[serde(default = "default_window_height")]
    pub height: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: default_window_width(),
            height: default_window_height(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeConfig {
    #[serde(default = "default_theme_name")]
    pub name: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: default_theme_name(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontConfig {
    #[serde(default = "default_font_family")]
    pub family: String,
    #[serde(default = "default_font_size")]
    pub size: u16,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: default_font_family(),
            size: default_font_size(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellConfig {
    #[serde(default = "default_shell_program")]
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_term")]
    pub term: String,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            program: default_shell_program(),
            args: Vec::new(),
            term: default_term(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererConfig {
    #[serde(default)]
    pub preference: RendererPreference,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            preference: RendererPreference::Auto,
        }
    }
}

fn default_theme_name() -> String {
    "oxide".into()
}

fn default_font_family() -> String {
    "Iosevka Term".into()
}

fn default_font_size() -> u16 {
    16
}

fn default_shell_program() -> String {
    "/bin/bash".into()
}

fn default_scrollback_limit() -> usize {
    10_000
}

fn default_term() -> String {
    "xterm-256color".into()
}

fn default_window_width() -> u32 {
    1280
}

fn default_window_height() -> u32 {
    760
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn sample_config_roundtrips() {
        let sample = AppConfig::sample_toml();
        let config: AppConfig = toml::from_str(&sample).unwrap();
        config.validate().unwrap();
    }
}
