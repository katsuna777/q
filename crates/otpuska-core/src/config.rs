//! Per-machine preferences (theme, scale, database location) and where files live.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const APP_DIR: &str = "Otpuska";

/// `OTPUSKA_HOME` overrides the location; otherwise the machine-local data directory
/// (`~/Library/Application Support/Otpuska`, `%LOCALAPPDATA%\Otpuska`, `~/.local/share/Otpuska`).
/// Local, not roaming: a SQLite file in WAL mode must not be synced or kept on a network share.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OTPUSKA_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir().or_else(dirs::data_dir).unwrap_or_else(|| PathBuf::from(".")).join(APP_DIR)
}

pub fn config_file() -> PathBuf {
    data_dir().join("config.json")
}

pub fn default_db_path() -> PathBuf {
    data_dir().join("otpuska.db")
}

pub fn backups_dir(db_path: &Path) -> PathBuf {
    db_path.parent().unwrap_or_else(|| Path::new(".")).join("backups")
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Ui {
    /// "system" | "dark" | "light"
    pub theme: Option<String>,
    /// "mono" | "blue" | "violet" | "green" | "orange"
    pub accent: Option<String>,
    pub scale: Option<f32>,
    pub reduce_motion: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub ui: Ui,
    /// Database file chosen by the user; the default location when empty.
    #[serde(default)]
    pub db_path: Option<PathBuf>,
    /// Last folder used in file dialogs.
    #[serde(default)]
    pub last_dir: Option<PathBuf>,
}

impl Config {
    pub fn load() -> Config {
        std::fs::read_to_string(config_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let file = config_file();
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&file, serde_json::to_string_pretty(self)?).with_context(|| format!("cannot write {}", file.display()))
    }

    pub fn db_path(&self) -> PathBuf {
        self.db_path.clone().unwrap_or_else(default_db_path)
    }
}

/// Keeps one backup per day (the first start of the day) and the last `keep` of them.
/// Returns the file written, if any.
pub fn auto_backup(db: &crate::db::Db, today: crate::date::Date, keep: usize) -> Result<Option<PathBuf>> {
    if db.path() == Path::new(":memory:") || db.is_empty()? {
        return Ok(None);
    }
    let dir = backups_dir(db.path());
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(format!("otpuska-{}.db", crate::date::iso(today)));
    let written = if target.exists() {
        None
    } else {
        db.backup_to(&target)?;
        Some(target)
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("otpuska-") && n.ends_with(".db")))
        .collect();
    files.sort();
    if files.len() > keep {
        for old in &files[..files.len() - keep] {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(written)
}

/// Replaces the database file with a verified copy. The caller must have closed `Db` first
/// (and reopens it afterwards). The old file is kept next to it as `*.before-restore`.
pub fn restore_file(db_path: &Path, backup: &Path) -> Result<()> {
    crate::db::Db::verify_file(backup)?;
    if db_path.exists() {
        let saved = db_path.with_extension("before-restore");
        let _ = std::fs::remove_file(&saved);
        std::fs::rename(db_path, &saved).context("cannot move the current database aside")?;
    }
    for ext in ["db-wal", "db-shm"] {
        let _ = std::fs::remove_file(db_path.with_extension(ext));
    }
    std::fs::copy(backup, db_path).context("cannot copy the backup")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::model::Employee;

    #[test]
    fn config_json_roundtrip_and_defaults() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c, Config::default());
        let c = Config { ui: Ui { theme: Some("dark".into()), ..Default::default() }, ..Default::default() };
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn daily_backup_is_kept_and_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("otpuska.db")).unwrap();
        let d = |day| crate::date::Date::from_ymd_opt(2026, 10, day).unwrap();
        // Nothing to back up yet.
        assert!(auto_backup(&db, d(1), 3).unwrap().is_none());
        db.insert_employee(&Employee { fio: "А".into(), ..Default::default() }).unwrap();
        assert!(auto_backup(&db, d(1), 3).unwrap().is_some());
        assert!(auto_backup(&db, d(1), 3).unwrap().is_none()); // once per day
        for day in 2..=5 {
            auto_backup(&db, d(day), 3).unwrap();
        }
        let names: Vec<_> = std::fs::read_dir(backups_dir(db.path())).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names.len(), 3);
        assert!(!names.contains(&"otpuska-2026-10-01.db".to_string()));
    }

    #[test]
    fn restore_swaps_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("otpuska.db");
        let backup = dir.path().join("b.db");
        {
            let db = Db::open(&path).unwrap();
            db.insert_employee(&Employee { fio: "Старый".into(), ..Default::default() }).unwrap();
            db.backup_to(&backup).unwrap();
            db.insert_employee(&Employee { fio: "Новый".into(), ..Default::default() }).unwrap();
        }
        restore_file(&path, &backup).unwrap();
        assert_eq!(Db::open(&path).unwrap().employees().unwrap().len(), 1);
        assert!(path.with_extension("before-restore").exists());
        assert!(restore_file(&path, &dir.path().join("missing.db")).is_err());
    }
}
