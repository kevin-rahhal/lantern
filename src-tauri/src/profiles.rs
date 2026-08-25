use std::{fs, path::PathBuf};

use tauri::{AppHandle, Manager};

use crate::{credentials, models::SshProfile};

fn profiles_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve the app data directory: {error}"))?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create the app data directory: {error}"))?;
    Ok(directory.join("profiles.json"))
}

pub fn load(app: &AppHandle) -> Result<Vec<SshProfile>, String> {
    let path = profiles_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("Could not read SSH profiles: {error}"))?;
    let mut profiles: Vec<SshProfile> = serde_json::from_str(&contents)
        .map_err(|error| format!("SSH profile data is invalid: {error}"))?;
    for profile in &mut profiles {
        profile.has_saved_password = credentials::has(profile).unwrap_or(false);
    }
    Ok(profiles)
}

fn write(app: &AppHandle, profiles: &[SshProfile]) -> Result<(), String> {
    let path = profiles_path(app)?;
    let contents = serde_json::to_string_pretty(profiles)
        .map_err(|error| format!("Could not serialize SSH profiles: {error}"))?;
    fs::write(path, contents).map_err(|error| format!("Could not save SSH profiles: {error}"))
}

pub fn save(app: &AppHandle, mut profile: SshProfile) -> Result<Vec<SshProfile>, String> {
    validate(&profile)?;
    profile.has_saved_password = credentials::has(&profile).unwrap_or(false);
    let mut profiles = load(app)?;
    if let Some(existing) = profiles.iter_mut().find(|item| item.id == profile.id) {
        *existing = profile;
    } else {
        profiles.push(profile);
    }
    profiles.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    write(app, &profiles)?;
    Ok(profiles)
}

pub fn delete(app: &AppHandle, id: &str) -> Result<Vec<SshProfile>, String> {
    let mut profiles = load(app)?;
    profiles.retain(|profile| profile.id != id);
    write(app, &profiles)?;
    Ok(profiles)
}

pub fn validate(profile: &SshProfile) -> Result<(), String> {
    if profile.id.is_empty()
        || profile.id.len() > 128
        || !profile
            .id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err("Profile identifier is invalid".into());
    }
    if profile.name.trim().is_empty() {
        return Err("Profile name cannot be empty".into());
    }
    if profile.username.is_empty()
        || profile.username.len() > 64
        || !profile
            .username
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err("Username contains unsupported characters".into());
    }
    if profile.host.is_empty()
        || profile.host.len() > 253
        || !profile
            .host
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".:_-[]".contains(character))
    {
        return Err("Host contains unsupported characters".into());
    }
    if profile.port == 0 {
        return Err("Port must be between 1 and 65535".into());
    }
    Ok(())
}
