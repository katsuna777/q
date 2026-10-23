//! Several databases, one per organization. The list lives in `config.json`; every database is its
//! own SQLite file with the usual schema, so it can be copied, backed up or restored on its own.
//!
//! Upgrading from 1.0.x: the single `otpuska.db` stays exactly where it is and becomes the first
//! entry ("Основная база"). Registering it moves and rewrites nothing; a copy in the old format is
//! saved first. The file itself is brought to the current schema when it is opened for work
//! (`Db::open`), again after a copy.

use crate::config::{self, Config};
use crate::date::Date;
use crate::db::{with_suffix, Db};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Name given to the database that already exists when the list is created.
pub const FIRST_NAME: &str = "Основная база";
const FIRST_ID: &str = "main";
const MAX_NAME: usize = 60;
/// Deleted databases are kept this long in `deleted/`, then removed for good.
pub const KEEP_DELETED_DAYS: i64 = 30;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DbEntry {
    pub id: String,
    pub name: String,
    /// Relative to the data folder (so the folder can be moved as a whole), or absolute.
    pub path: PathBuf,
}

impl Config {
    /// The data folder: `data_dir()` unless a test points elsewhere.
    pub fn home(&self) -> PathBuf {
        self.home.clone().unwrap_or_else(config::data_dir)
    }

    fn resolve(&self, p: &Path) -> PathBuf {
        if p.is_relative() {
            self.home().join(p)
        } else {
            p.to_path_buf()
        }
    }

    fn relative(&self, p: &Path) -> PathBuf {
        p.strip_prefix(self.home()).map(Path::to_path_buf).unwrap_or_else(|_| p.to_path_buf())
    }

    pub fn db_file(&self, e: &DbEntry) -> PathBuf {
        self.resolve(&e.path)
    }

    pub fn find_db(&self, id: &str) -> Option<&DbEntry> {
        self.databases.iter().find(|d| d.id == id)
    }

    pub fn ask_on_start(&self) -> bool {
        self.ask_on_start.unwrap_or(true)
    }

    /// Brings an older configuration up to date; `true` when it changed and has to be saved.
    /// The 1.0.x database file is registered in place, after a safety copy next to the daily ones.
    pub fn migrate(&mut self) -> bool {
        let mut changed = false;
        if self.databases.is_empty() {
            let legacy = self.db_path();
            if legacy.is_file() {
                self.databases.push(DbEntry { id: FIRST_ID.into(), name: FIRST_NAME.into(), path: self.relative(&legacy) });
                safety_copy(&legacy);
                changed = true;
            }
        }
        changed |= self.recover_unlisted();
        let known = self.last_db.as_deref().is_some_and(|id| self.find_db(id).is_some());
        if !known {
            let first = self.databases.first().map(|d| d.id.clone());
            changed |= self.last_db != first;
            self.last_db = first;
        }
        changed
    }

    /// Databases that lie in `databases/<id>/` but are not in the list (the configuration was lost
    /// or could not be read) are added again, under a name made of their id — the data is never
    /// left out of sight. Returns `true` when something was added.
    fn recover_unlisted(&mut self) -> bool {
        let Ok(items) = std::fs::read_dir(self.home().join("databases")) else { return false };
        let mut found: Vec<String> = items
            .flatten()
            .filter(|i| i.path().join("otpuska.db").is_file())
            .filter_map(|i| i.file_name().into_string().ok())
            .filter(|id| self.find_db(id).is_none())
            .collect();
        found.sort();
        let added = !found.is_empty();
        for id in found {
            let name = self.free_name(&format!("Восстановленная база {id}"));
            let path = Path::new("databases").join(&id).join("otpuska.db");
            self.databases.push(DbEntry { id, name, path });
        }
        added
    }

    /// `base`, or `base 2`, `base 3`… when the name is taken.
    fn free_name(&self, base: &str) -> String {
        let mut name = base.to_string();
        let mut n = 1;
        while self.clean_name(&name, None).is_err() {
            n += 1;
            name = format!("{base} {n}");
        }
        name
    }

