use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use russh::{
    client,
    keys::{HashAlg, PrivateKeyWithHashAlg, load_secret_key, ssh_key},
};

use crate::config::{AppConfig, AppPaths, AuthMethod, save_config};

pub struct Client {
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
}

pub async fn connect(
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
) -> Result<client::Handle<Client>> {
    let snapshot = {
        let config = config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        config.runtime_config()?.ssh
    };
    log::info!(
        "Connecting to {}@{}:{} (auth={})",
        snapshot.username,
        snapshot.server,
        snapshot.port,
        snapshot.auth_method.as_str(),
    );
    let ssh_config = Arc::new(client::Config {
        nodelay: true,
        ..Default::default()
    });
    let handler = Client { config, paths };
    let mut session = client::connect(
        ssh_config,
        (snapshot.server.as_str(), snapshot.port),
        handler,
    )
    .await
    .with_context(|| format!("failed to connect SSH server {}", snapshot.server))?;

    match snapshot.auth_method {
        AuthMethod::Password => {
            authenticate_password(&mut session, &snapshot.username, &snapshot.ssh_password).await?
        }
        AuthMethod::Key => {
            authenticate_public_key(
                &mut session,
                &snapshot.username,
                &snapshot.key_path,
                &snapshot.key_password,
            )
            .await?
        }
    }

    log::info!("SSH authenticated successfully");
    Ok(session)
}

async fn authenticate_password(
    session: &mut client::Handle<Client>,
    username: &str,
    password: &str,
) -> Result<()> {
    log::info!("Authenticating with password");
    let auth_result = session
        .authenticate_password(username, password)
        .await
        .context("SSH password authentication failed")?;
    if !auth_result.success() {
        bail!("SSH password authentication was rejected");
    }
    Ok(())
}

async fn authenticate_public_key(
    session: &mut client::Handle<Client>,
    username: &str,
    key_path: &str,
    key_password: &str,
) -> Result<()> {
    log::info!("Authenticating with public key: {key_path}");
    let passphrase = (!key_password.is_empty()).then_some(key_password);
    let key_pair = load_secret_key(key_path, passphrase).context("failed to load SSH key")?;
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
        bail!("SSH public key authentication was rejected");
    }
    Ok(())
}

impl client::Handler for Client {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key.fingerprint(HashAlg::Sha256).to_string();
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
