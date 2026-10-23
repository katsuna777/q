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
    /// Database file of the single-database versions (1.0.x); the default location when empty.
    /// Kept as it was, only read once to register that file in `databases`.
    #[serde(default)]
    pub db_path: Option<PathBuf>,
    /// Last folder used in file dialogs.
    #[serde(default)]
    pub last_dir: Option<PathBuf>,
    /// Every database the user can open (one per organization), see `databases.rs`.
    #[serde(default)]
    pub databases: Vec<crate::databases::DbEntry>,
    /// Id of the database opened last.
    #[serde(default)]
    pub last_db: Option<String>,
    /// Show the database chooser at every start (the default); otherwise the last one opens at once.
    #[serde(default)]
    pub ask_on_start: Option<bool>,
    /// Data folder override for tests; the real one is `data_dir()`.
    #[serde(skip)]
    pub home: Option<PathBuf>,
}

impl Config {
    /// The saved configuration; defaults when there is none. A file that cannot be read is kept
    /// next to it as `config.json.broken` (the databases it listed are found again by `migrate`).
    pub fn load() -> Config {
        let file = config_file();
        let Ok(text) = std::fs::read_to_string(&file) else { return Config::default() };
        match serde_json::from_str(&text) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("otpuska: config.json не прочитан ({e}); он сохранён как config.json.broken");
                let _ = std::fs::copy(&file, file.with_extension("json.broken"));
                Config::default()
            }
        }
    }

    /// Writes the file whole or not at all: a crash or a power cut in the middle must not leave
    /// half a file (it holds the list of the databases).
    pub fn save(&self) -> Result<()> {
        use std::io::Write;
        let file = self.home().join("config.json");
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = file.with_extension("json.tmp");
        let written = std::fs::File::create(&tmp).and_then(|mut f| f.write_all(json.as_bytes()).and_then(|_| f.sync_all()));
        // Replacing by rename is atomic (also on Windows); if something holds the file, write in place.
        match written.and_then(|_| std::fs::rename(&tmp, &file)) {
            Ok(()) => Ok(()),
            Err(_) => {
                let _ = std::fs::remove_file(&tmp);
                std::fs::write(&file, json).with_context(|| format!("cannot write {}", file.display()))
            }
        }
    }

    /// The database file of versions 1.0.x (see `databases` for the list used now).
    pub fn db_path(&self) -> PathBuf {
        self.db_path.clone().unwrap_or_else(|| self.home().join("otpuska.db"))
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
    // Only the daily files take part in pruning: "before import / before update" copies stay.
    let mut files: Vec<PathBuf> =
        std::fs::read_dir(&dir)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(is_daily_backup)).collect();
    files.sort();
    if files.len() > keep {
        for old in &files[..files.len() - keep] {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(written)
}

/// `otpuska-2026-10-22.db`
fn is_daily_backup(name: &str) -> bool {
    let Some(date) = name.strip_prefix("otpuska-").and_then(|n| n.strip_suffix(".db")) else { return false };
    date.len() == 10 && date.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })
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
    fn config_is_saved_whole_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Config { home: Some(dir.path().to_path_buf()), ..Default::default() };
        c.ui.theme = Some("light".into());
        c.save().unwrap();
        c.ui.theme = Some("dark".into());
        c.save().unwrap(); // replaces the first one
        let back: Config = serde_json::from_str(&std::fs::read_to_string(dir.path().join("config.json")).unwrap()).unwrap();
        assert_eq!(back.ui.theme.as_deref(), Some("dark"));
        let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names, ["config.json"]);
    }

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
    fn pruning_leaves_other_copies_alone() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("otpuska.db")).unwrap();
        db.insert_employee(&Employee { fio: "А".into(), ..Default::default() }).unwrap();
        let backups = backups_dir(db.path());
        std::fs::create_dir_all(&backups).unwrap();
        for name in ["otpuska-before-import-2026-10-01-101010.db", "otpuska-before-update-2026-10-02-101010.db", "notes.txt"] {
            std::fs::write(backups.join(name), b"x").unwrap();
        }
        for day in 1..=4 {
            auto_backup(&db, crate::date::Date::from_ymd_opt(2026, 10, day).unwrap(), 2).unwrap();
        }
        let mut names: Vec<_> = std::fs::read_dir(&backups).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        assert_eq!(
            names,
            ["notes.txt", "otpuska-2026-10-03.db", "otpuska-2026-10-04.db", "otpuska-before-import-2026-10-01-101010.db", "otpuska-before-update-2026-10-02-101010.db"]
        );
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
