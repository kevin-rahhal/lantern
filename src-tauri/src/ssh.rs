use std::{
    collections::HashMap,
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tauri::ipc::Channel;
use uuid::Uuid;

use crate::{
    background_process, credentials,
    models::{SshProfile, TerminalEvent},
    profiles,
};

struct SessionHandle {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

#[derive(Clone, Default)]
pub struct SshState {
    sessions: Arc<Mutex<HashMap<String, Arc<SessionHandle>>>>,
}

impl SshState {
    pub fn start(
        &self,
        profile: SshProfile,
        cols: u16,
        rows: u16,
        on_event: Channel<TerminalEvent>,
    ) -> Result<String, String> {
        profiles::validate(&profile)?;
        if !available() {
            return Err(
                "OpenSSH client was not found. Install the operating system's OpenSSH client first."
                    .into(),
            );
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(pty_size(cols, rows))
            .map_err(|error| format!("Could not open a terminal: {error}"))?;
        let mut command = CommandBuilder::new("ssh");
        command.env("TERM", "xterm-256color");
        command.arg("-tt");
        command.arg("-p");
        command.arg(profile.port.to_string());
        command.arg("-o");
        command.arg("ConnectTimeout=10");
        command.arg("-o");
        command.arg("ServerAliveInterval=15");
        command.arg("-o");
        command.arg("ServerAliveCountMax=3");
        if credentials::has(&profile).unwrap_or(false) {
            let executable = std::env::current_exe()
                .map_err(|error| format!("Could not prepare secure password access: {error}"))?;
            let (account_key, account) = credentials::configure_askpass(&profile)?;
            command.env("SSH_ASKPASS", executable);
            command.env("SSH_ASKPASS_REQUIRE", "force");
            command.env(account_key, account);
            command.arg("-o");
            command.arg("StrictHostKeyChecking=accept-new");
        }
        if let Some(identity_file) = profile
            .identity_file
            .as_deref()
            .filter(|path| !path.trim().is_empty())
        {
            command.arg("-i");
            command.arg(expand_home(identity_file));
        }
        command.arg(format!("{}@{}", profile.username, profile.host));

        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("Could not start OpenSSH: {error}"))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("Could not read from the terminal: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("Could not write to the terminal: {error}"))?;
        let killer = child.clone_killer();
        let id = Uuid::new_v4().to_string();
        let handle = Arc::new(SessionHandle {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
        });
        self.sessions
            .lock()
            .map_err(|_| "SSH session state is unavailable".to_string())?
            .insert(id.clone(), handle);

        let session_id = id.clone();
        let sessions = Arc::clone(&self.sessions);
        thread::spawn(move || {
            let mut buffer = [0_u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(length) => {
                        let data = String::from_utf8_lossy(&buffer[..length]).into_owned();
                        if on_event.send(TerminalEvent::Data { data }).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let session_open = sessions
                            .lock()
                            .map(|open| open.contains_key(&session_id))
                            .unwrap_or(false);
                        if session_open {
                            let _ = on_event.send(TerminalEvent::Error {
                                message: format!("Terminal read failed: {error}"),
                            });
                        }
                        break;
                    }
                }
            }
            let code = child.wait().ok().map(|status| status.exit_code());
            if let Ok(mut open) = sessions.lock() {
                open.remove(&session_id);
            }
            let _ = on_event.send(TerminalEvent::Exit { code });
        });

        Ok(id)
    }

    pub fn write(&self, session_id: &str, data: &str) -> Result<(), String> {
        if data.len() > 1_048_576 {
            return Err("Terminal input is too large".into());
        }
        let session = self.session(session_id)?;
        let mut writer = session
            .writer
            .lock()
            .map_err(|_| "Terminal writer is unavailable".to_string())?;
        writer
            .write_all(data.as_bytes())
            .and_then(|_| writer.flush())
            .map_err(|error| format!("Could not write to the SSH session: {error}"))
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let session = self.session(session_id)?;
        let master = session
            .master
            .lock()
            .map_err(|_| "Terminal resize handle is unavailable".to_string())?;
        master
            .resize(pty_size(cols, rows))
            .map_err(|error| format!("Could not resize the terminal: {error}"))
    }

    pub fn close(&self, session_id: &str) -> Result<(), String> {
        let session = self
            .sessions
            .lock()
            .map_err(|_| "SSH session state is unavailable".to_string())?
            .remove(session_id);
        if let Some(session) = session {
            let close_result = session
                .killer
                .lock()
                .map_err(|_| "SSH process handle is unavailable".to_string())?
                .kill();
            if let Err(error) = close_result {
                // Windows' ConPTY can report ERROR_SUCCESS as an error when the
                // process exits between our session removal and the kill call.
                // The requested end state is already true, so this race is safe.
                if !is_benign_close_error(&error) {
                    return Err(format!("Could not close the SSH session: {error}"));
                }
            }
        }
        Ok(())
    }

    fn session(&self, session_id: &str) -> Result<Arc<SessionHandle>, String> {
        self.sessions
            .lock()
            .map_err(|_| "SSH session state is unavailable".to_string())?
            .get(session_id)
            .cloned()
            .ok_or_else(|| "SSH session is no longer open".to_string())
    }
}

fn is_benign_close_error(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(0)
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.clamp(2, 500),
        cols: cols.clamp(2, 500),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn expand_home(path: &str) -> OsString {
    let Some(remainder) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) else {
        return OsString::from(path);
    };
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    home.map(|directory| PathBuf::from(directory).join(remainder).into_os_string())
        .unwrap_or_else(|| OsString::from(path))
}

pub fn available() -> bool {
    background_process::command("ssh")
        .arg("-V")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_success_code_is_benign_during_close_race() {
        assert!(is_benign_close_error(&std::io::Error::from_raw_os_error(0)));
        assert!(!is_benign_close_error(&std::io::Error::from_raw_os_error(
            5
        )));
    }
}