    /// The database to open at start without asking, if there is a clear answer:
    /// the only one, or (when asking is switched off) the one used last.
    pub fn start_choice(&self) -> Option<String> {
        match self.databases.as_slice() {
            [] => None,
            [only] => Some(only.id.clone()),
            _ if self.ask_on_start() => None,
            _ => self.last_db.clone().filter(|id| self.find_db(id).is_some()),
        }
    }

    /// Trims and checks a database name (not empty, not too long, not taken — letter case ignored).
    fn clean_name(&self, name: &str, except: Option<&str>) -> Result<String> {
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            bail!("Введите название базы");
        }
        if name.chars().count() > MAX_NAME {
            bail!("Название слишком длинное — не больше {MAX_NAME} знаков");
        }
        let key = name.to_lowercase();
        if self.databases.iter().any(|d| Some(d.id.as_str()) != except && d.name.to_lowercase() == key) {
            bail!("База «{name}» уже есть — выберите другое название");
        }
        Ok(name)
    }

    fn new_id(&self) -> String {
        let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let base = format!("{millis:x}");
        let mut id = base.clone();
        let mut n = 1;
        while self.find_db(&id).is_some() || self.home().join("databases").join(&id).exists() {
            n += 1;
            id = format!("{base}-{n}");
        }
        id
    }

    fn new_entry(&self, name: String) -> (DbEntry, PathBuf) {
        let id = self.new_id();
        let path = Path::new("databases").join(&id).join("otpuska.db");
        let file = self.resolve(&path);
        (DbEntry { id, name, path }, file)
    }

    /// A new empty database in its own folder. The caller saves the configuration.
    pub fn create_database(&mut self, name: &str) -> Result<String> {
        let name = self.clean_name(name, None)?;
        let (entry, file) = self.new_entry(name);
        if let Err(e) = Db::open(&file) {
            remove_folder_of(&file);
            return Err(e.context("Не удалось создать базу"));
        }
        let id = entry.id.clone();
        self.databases.push(entry);
        Ok(id)
    }

    /// Adds a copy of a database file (a backup, or a file from another computer) under a new name.
    pub fn add_database_file(&mut self, name: &str, source: &Path) -> Result<String> {
        let name = self.clean_name(name, None)?;
        Db::verify_file(source)?;
        let same = std::fs::canonicalize(source).ok();
        if same.is_some() && self.databases.iter().any(|d| std::fs::canonicalize(self.db_file(d)).ok() == same) {
            bail!("Эта база уже есть в списке");
        }
        let (entry, file) = self.new_entry(name);
        let copied = std::fs::create_dir_all(file.parent().unwrap_or(Path::new("."))).and_then(|_| std::fs::copy(source, &file));
        if let Err(e) = copied {
            remove_folder_of(&file);
            return Err(anyhow::Error::new(e).context("Не удалось скопировать файл базы"));
        }
        if let Err(e) = Db::open(&file) {
            remove_folder_of(&file);
            return Err(e);
        }
        let id = entry.id.clone();
        self.databases.push(entry);
        Ok(id)
    }

    pub fn rename_database(&mut self, id: &str, name: &str) -> Result<()> {
        let name = self.clean_name(name, Some(id))?;
        let Some(entry) = self.databases.iter_mut().find(|d| d.id == id) else { bail!("Такой базы нет в списке") };
        entry.name = name;
        Ok(())
    }

    /// Takes a database off the list and moves its files to `deleted/` (kept `KEEP_DELETED_DAYS`
    /// days, in case of a mistake). Returns that folder. The last database cannot be removed.
    pub fn remove_database(&mut self, id: &str) -> Result<Option<PathBuf>> {
        let Some(entry) = self.find_db(id).cloned() else { bail!("Такой базы нет в списке") };
        if self.databases.len() <= 1 {
            bail!("Последнюю базу удалить нельзя");
        }
        let file = self.db_file(&entry);
        let mut kept = None;
        if file.exists() {
            let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
            let dest = self.home().join("deleted").join(format!("{}-{stamp}", entry.id));
            std::fs::create_dir_all(&dest).with_context(|| format!("Не удалось создать {}", dest.display()))?;
            for suffix in ["", "-wal", "-shm"] {
                let src = with_suffix(&file, suffix);
                if let Some(name) = src.file_name().filter(|_| src.exists()) {
                    move_file(&src, &dest.join(name)).with_context(|| format!("Не удалось убрать файл {}", src.display()))?;
                }
            }
            let backups = config::backups_dir(&file);
            if backups.is_dir() {
                let _ = std::fs::rename(&backups, dest.join("backups"));
            }
            if file.parent().is_some_and(|p| p.starts_with(self.home().join("databases"))) {
                remove_folder_of(&file); // only if it is empty by now
            }
            kept = Some(dest);
        }
        self.databases.retain(|d| d.id != id);
        if self.last_db.as_deref() == Some(id) {
            self.last_db = self.databases.first().map(|d| d.id.clone());
        }
        Ok(kept)
    }

    /// Drops an entry whose file is gone (deleted or moved by hand) from the list.
    pub fn forget_database(&mut self, id: &str) -> Result<()> {
        let Some(entry) = self.find_db(id).cloned() else { return Ok(()) };
        if self.db_file(&entry).exists() {
            bail!("Файл базы на месте — удалите её в «Настройках»");
        }
        self.databases.retain(|d| d.id != id);
        if self.last_db.as_deref() == Some(id) {
            self.last_db = self.databases.first().map(|d| d.id.clone());
        }
        Ok(())
    }

    /// Opens a database for work: checks the file is there (a missing one is never re-created
    /// silently), takes the daily backup and remembers it as the last used.
    pub fn open_database(&mut self, id: &str, today: Date) -> Result<Db> {
        let Some(entry) = self.find_db(id).cloned() else { bail!("Такой базы нет в списке") };
        let file = self.db_file(&entry);
        if !file.is_file() {
            bail!("Файл базы «{}» не найден:\n{}\nВозможно, его удалили или переместили.", entry.name, file.display());
        }
        let db = Db::open(&file).with_context(|| format!("Не удалось открыть базу «{}»", entry.name))?;
        if let Err(e) = config::auto_backup(&db, today, 14) {
            eprintln!("otpuska: автокопия не создана: {e:#}");
        }
        self.last_db = Some(entry.id);
        Ok(db)
    }

    /// Removes what has been in `deleted/` longer than `KEEP_DELETED_DAYS`.
    pub fn purge_deleted(&self, today: Date) {
        let Ok(items) = std::fs::read_dir(self.home().join("deleted")) else { return };
        for item in items.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if deleted_on(&name).is_some_and(|d| (today - d).num_days() > KEEP_DELETED_DAYS) {
                let _ = std::fs::remove_dir_all(item.path());
            }
        }
    }
}

