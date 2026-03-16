use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::effects::Effects;

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
    pub fn load(key_dir: &Path, fx: &dyn Effects) -> Result<Self> {
        fx.create_dir_all(key_dir)
            .with_context(|| format!("Creating key dir {}", key_dir.display()))?;
        fx.create_dir_all(&key_dir.join("keys"))
            .with_context(|| format!("Creating keys subdir"))?;

        fx.set_permissions(key_dir, 0o700)
            .with_context(|| format!("Setting permissions on {}", key_dir.display()))?;
        fx.set_permissions(&key_dir.join("keys"), 0o700)
            .context("Setting permissions on keys subdir")?;

        let settings = load_settings(key_dir, fx)?;
        let keys = load_keys(key_dir, fx)?;

        Ok(State {
            key_dir: key_dir.to_path_buf(),
            settings,
            keys,
        })
    }

    pub fn save_settings(&self, fx: &dyn Effects) -> Result<()> {
        let path = self.key_dir.join("settings.json");
        let content = serde_json::to_string_pretty(&self.settings)?;
        fx.write_file(&path, content.as_bytes())
    }

    pub fn keys_path(&self) -> PathBuf {
        self.key_dir.join("keys")
    }
}

fn load_settings(key_dir: &Path, fx: &dyn Effects) -> Result<Settings> {
    let path = key_dir.join("settings.json");
    if !fx.path_exists(&path) {
        return Ok(Settings::default());
    }
    let content = fx
        .read_file_string(&path)
        .with_context(|| format!("Reading {}", path.display()))?;
    let settings: Settings =
        serde_json::from_str(&content).with_context(|| format!("Parsing {}", path.display()))?;
    Ok(settings)
}

fn load_keys(key_dir: &Path, fx: &dyn Effects) -> Result<Vec<KeyDir>> {
    let keys_path = key_dir.join("keys");
    let mut keys = Vec::new();

    let names = match fx.read_dir_names(&keys_path) {
        Ok(n) => n,
        Err(_) => return Ok(keys),
    };

    for dir_name in names {
        let path = keys_path.join(&dir_name);
        if !fx.is_dir(&path) {
            continue;
        }
        let info_path = path.join("info.json");
        if !fx.path_exists(&info_path) {
            continue;
        }
        let content = fx
            .read_file_string(&info_path)
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

pub fn write_info(dir: &Path, info: &KeyInfo, fx: &dyn Effects) -> Result<()> {
    let path = dir.join("info.json");
    let content = serde_json::to_string_pretty(info)?;
    fx.write_file(&path, content.as_bytes())
}
