use std::{fmt, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use russh::{
    client,
    keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate, load_secret_key},
};
use tokio::{sync::oneshot, time::timeout};

use crate::{
    app::stats::{HostKeyPrompt, ProxyStats},
    config::{AuthMethod, SshConnectConfig},
};

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
    stats: Arc<ProxyStats>,
}

/// A connect failure that retrying with the same settings is unlikely to fix,
/// such as rejected credentials or a changed host key.
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
    pub fn new(ssh: SshConnectConfig, stats: Arc<ProxyStats>) -> Self {
        Self { ssh, stats }
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
                        "SSH host key for {} is not trusted; verify it in Settings",
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
        let ssh = &self.connector.ssh;
        if ssh.host_fingerprint.as_ref() == Some(&fingerprint) {
            return Ok(true);
        }
        // Unknown or changed keys are refused until the user trusts them.
        tracing::warn!(
            event = "ssh_host_key_untrusted",
            server = %ssh.server,
            fingerprint = %fingerprint,
            changed = ssh.host_fingerprint.is_some(),
            "SSH host key is not trusted"
        );
        self.connector
            .stats
            .set_host_key_prompt(Some(HostKeyPrompt {
                server: format!("{}:{}", ssh.server, ssh.port),
                fingerprint,
                previous: ssh.host_fingerprint.clone(),
            }));
        Ok(false)
    }
}