/// `<id>-2026-10-22-103045` → the day it was deleted.
fn deleted_on(folder: &str) -> Option<Date> {
    let stamp = folder.get(folder.len().checked_sub(17)?..)?;
    chrono::NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d-%H%M%S").ok().map(|t| t.date())
}

/// One copy right after the first start of the multi-database version, next to the daily backups.
fn safety_copy(file: &Path) {
    // As it is: the copy must stay a file of the old version, which the file itself will stop being
    // the first time it is opened for work.
    let Ok(db) = Db::open_as_is(file) else { return };
    if db.is_empty().unwrap_or(true) {
        return;
    }
    let dir = config::backups_dir(file);
    if std::fs::create_dir_all(&dir).is_ok() {
        let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
        let _ = db.backup_to(&dir.join(format!("otpuska-before-update-{stamp}.db")));
    }
}

/// Removes the (empty) folder a database file was in; a folder with anything left stays.
fn remove_folder_of(file: &Path) {
    if let Some(dir) = file.parent() {
        let _ = std::fs::remove_dir(dir);
    }
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to).or_else(|_| std::fs::copy(from, to).and_then(|_| std::fs::remove_file(from)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Employee;

    fn day(m: u32, d: u32) -> Date {
        Date::from_ymd_opt(2026, m, d).unwrap()
    }

    fn config_in(home: &Path) -> Config {
        Config { home: Some(home.to_path_buf()), ..Default::default() }
    }

    /// What version 1.0.x leaves behind: `otpuska.db` (first schema, three employees) and a
    /// `config.json` without any list of databases.
    fn old_install(home: &Path) -> Config {
        crate::db::v1::file(&home.join("otpuska.db"), true);
        let mut c: Config = serde_json::from_str(r#"{"ui":{"theme":"light","scale":1.1},"last_dir":"/tmp"}"#).unwrap();
        c.home = Some(home.to_path_buf());
        c
    }

    #[test]
    fn old_database_is_registered_where_it_lies() {
        let home = tempfile::tempdir().unwrap();
        let mut c = old_install(home.path());

        assert!(c.migrate());
        assert_eq!(c.databases.len(), 1);
        let e = &c.databases[0];
        assert_eq!((e.name.as_str(), e.path.as_path()), (FIRST_NAME, Path::new("otpuska.db")));
        assert_eq!(c.db_file(e), home.path().join("otpuska.db"));
        assert_eq!(c.last_db.as_deref(), Some("main"));
        assert_eq!(c.start_choice().as_deref(), Some("main"), "a single database opens without asking");
        assert_eq!(c.ui.theme.as_deref(), Some("light"), "other settings survive");

        // Registering did not touch the data file (still the old schema); a safety copy in the old format was made.
        let schema = |f: &Path| rusqlite::Connection::open(f).unwrap().pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0)).unwrap();
        assert_eq!(schema(&home.path().join("otpuska.db")), 1);
        let copies: Vec<_> = std::fs::read_dir(home.path().join("backups")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(copies.len(), 1);
        assert!(copies[0].starts_with("otpuska-before-update-"));
        assert_eq!(Db::peek(&home.path().join("backups").join(&copies[0])).unwrap().employees, 3);
        assert_eq!(schema(&home.path().join("backups").join(&copies[0])), 1, "the copy can still be used by the old version");
        // Opening it for work upgrades the file (after one more copy) and loses nothing.
        let db = c.open_database("main", day(10, 22)).unwrap();
        assert_eq!((db.employees().unwrap().len(), db.leaves().unwrap().len()), (3, 3));
        assert_eq!(db.upgrade().map(|u| u.from), Some(1));
        assert_eq!(schema(&home.path().join("otpuska.db")), 2);
        drop(db);

        // Running it again changes nothing; the saved config reads back the same.
        assert!(!c.migrate());
        let json = serde_json::to_string(&c).unwrap();
        let mut back: Config = serde_json::from_str(&json).unwrap();
        back.home = c.home.clone();
        assert_eq!(back, c);
        // The old version's config has no list: it is ignored, not an error.
        assert!(!json.contains("home"));
    }

    #[test]
    fn old_custom_db_path_is_kept() {
        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let file = elsewhere.path().join("moya.db");
        Db::open(&file).unwrap();
        let mut c = config_in(home.path());
        c.db_path = Some(file.clone());
        assert!(c.migrate());
        assert_eq!(c.db_file(&c.databases[0]), file, "absolute paths stay absolute");
    }

    #[test]
    fn databases_are_found_again_when_the_configuration_is_lost() {
        let home = tempfile::tempdir().unwrap();
        let mut c = old_install(home.path());
        c.migrate();
        let a = c.create_database("Школа").unwrap();
        let b = c.create_database("Сад").unwrap();
        drop(c.open_database(&b, day(10, 22)).unwrap());
        // config.json is gone (or unreadable): a configuration with no list at all.
        let mut lost = config_in(home.path());
        assert!(lost.migrate());
        let ids: Vec<&str> = lost.databases.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids.len(), 3, "the old main database and both new ones: {ids:?}");
        assert!(ids.contains(&"main") && ids.contains(&a.as_str()) && ids.contains(&b.as_str()));
        // Each one opens with its own data.
        assert_eq!(lost.open_database("main", day(10, 22)).unwrap().employees().unwrap().len(), 3);
        assert!(lost.open_database(&a, day(10, 22)).unwrap().is_empty().unwrap());
        assert!(lost.databases.iter().filter(|d| d.id != "main").all(|d| d.name.starts_with("Восстановленная база")));
        // Nothing is added twice.
        assert!(!lost.migrate());
    }

    #[test]
    fn fresh_install_asks_for_the_first_database() {
        let home = tempfile::tempdir().unwrap();
        let mut c = config_in(home.path());
        assert!(!c.migrate());
        assert!(c.databases.is_empty() && c.start_choice().is_none());
        let id = c.create_database("  Школа   №5 ").unwrap();
        assert_eq!(c.find_db(&id).unwrap().name, "Школа №5", "spaces are tidied");
        assert!(home.path().join("databases").join(&id).join("otpuska.db").is_file());
        assert_eq!(c.start_choice().as_deref(), Some(id.as_str()));
    }

    #[test]
    fn names_are_checked() {
        let home = tempfile::tempdir().unwrap();
        let mut c = config_in(home.path());
        let a = c.create_database("Школа").unwrap();
        assert!(c.create_database("   ").is_err());
        assert!(c.create_database("школа").is_err(), "letter case does not make a new name");
        assert!(c.create_database(&"я".repeat(61)).is_err());
        let b = c.create_database("Детский сад").unwrap();
        assert_ne!(a, b);
        assert!(c.rename_database(&b, "ШКОЛА").is_err());
        c.rename_database(&a, "школа").unwrap(); // its own name in another case is fine
        c.rename_database(&b, "Сад №2").unwrap();
        assert_eq!(c.find_db(&b).unwrap().name, "Сад №2");
        assert!(c.rename_database("nope", "x").is_err());
    }

    #[test]
    fn databases_are_separate_files() {
        let home = tempfile::tempdir().unwrap();
        let mut c = config_in(home.path());
        let a = c.create_database("А").unwrap();
        let b = c.create_database("Б").unwrap();
        let da = c.open_database(&a, day(10, 22)).unwrap();
        da.insert_employee(&Employee { fio: "Только в А".into(), ..Default::default() }).unwrap();
        drop(da);
        assert_eq!(c.open_database(&a, day(10, 22)).unwrap().employees().unwrap().len(), 1);
        assert_eq!(c.last_db.as_deref(), Some(a.as_str()));
        assert!(c.open_database(&b, day(10, 22)).unwrap().is_empty().unwrap());
        assert_eq!(c.last_db.as_deref(), Some(b.as_str()));
        // Each one gets its daily backup in its own folder.
        let backups = home.path().join("databases").join(&a).join("backups");
        assert!(backups.join("otpuska-2026-10-22.db").is_file());
        assert!(!home.path().join("backups").exists());
    }

    #[test]
    fn missing_file_is_reported_not_recreated() {
        let home = tempfile::tempdir().unwrap();
        let mut c = config_in(home.path());
        let a = c.create_database("А").unwrap();
        let file = c.db_file(c.find_db(&a).unwrap());
        std::fs::remove_file(&file).unwrap();
        let err = c.open_database(&a, day(10, 22)).err().unwrap().to_string();
        assert!(err.contains("не найден") && err.contains("«А»"), "{err}");
        assert!(!file.exists(), "no empty database appeared in its place");
        assert!(c.open_database("nope", day(10, 22)).is_err());
        // Such an entry can be dropped from the list, even the only one.
        c.forget_database(&a).unwrap();
        assert!(c.databases.is_empty() && c.last_db.is_none());
    }

    #[test]
    fn start_choice_follows_the_rules() {
        let home = tempfile::tempdir().unwrap();
        let mut c = config_in(home.path());
        let a = c.create_database("А").unwrap();
        let b = c.create_database("Б").unwrap();
        assert_eq!(c.start_choice(), None, "two databases: ask");
        c.ask_on_start = Some(false);
        assert_eq!(c.start_choice(), None, "nothing was opened yet, so there is no 'last'");
        c.last_db = Some(b.clone());
        assert_eq!(c.start_choice().as_deref(), Some(b.as_str()));
        c.last_db = Some("gone".into());
        assert_eq!(c.start_choice(), None);
        c.ask_on_start = None;
        c.last_db = Some(a);
        assert_eq!(c.start_choice(), None, "asking is the default");
    }

    #[test]
    fn removing_moves_files_aside_and_keeps_the_last_one() {
        let home = tempfile::tempdir().unwrap();
        let mut c = old_install(home.path());
        c.migrate();
        let b = c.create_database("Б").unwrap();
        let db = c.open_database(&b, day(10, 22)).unwrap();
        db.insert_employee(&Employee { fio: "Б1".into(), ..Default::default() }).unwrap();
        drop(db);
        drop(c.open_database(&b, day(10, 22)).unwrap()); // now there is something to back up

        let kept = c.remove_database(&b).unwrap().unwrap();
        assert_eq!(c.databases.len(), 1);
        assert_eq!(c.last_db.as_deref(), Some("main"), "last_db moves to a database that exists");
        assert!(!home.path().join("databases").join(&b).exists(), "its folder is gone");
        assert_eq!(Db::peek(&kept.join("otpuska.db")).unwrap().employees, 1, "the data is kept aside");
        assert!(kept.join("backups").is_dir());

        assert!(c.remove_database("main").is_err(), "the last database stays");
        assert_eq!(Db::open(&home.path().join("otpuska.db")).unwrap().employees().unwrap().len(), 3);

        // The old main database can be removed too once there is another one.
        let c2 = c.create_database("В").unwrap();
        let kept = c.remove_database("main").unwrap().unwrap();
        assert!(!home.path().join("otpuska.db").exists());
        assert_eq!(Db::peek(&kept.join("otpuska.db")).unwrap().employees, 3);
        assert_eq!(c.last_db.as_deref(), Some(c2.as_str()));
    }

    #[test]
    fn deleted_folders_are_purged_after_a_month() {
        let home = tempfile::tempdir().unwrap();
        let c = config_in(home.path());
        let dir = home.path().join("deleted");
        for name in ["a-2026-09-01-101010", "b-2026-10-01-101010", "c-2026-10-20-101010", "odd name"] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        c.purge_deleted(day(10, 22));
        let mut left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left, ["b-2026-10-01-101010", "c-2026-10-20-101010", "odd name"]);
    }

    #[test]
    fn a_backup_file_can_be_added_as_a_new_database() {
        let home = tempfile::tempdir().unwrap();
        let mut c = old_install(home.path());
        c.migrate();
        let copy = home.path().join("kopiya.db");
        Db::open(&home.path().join("otpuska.db")).unwrap().backup_to(&copy).unwrap();

        let id = c.add_database_file("Копия", &copy).unwrap();
        assert_eq!(c.open_database(&id, day(10, 22)).unwrap().employees().unwrap().len(), 3);
        assert!(copy.exists(), "the source file is left alone");
        assert!(c.add_database_file("Копия", &copy).is_err(), "name taken");
        assert!(c.add_database_file("Ещё", &home.path().join("otpuska.db")).is_err(), "the live file is already in the list");
        std::fs::write(home.path().join("junk.db"), b"not a database").unwrap();
        assert!(c.add_database_file("Мусор", &home.path().join("junk.db")).is_err());
        assert_eq!(c.databases.len(), 2);
        assert!(std::fs::read_dir(home.path().join("databases")).unwrap().count() == 1, "no leftovers from the failed attempt");
    }
}
