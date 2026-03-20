use easyterm_remote::RemoteSessionSpec;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSessionSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

impl LocalSessionSpec {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
        }
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTarget {
    Local(LocalSessionSpec),
    Ssh(RemoteSessionSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub target: SessionTarget,
}

#[derive(Debug, Default)]
pub struct SessionManager {
    sessions: Vec<Session>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    pub fn spawn_local(&mut self, spec: LocalSessionSpec) -> &Session {
        let id = format!("local-{}", self.sessions.len() + 1);
        let title = std::path::Path::new(&spec.program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(spec.program.as_str())
            .to_string();
        self.sessions.push(Session {
            id,
            title,
            target: SessionTarget::Local(spec),
        });
        self.sessions.last().unwrap()
    }

    pub fn spawn_ssh(&mut self, spec: RemoteSessionSpec) -> &Session {
        let id = format!("ssh-{}", self.sessions.len() + 1);
        self.sessions.push(Session {
            id,
            title: spec.profile_name.clone(),
            target: SessionTarget::Ssh(spec),
        });
        self.sessions.last().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalSessionSpec, SessionManager, SessionTarget};
    use easyterm_remote::{AuthMethod, RemoteSessionSpec};

    #[test]
    fn creates_local_and_ssh_sessions() {
        let mut manager = SessionManager::new();
        manager.spawn_local(LocalSessionSpec::new("/bin/bash"));
        manager.spawn_ssh(RemoteSessionSpec {
            profile_name: "dev".into(),
            target: "dev.internal".into(),
            port: 22,
            user: Some("alice".into()),
            auth: AuthMethod::Agent,
        });

        assert_eq!(manager.sessions().len(), 2);
        assert!(matches!(
            manager.sessions()[1].target,
            SessionTarget::Ssh(_)
        ));
    }
}
