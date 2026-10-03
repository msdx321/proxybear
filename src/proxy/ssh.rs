use std::{
    fmt,
    sync::{Arc, Mutex},
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
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
    /// Dropped together with the handler when the session task ends.
    _closed: oneshot::Sender<()>,
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

/// Connect and authenticate an SSH session.
///
/// The returned receiver resolves once the session has ended for any reason.
pub async fn connect(
    ssh: &SshConnectConfig,
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
) -> Result<(client::Handle<Client>, oneshot::Receiver<()>)> {
    timeout(SSH_CONNECT_TIMEOUT, connect_inner(ssh, config, paths))
        .await
        .with_context(|| {
            format!(
                "timed out connecting to SSH server {} after {SSH_CONNECT_TIMEOUT:?}",
                ssh.server
            )
        })?
}

async fn connect_inner(
    ssh: &SshConnectConfig,
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
) -> Result<(client::Handle<Client>, oneshot::Receiver<()>)> {
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
        config,
        paths,
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
        let mut config = self
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(expected) = &config.host_fingerprint {
            Ok(expected == &fingerprint)
        } else {
            config.host_fingerprint = Some(fingerprint);
            save_config(&self.paths, &config)?;
            Ok(true)
        }
    }
}
