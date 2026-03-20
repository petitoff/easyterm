use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthMethod {
    Agent,
    KeyFile { path: PathBuf },
    PasswordPrompt,
}

impl Default for AuthMethod {
    fn default() -> Self {
        Self::Agent
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshProfile {
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub auth: AuthMethod,
    #[serde(default)]
    pub startup_command: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSessionSpec {
    pub profile_name: String,
    pub target: String,
    pub port: u16,
    pub user: Option<String>,
    pub auth: AuthMethod,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProfileStore {
    #[serde(default)]
    pub profiles: Vec<SshProfile>,
}

impl ProfileStore {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut names = HashSet::new();

        for profile in &self.profiles {
            if profile.name.trim().is_empty() {
                return Err(ValidationError::EmptyName);
            }
            if profile.host.trim().is_empty() {
                return Err(ValidationError::EmptyHost {
                    profile: profile.name.clone(),
                });
            }
            if !names.insert(profile.name.clone()) {
                return Err(ValidationError::DuplicateName(profile.name.clone()));
            }
        }

        Ok(())
    }

    pub fn quick_connect(&self, profile_name: &str) -> Result<RemoteSessionSpec, ValidationError> {
        let profile = self
            .profiles
            .iter()
            .find(|profile| profile.name == profile_name)
            .ok_or_else(|| ValidationError::UnknownProfile(profile_name.to_string()))?;

        Ok(RemoteSessionSpec {
            profile_name: profile.name.clone(),
            target: profile.host.clone(),
            port: profile.port,
            user: profile.user.clone(),
            auth: profile.auth.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    EmptyName,
    EmptyHost { profile: String },
    DuplicateName(String),
    UnknownProfile(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::EmptyName => write!(f, "ssh profile name cannot be empty"),
            ValidationError::EmptyHost { profile } => {
                write!(f, "ssh profile `{profile}` must define a host")
            }
            ValidationError::DuplicateName(name) => {
                write!(f, "ssh profile `{name}` is defined more than once")
            }
            ValidationError::UnknownProfile(name) => {
                write!(f, "ssh profile `{name}` does not exist")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

fn default_port() -> u16 {
    22
}

#[cfg(test)]
mod tests {
    use super::{AuthMethod, ProfileStore, SshProfile, ValidationError};
    use std::path::PathBuf;

    #[test]
    fn rejects_duplicate_profile_names() {
        let store = ProfileStore {
            profiles: vec![
                SshProfile {
                    name: "prod".into(),
                    host: "prod-1".into(),
                    port: 22,
                    user: None,
                    auth: AuthMethod::Agent,
                    startup_command: None,
                    tags: vec![],
                },
                SshProfile {
                    name: "prod".into(),
                    host: "prod-2".into(),
                    port: 22,
                    user: None,
                    auth: AuthMethod::Agent,
                    startup_command: None,
                    tags: vec![],
                },
            ],
        };

        assert_eq!(
            store.validate(),
            Err(ValidationError::DuplicateName("prod".into()))
        );
    }

    #[test]
    fn builds_quick_connect_spec() {
        let store = ProfileStore {
            profiles: vec![SshProfile {
                name: "dev".into(),
                host: "dev.internal".into(),
                port: 2200,
                user: Some("alice".into()),
                auth: AuthMethod::KeyFile {
                    path: PathBuf::from("~/.ssh/id_ed25519"),
                },
                startup_command: None,
                tags: vec!["team-a".into()],
            }],
        };

        let spec = store.quick_connect("dev").unwrap();

        assert_eq!(spec.profile_name, "dev");
        assert_eq!(spec.target, "dev.internal");
        assert_eq!(spec.port, 2200);
        assert_eq!(spec.user.as_deref(), Some("alice"));
    }
}
