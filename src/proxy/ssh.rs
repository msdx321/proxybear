use std::{
    fmt,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use anyhow::{Context, Result};
use russh::{
    client,
    keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate, load_secret_key},
};
use tokio::{sync::oneshot, time::timeout};

use crate::config::{AppConfig, AppPaths, AuthMethod, SshConnectConfig, save_config};

const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const SSH_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

pub struct Client {
    connector: Connector,
    /// Dropped together with the handler when the session task ends.
    _closed: oneshot::Sender<()>,
}

/// Connects the SSH sessions of one proxy run with the settings it started with.
#[derive(Clone)]
pub struct Connector {
    ssh: SshConnectConfig,
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
    /// Host key every session of this run must present. Kept separately from
    /// the config, which may already describe a different server.
    host_fingerprint: Arc<Mutex<Option<String>>>,
}

/// A connect failure that retrying with the same settings cannot fix.
#[derive(Debug)]
pub struct FatalError(String);

impl fmt::Display for FatalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FatalError {}

pub fn is_fatal(error: &anyhow::Error) -> bool {
    error.is::<FatalError>()
}

impl Connector {
    pub fn new(ssh: SshConnectConfig, config: Arc<Mutex<AppConfig>>, paths: AppPaths) -> Self {
        let host_fingerprint = lock(&config).host_fingerprint.clone();
        Self {
            ssh,
            config,
            paths,
            host_fingerprint: Arc::new(Mutex::new(host_fingerprint)),
        }
    }

    /// Connect and authenticate an SSH session.
    ///
    /// The returned receiver resolves once the session has ended for any reason.
    pub async fn connect(&self) -> Result<(client::Handle<Client>, oneshot::Receiver<()>)> {
        timeout(SSH_CONNECT_TIMEOUT, self.connect_inner())
            .await
            .with_context(|| {
                format!(
                    "timed out connecting to SSH server {} after {SSH_CONNECT_TIMEOUT:?}",
                    self.ssh.server
                )
            })?
    }

    async fn connect_inner(&self) -> Result<(client::Handle<Client>, oneshot::Receiver<()>)> {
        let ssh = &self.ssh;
        tracing::info!(
            event = "ssh_connecting",
            username = %ssh.username,
            server = %ssh.server,
            port = ssh.port,
            auth = ssh.auth_method.as_str(),
            "Connecting to {}@{}:{} (auth={})",
            ssh.username,
            ssh.server,
            ssh.port,
            ssh.auth_method.as_str(),
        );
        let ssh_config = Arc::new(client::Config {
            nodelay: true,
            keepalive_interval: Some(SSH_KEEPALIVE_INTERVAL),
            ..Default::default()
        });
        let (closed_tx, closed_rx) = oneshot::channel();
        let handler = Client {
            connector: self.clone(),
            _closed: closed_tx,
        };
        let mut session = client::connect(ssh_config, (ssh.server.as_str(), ssh.port), handler)
            .await
            .map_err(|error| {
                if matches!(
                    error.downcast_ref::<russh::Error>(),
                    Some(russh::Error::UnknownKey)
                ) {
                    FatalError(format!(
                        "SSH host key for {} does not match the saved fingerprint",
                        ssh.server
                    ))
                    .into()
                } else {
                    error.context(format!("failed to connect SSH server {}", ssh.server))
                }
            })?;

        match ssh.auth_method {
            AuthMethod::Password => {
                authenticate_password(&mut session, &ssh.username, &ssh.ssh_password).await?
            }
            AuthMethod::Key => {
                authenticate_public_key(
                    &mut session,
                    &ssh.username,
                    &ssh.key_path,
                    &ssh.key_password,
                )
                .await?
            }
        }

        tracing::info!(
            event = "ssh_authenticated",
            "SSH authenticated successfully"
        );
        Ok((session, closed_rx))
    }
}

async fn authenticate_password(
    session: &mut client::Handle<Client>,
    username: &str,
    password: &str,
) -> Result<()> {
    tracing::info!(
        event = "ssh_authenticating",
        auth = "password",
        "Authenticating with password"
    );
    let auth_result = session
        .authenticate_password(username, password)
        .await
        .context("SSH password authentication failed")?;
    if !auth_result.success() {
        return Err(FatalError("SSH password authentication was rejected".into()).into());
    }
    Ok(())
}

async fn authenticate_public_key(
    session: &mut client::Handle<Client>,
    username: &str,
    key_path: &str,
    key_password: &str,
) -> Result<()> {
    tracing::info!(
        event = "ssh_authenticating",
        auth = "key",
        key_path = %key_path,
        "Authenticating with public key"
    );
    let passphrase = (!key_password.is_empty()).then_some(key_password);
    let key_pair = load_secret_key(key_path, passphrase)
        .map_err(|error| FatalError(format!("failed to load SSH key: {error}")))?;
    let auth_result = session
        .authenticate_publickey(
            username,
            PrivateKeyWithHashAlg::new(
                Arc::new(key_pair),
                session.best_supported_rsa_hash().await?.flatten(),
            ),
        )
        .await
        .context("SSH public key authentication failed")?;
    if !auth_result.success() {
        return Err(FatalError("SSH public key authentication was rejected".into()).into());
    }
    Ok(())
}

impl client::Handler for Client {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key
            .public_key()
            .fingerprint(HashAlg::Sha256)
            .to_string();
        let connector = &self.connector;
        let mut pinned = lock(&connector.host_fingerprint);
        if let Some(expected) = pinned.as_ref() {
            return Ok(expected == &fingerprint);
        }
        *pinned = Some(fingerprint.clone());

        // Trust on first use, but only save the key while the settings still
        // point at the server this run connects to.
        let mut config = lock(&connector.config);
        if config.server.trim() == connector.ssh.server
            && config.port == connector.ssh.port
            && config.host_fingerprint.is_none()
        {
            config.host_fingerprint = Some(fingerprint);
            save_config(&connector.paths, &config)?;
        }
        Ok(true)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
