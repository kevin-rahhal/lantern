mod credentials;
mod discovery;
mod models;
mod profiles;
mod ssh;

use models::{Device, NetworkInterface, ProfileStatus, ScanEvent, SshProfile, TerminalEvent};
use ssh::SshState;
use tauri::{ipc::Channel, AppHandle, State};
use zeroize::Zeroizing;

pub fn askpass_exit_code() -> Option<i32> {
    credentials::askpass_exit_code()
}

#[tauri::command]
fn list_interfaces() -> Result<Vec<NetworkInterface>, String> {
    discovery::interfaces()
}

#[tauri::command]
async fn scan_network(cidr: String, on_event: Channel<ScanEvent>) -> Result<Vec<Device>, String> {
    discovery::scan(&cidr, &on_event).await
}

#[tauri::command]
fn load_profiles(app: AppHandle) -> Result<Vec<SshProfile>, String> {
    profiles::load(&app)
}

#[tauri::command]
fn save_profile(app: AppHandle, profile: SshProfile) -> Result<Vec<SshProfile>, String> {
    if let Some(existing) = profiles::load(&app)?
        .into_iter()
        .find(|existing| existing.id == profile.id)
    {
        let target_changed = existing.username != profile.username
            || existing.host != profile.host
            || existing.port != profile.port;
        if target_changed {
            credentials::delete(&existing)?;
        }
    }
    profiles::save(&app, profile)
}

#[tauri::command]
fn delete_profile(app: AppHandle, id: String) -> Result<Vec<SshProfile>, String> {
    if let Some(profile) = profiles::load(&app)?
        .into_iter()
        .find(|profile| profile.id == id)
    {
        credentials::delete(&profile)?;
    }
    profiles::delete(&app, &id)
}

#[tauri::command]
fn save_password(app: AppHandle, profile_id: String, password: String) -> Result<(), String> {
    let password = Zeroizing::new(password);
    let profile = profiles::load(&app)?
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "Save the SSH profile before storing its password".to_string())?;
    credentials::store(&profile, &password)
}

#[tauri::command]
fn delete_password(app: AppHandle, profile_id: String) -> Result<(), String> {
    let profile = profiles::load(&app)?
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "The SSH profile no longer exists".to_string())?;
    credentials::delete(&profile)
}

#[tauri::command]
async fn check_profile_statuses(profiles: Vec<SshProfile>) -> Vec<ProfileStatus> {
    discovery::profile_statuses(profiles).await
}

#[tauri::command]
fn ssh_available() -> bool {
    ssh::available()
}

#[tauri::command]
fn start_ssh(
    profile: SshProfile,
    cols: u16,
    rows: u16,
    on_event: Channel<TerminalEvent>,
    state: State<'_, SshState>,
) -> Result<String, String> {
    state.start(profile, cols, rows, on_event)
}

#[tauri::command]
fn write_ssh(session_id: String, data: String, state: State<'_, SshState>) -> Result<(), String> {
    state.write(&session_id, &data)
}

#[tauri::command]
fn resize_ssh(
    session_id: String,
    cols: u16,
    rows: u16,
    state: State<'_, SshState>,
) -> Result<(), String> {
    state.resize(&session_id, cols, rows)
}

#[tauri::command]
fn close_ssh(session_id: String, state: State<'_, SshState>) -> Result<(), String> {
    state.close(&session_id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(SshState::default())
        .invoke_handler(tauri::generate_handler![
            list_interfaces,
            scan_network,
            load_profiles,
            save_profile,
            delete_profile,
            save_password,
            delete_password,
            check_profile_statuses,
            ssh_available,
            start_ssh,
            write_ssh,
            resize_ssh,
            close_ssh,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lantern");
}
