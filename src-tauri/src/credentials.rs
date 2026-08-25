use std::{env, io::Write};

use keyring::{Entry, Error};
use zeroize::{Zeroize, Zeroizing};

use crate::models::SshProfile;

const SERVICE: &str = "com.lantern.ssh";
const ASKPASS_ACCOUNT_ENV: &str = "LANTERN_ASKPASS_ACCOUNT";

fn account(profile: &SshProfile) -> Result<String, String> {
    let account = format!(
        "{}|{}@{}:{}",
        profile.id, profile.username, profile.host, profile.port
    );
    if account.len() > 512 {
        return Err("The SSH profile is too long for the operating system credential vault".into());
    }
    Ok(account)
}

fn entry(account: &str) -> Result<Entry, String> {
    if account.is_empty() || account.len() > 512 {
        return Err("The credential account identifier is invalid".into());
    }
    Entry::new(SERVICE, account)
        .map_err(|error| format!("Could not access the operating system credential vault: {error}"))
}

pub fn store(profile: &SshProfile, password: &str) -> Result<(), String> {
    if password.is_empty() {
        return Err("Password cannot be empty".into());
    }
    if password.len() > 4096 || password.contains(['\r', '\n', '\0']) {
        return Err("Password contains unsupported characters or is too long".into());
    }
    entry(&account(profile)?)?
        .set_password(password)
        .map_err(|error| {
            format!("Could not save the password in the operating system credential vault: {error}")
        })
}

pub fn delete(profile: &SshProfile) -> Result<(), String> {
    match entry(&account(profile)?)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(format!(
            "Could not remove the password from the operating system credential vault: {error}"
        )),
    }
}

pub fn has(profile: &SshProfile) -> Result<bool, String> {
    match entry(&account(profile)?)?.get_password() {
        Ok(mut password) => {
            password.zeroize();
            Ok(true)
        }
        Err(Error::NoEntry) => Ok(false),
        Err(error) => Err(format!(
            "Could not read the operating system credential vault: {error}"
        )),
    }
}

fn get(account: &str) -> Result<Option<Zeroizing<String>>, String> {
    match entry(account)?.get_password() {
        Ok(password) => Ok(Some(Zeroizing::new(password))),
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(format!(
            "Could not read the operating system credential vault: {error}"
        )),
    }
}

fn is_login_password_prompt(prompt: &str) -> bool {
    let prompt = prompt.trim().to_ascii_lowercase();
    prompt.ends_with("password:") && !prompt.contains("passphrase")
}

pub fn askpass_exit_code() -> Option<i32> {
    let account = env::var(ASKPASS_ACCOUNT_ENV).ok()?;
    let prompt = env::args().skip(1).collect::<Vec<_>>().join(" ");
    if !is_login_password_prompt(&prompt) {
        return Some(1);
    }
    let Ok(Some(password)) = get(&account) else {
        return Some(1);
    };
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let result = output
        .write_all(password.as_bytes())
        .and_then(|_| output.write_all(b"\n"))
        .and_then(|_| output.flush());
    Some(if result.is_ok() { 0 } else { 1 })
}

pub fn configure_askpass(profile: &SshProfile) -> Result<(String, String), String> {
    Ok((ASKPASS_ACCOUNT_ENV.into(), account(profile)?))
}

#[cfg(test)]
mod tests {
    use super::{account, is_login_password_prompt};
    use crate::models::SshProfile;

    #[test]
    fn accepts_only_login_password_prompts() {
        assert!(is_login_password_prompt("alex@pi's password: "));
        assert!(is_login_password_prompt("Password:"));
        assert!(!is_login_password_prompt(
            "Enter passphrase for key '/id_ed25519':"
        ));
        assert!(!is_login_password_prompt(
            "Are you sure you want to continue connecting?"
        ));
    }

    #[test]
    fn credentials_are_bound_to_the_connection_target() {
        let profile = SshProfile {
            id: "profile-one".into(),
            name: "Pi".into(),
            host: "192.168.1.10".into(),
            port: 22,
            username: "alex".into(),
            identity_file: None,
            tags: Vec::new(),
            last_connected_at: None,
            has_saved_password: false,
        };
        let mut changed = profile.clone();
        changed.host = "192.168.1.11".into();
        assert_ne!(account(&profile).unwrap(), account(&changed).unwrap());
    }
}
