use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Settings {
    pub users: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KeyInfo {
    pub creation_date: String,
    pub password_storage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug)]
pub struct KeyDir {
    /// Full directory name (e.g. "github-work_2026-03-15_14-32_UTC+0530")
    pub dir_name: String,
    pub path: PathBuf,
    pub info: KeyInfo,
}

impl KeyDir {
    pub fn key_id(&self) -> &str {
        // key-id is everything before the first `_YYYY` date pattern
        // dir_name = "<key-id>_<date>"
        // date starts with 4-digit year, so find `_20` or `_19` etc.
        // Simplest: split on `_` and recombine until we hit a 4-digit year segment
        let parts: Vec<&str> = self.dir_name.splitn(2, '_').collect();
        if parts.len() >= 2 {
            parts[0]
        } else {
            &self.dir_name
        }
    }
}

pub struct State {
    pub key_dir: PathBuf,
    pub settings: Settings,
    pub keys: Vec<KeyDir>,
}

impl State {
    pub fn load(key_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(key_dir)
            .with_context(|| format!("Creating key dir {}", key_dir.display()))?;
        std::fs::create_dir_all(key_dir.join("keys"))
            .with_context(|| format!("Creating keys subdir"))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(key_dir, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("Setting permissions on {}", key_dir.display()))?;
            std::fs::set_permissions(key_dir.join("keys"), std::fs::Permissions::from_mode(0o700))
                .context("Setting permissions on keys subdir")?;
        }

        let settings = load_settings(key_dir)?;
        let keys = load_keys(key_dir)?;

        Ok(State {
            key_dir: key_dir.to_path_buf(),
            settings,
            keys,
        })
    }

    pub fn save_settings(&self) -> Result<()> {
        save_settings(&self.key_dir, &self.settings)
    }

    pub fn keys_path(&self) -> PathBuf {
        self.key_dir.join("keys")
    }
}

fn load_settings(key_dir: &Path) -> Result<Settings> {
    let path = key_dir.join("settings.json");
    if !path.exists() {
        return Ok(Settings::default());
    }
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("Reading {}", path.display()))?;
    let settings: Settings =
        serde_json::from_str(&content).with_context(|| format!("Parsing {}", path.display()))?;
    Ok(settings)
}

fn save_settings(key_dir: &Path, settings: &Settings) -> Result<()> {
    let path = key_dir.join("settings.json");
    let content = serde_json::to_string_pretty(settings)?;
    std::fs::write(&path, content).with_context(|| format!("Writing {}", path.display()))?;
    Ok(())
}

fn load_keys(key_dir: &Path) -> Result<Vec<KeyDir>> {
    let keys_path = key_dir.join("keys");
    let mut keys = Vec::new();

    let entries = match std::fs::read_dir(&keys_path) {
        Ok(e) => e,
        Err(_) => return Ok(keys),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if dir_name.is_empty() {
            continue;
        }
        let info_path = path.join("info.json");
        if !info_path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&info_path)
            .with_context(|| format!("Reading {}", info_path.display()))?;
        let info: KeyInfo = serde_json::from_str(&content)
            .with_context(|| format!("Parsing {}", info_path.display()))?;
        keys.push(KeyDir {
            dir_name,
            path,
            info,
        });
    }

    keys.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    Ok(keys)
}

pub fn write_info(dir: &Path, info: &KeyInfo) -> Result<()> {
    let path = dir.join("info.json");
    let content = serde_json::to_string_pretty(info)?;
    std::fs::write(&path, content).with_context(|| format!("Writing {}", path.display()))?;
    Ok(())
}

/// Returns the date string for a key directory name in the format:
/// `YYYY-MM-DD_HH-MM_UTC±HHMM` (colon stripped from offset, safe for filenames)
pub fn current_date_string() -> String {
    use chrono::Local;
    let now = Local::now();
    // Format: 2026-03-15_14-32_UTC+0530
    // %z gives +0530 (no colon), already safe for filenames
    now.format("%Y-%m-%d_%H-%M_UTC%z").to_string()
}
