use anyhow::{Context, Result};
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

const SERVICE: &str = super::APP_ID;
/// `errSecItemNotFound`.
const ITEM_NOT_FOUND: i32 = -25300;

pub const SSH_PASSWORD: &str = "ssh-password";
pub const KEY_PASSPHRASE: &str = "key-passphrase";

/// Read a secret, or an empty string when none is stored.
pub fn get(account: &str) -> Result<String> {
    match get_generic_password(SERVICE, account) {
        Ok(bytes) => String::from_utf8(bytes)
            .with_context(|| format!("Keychain item {account} is not valid UTF-8")),
        Err(error) if error.code() == ITEM_NOT_FOUND => Ok(String::new()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read {account} from the Keychain"))
        }
    }
}

/// Store a secret, or delete it when `value` is empty.
pub fn set(account: &str, value: &str) -> Result<()> {
    let result = if value.is_empty() {
        match delete_generic_password(SERVICE, account) {
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(()),
            result => result,
        }
    } else {
        set_generic_password(SERVICE, account, value.as_bytes())
    };
    result.with_context(|| format!("failed to save {account} to the Keychain"))
}
