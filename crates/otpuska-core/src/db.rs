//! SQLite storage. One file, WAL mode; the whole data set is small (hundreds of employees,
//! a couple of thousand journal rows), so reads load everything and writes are single statements.

use crate::date::{self, Date};
use crate::model::*;
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

/// 1 — versions 1.0 and 1.1. 2 — adds the kind and the end date of a leave, the vacation
/// schedule and the reminder settings (stored as ordinary settings keys, no schema change).
const SCHEMA_VERSION: i32 = 2;

const SCHEMA: &str = "
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE departments (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    sort INTEGER NOT NULL
);
CREATE TABLE employees (
    id            INTEGER PRIMARY KEY,
    fio           TEXT NOT NULL,
    position      TEXT NOT NULL DEFAULT '',
    department_id INTEGER REFERENCES departments(id) ON DELETE SET NULL,
    hire_date     TEXT,
    status        TEXT NOT NULL DEFAULT 'work' CHECK (status IN ('work', 'dismissed', 'suspended')),
    extra_days    REAL,
    suspend_from  TEXT,
    suspend_to    TEXT,
    dismiss_date  TEXT,
    base_days     REAL,
    accrual_from  TEXT,
    note          TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);
CREATE TABLE leaves (
    id          INTEGER PRIMARY KEY,
    employee_id INTEGER NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    start_date  TEXT,
    base_days   REAL NOT NULL DEFAULT 0,
    extra_days  REAL NOT NULL DEFAULT 0,
    note        TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    kind        TEXT NOT NULL DEFAULT 'paid' CHECK (kind IN ('paid', 'unpaid', 'maternity', 'childcare')),
    end_date    TEXT
);
CREATE INDEX leaves_employee ON leaves(employee_id);
CREATE INDEX employees_department ON employees(department_id);
";

/// What the second schema adds to a database of the first one. The columns sit at the end of
/// `leaves` in both cases, so a new and an upgraded file have the same layout.
const UPGRADE_1_TO_2: &str = "
ALTER TABLE leaves ADD COLUMN kind TEXT NOT NULL DEFAULT 'paid' CHECK (kind IN ('paid', 'unpaid', 'maternity', 'childcare'));
ALTER TABLE leaves ADD COLUMN end_date TEXT;
";

const SCHEDULE_TABLE: &str = "
CREATE TABLE schedule (
    id          INTEGER PRIMARY KEY,
    employee_id INTEGER NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    start_date  TEXT NOT NULL,
    base_days   REAL NOT NULL DEFAULT 0,
    extra_days  REAL NOT NULL DEFAULT 0,
    note        TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);
CREATE INDEX schedule_employee ON schedule(employee_id);
";

pub struct Db {
    conn: Connection,
    path: PathBuf,
    upgrade: Option<Upgrade>,
}

/// A file of an older version was brought up to the current schema while it was being opened.
#[derive(Clone, Debug, PartialEq)]
pub struct Upgrade {
    /// Schema version the file had.
    pub from: i32,
    /// The copy made right before the upgrade (none for an empty database).
    pub backup: Option<PathBuf>,
}

/// `otpuska.db` + `-wal` → `otpuska.db-wal`: the files SQLite keeps next to an open database.
pub(crate) fn with_suffix(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn opt_date(d: Option<Date>) -> Option<String> {
    d.map(date::iso)
}

fn read_date(s: Option<String>) -> Option<Date> {
    s.as_deref().and_then(date::parse_iso)
}

impl Db {
    /// Opens a database for work; a file of an older version is upgraded first (see `Db::upgrade`).
    pub fn open(path: &Path) -> Result<Db> {
        Self::open_with(path, true)
    }

    /// Opens a file as it is, without upgrading it. For copying a file of an older version
    /// (the copy must stay readable by that version).
    pub fn open_as_is(path: &Path) -> Result<Db> {
        Self::open_with(path, false)
    }

    fn open_with(path: &Path, upgrade: bool) -> Result<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        Self::init(conn, path.to_path_buf(), upgrade)
    }

    pub fn open_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory()?, PathBuf::from(":memory:"), true)
    }

    fn init(conn: Connection, path: PathBuf, upgrade: bool) -> Result<Db> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let mut db = Db { conn, path, upgrade: None };
        if upgrade {
            db.migrate()?;
        }
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Set when opening this file upgraded it from an older schema.
    pub fn upgrade(&self) -> Option<&Upgrade> {
        self.upgrade.as_ref()
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i32 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match version {
            0 => {
                let tx = self.conn.unchecked_transaction()?;
                tx.execute_batch(SCHEMA)?;
                tx.execute_batch(SCHEDULE_TABLE)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
            1 => {
                // The user's data is about to be rewritten in place: keep a copy first, and do not
                // touch the file at all if the copy cannot be made.
                let backup = self.copy_before_upgrade(version)?;
                let tx = self.conn.unchecked_transaction()?;
                tx.execute_batch(UPGRADE_1_TO_2)?;
                tx.execute_batch(SCHEDULE_TABLE)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
                self.upgrade = Some(Upgrade { from: version, backup });
            }
            v if v == SCHEMA_VERSION => {}
            v => bail!("База создана более новой версией программы (схема {v}, поддерживается {SCHEMA_VERSION})"),
        }
        Ok(())
    }

    /// `backups/otpuska-before-v2-<date-time>.db` next to the database; nothing for an empty one.
    fn copy_before_upgrade(&self, from: i32) -> Result<Option<PathBuf>> {
        if self.path == Path::new(":memory:") || self.is_empty()? {
            return Ok(None);
        }
        let dir = crate::config::backups_dir(&self.path);
        std::fs::create_dir_all(&dir).with_context(|| format!("Не удалось создать папку {} для копии перед обновлением базы", dir.display()))?;
        let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
        // Never replace an earlier copy: add a number if the name is taken.
        let mut target = dir.join(format!("otpuska-before-v{}-{stamp}.db", from + 1));
        for n in 2..100 {
            if !target.exists() {
                break;
            }
            target = dir.join(format!("otpuska-before-v{}-{stamp}-{n}.db", from + 1));
        }
        self.backup_to(&target).with_context(|| "Не удалось сохранить копию базы перед обновлением — базу не меняли")?;
        // The copy has to be a readable database, otherwise it is no protection.
        Db::verify_file(&target).with_context(|| "Копия базы перед обновлением не читается — базу не меняли")?;
        Ok(Some(target))
    }

    // ---- reading ---------------------------------------------------------------------

    pub fn load(&self) -> Result<Data> {
        Ok(Data {
            settings: self.settings()?,
            departments: self.departments()?,
            employees: self.employees()?,
            leaves: self.leaves()?,
            plans: self.plans()?,
        })
    }

    pub fn is_empty(&self) -> Result<bool> {
        let n: i64 = self.conn.query_row("SELECT (SELECT COUNT(*) FROM employees) + (SELECT COUNT(*) FROM leaves)", [], |r| r.get(0))?;
        Ok(n == 0)
    }

    pub fn settings(&self) -> Result<Settings> {
        let mut s = Settings::default();
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (key, value) = row?;
            let num = value.trim().replace(',', ".").parse::<f64>().ok();
            match key.as_str() {
                "as_of" => s.as_of_manual = date::parse_iso(&value),
                "base_days" => s.base_days = num.unwrap_or(s.base_days),
                "first_leave_months" => s.first_leave_months = num.map_or(s.first_leave_months, |v| v as i32),
                "deadline_months" => s.deadline_months = num.map_or(s.deadline_months, |v| v as i32),
                "burning_days" => s.burning_days = num.map_or(s.burning_days, |v| v as i32),
                "soon_days" => s.soon_days = num.map_or(s.soon_days, |v| v as i32),
                "suspension_counts" => s.suspension_counts = value == "1",
                "remind_near_days" => s.remind_near_days = num.map_or(s.remind_near_days, |v| v as i32),
                "remind_far_months" => s.remind_far_months = num.map_or(s.remind_far_months, |v| v as i32),
                "remind_end_days" => s.remind_end_days = num.map_or(s.remind_end_days, |v| v as i32),
                _ => {}
            }
        }
        Ok(s)
    }

    pub fn departments(&self) -> Result<Vec<Department>> {
        let mut stmt = self.conn.prepare("SELECT id, name FROM departments ORDER BY sort, id")?;
        let rows = stmt.query_map([], |r| Ok(Department { id: r.get(0)?, name: r.get(1)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn employees(&self) -> Result<Vec<Employee>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, fio, position, department_id, hire_date, status, extra_days, suspend_from, suspend_to,
                    dismiss_date, base_days, accrual_from, note
             FROM employees ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Employee {
                id: r.get(0)?,
                fio: r.get(1)?,
                position: r.get(2)?,
                department_id: r.get(3)?,
                hire_date: read_date(r.get(4)?),
                status: Status::from_code(&r.get::<_, String>(5)?),
                extra_days: r.get(6)?,
                suspend_from: read_date(r.get(7)?),
                suspend_to: read_date(r.get(8)?),
                dismiss_date: read_date(r.get(9)?),
                base_days: r.get(10)?,
                accrual_from: read_date(r.get(11)?),
                note: r.get(12)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn leaves(&self) -> Result<Vec<Leave>> {
        let mut stmt = self.conn.prepare("SELECT id, employee_id, start_date, base_days, extra_days, note, kind, end_date FROM leaves ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Leave {
                id: r.get(0)?,
                employee_id: r.get(1)?,
                start: read_date(r.get(2)?),
                base_days: r.get(3)?,
                extra_days: r.get(4)?,
                note: r.get(5)?,
                kind: LeaveType::from_code(&r.get::<_, String>(6)?),
                end: read_date(r.get(7)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn plans(&self) -> Result<Vec<Plan>> {
        let mut stmt = self.conn.prepare("SELECT id, employee_id, start_date, base_days, extra_days, note FROM schedule ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Plan {
                id: r.get(0)?,
                employee_id: r.get(1)?,
                start: read_date(r.get(2)?),
                base_days: r.get(3)?,
                extra_days: r.get(4)?,
                note: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- settings ----------------------------------------------------------------------

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let pairs: [(&str, String); 10] = [
            ("as_of", s.as_of_manual.map(date::iso).unwrap_or_default()),
            ("base_days", s.base_days.to_string()),
            ("first_leave_months", s.first_leave_months.to_string()),
            ("deadline_months", s.deadline_months.to_string()),
            ("burning_days", s.burning_days.to_string()),
            ("soon_days", s.soon_days.to_string()),
            ("suspension_counts", if s.suspension_counts { "1" } else { "0" }.to_string()),
            ("remind_near_days", s.remind_near_days.to_string()),
            ("remind_far_months", s.remind_far_months.to_string()),
            ("remind_end_days", s.remind_end_days.to_string()),
        ];
        for (key, value) in pairs {
            self.conn.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        Ok(())
    }

    // ---- departments -------------------------------------------------------------------

    pub fn add_department(&self, name: &str) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            bail!("Название подразделения не может быть пустым");
        }
        let sort: i64 = self.conn.query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM departments", [], |r| r.get(0))?;
        match self.conn.execute("INSERT INTO departments(name, sort) VALUES (?1, ?2)", params![name, sort]) {
            Ok(_) => Ok(self.conn.last_insert_rowid()),
            Err(e) if is_unique(&e) => bail!("Подразделение «{name}» уже есть в списке"),
            Err(e) => Err(e.into()),
        }
    }

    pub fn rename_department(&self, id: i64, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("Название подразделения не может быть пустым");
        }
        match self.conn.execute("UPDATE departments SET name = ?1 WHERE id = ?2", params![name, id]) {
            Ok(_) => Ok(()),
            Err(e) if is_unique(&e) => bail!("Подразделение «{name}» уже есть в списке"),
            Err(e) => Err(e.into()),
        }
    }

    pub fn department_usage(&self, id: i64) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM employees WHERE department_id = ?1", [id], |r| r.get(0))?)
    }

    /// Only an unused department can be deleted.
    pub fn delete_department(&self, id: i64) -> Result<()> {
        let used = self.department_usage(id)?;
        if used > 0 {
            bail!("В подразделении ещё {used} сотрудников — сначала переведите их в другое");
        }
        self.conn.execute("DELETE FROM departments WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Swap the position of a department with its neighbour (`up` = towards the start).
    pub fn move_department(&self, id: i64, up: bool) -> Result<()> {
        let list = self.departments()?;
        let Some(pos) = list.iter().position(|d| d.id == id) else { return Ok(()) };
        let other = if up { pos.checked_sub(1) } else { (pos + 1 < list.len()).then_some(pos + 1) };
        let Some(other) = other else { return Ok(()) };
        let mut order: Vec<i64> = list.iter().map(|d| d.id).collect();
        order.swap(pos, other);
        for (i, did) in order.iter().enumerate() {
            self.conn.execute("UPDATE departments SET sort = ?1 WHERE id = ?2", params![i as i64 + 1, did])?;
        }
        Ok(())
    }

    // ---- employees ---------------------------------------------------------------------

    pub fn insert_employee(&self, e: &Employee) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO employees(fio, position, department_id, hire_date, status, extra_days, suspend_from,
                                   suspend_to, dismiss_date, base_days, accrual_from, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                e.fio.trim(),
                e.position.trim(),
                e.department_id,
                opt_date(e.hire_date),
                e.status.code(),
                e.extra_days,
                opt_date(e.suspend_from),
                opt_date(e.suspend_to),
                opt_date(e.dismiss_date),
                e.base_days,
                opt_date(e.accrual_from),
                e.note.trim(),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_employee(&self, e: &Employee) -> Result<()> {
        self.conn.execute(
            "UPDATE employees SET fio = ?1, position = ?2, department_id = ?3, hire_date = ?4, status = ?5,
                    extra_days = ?6, suspend_from = ?7, suspend_to = ?8, dismiss_date = ?9, base_days = ?10,
                    accrual_from = ?11, note = ?12
             WHERE id = ?13",
            params![
                e.fio.trim(),
                e.position.trim(),
                e.department_id,
                opt_date(e.hire_date),
                e.status.code(),
                e.extra_days,
                opt_date(e.suspend_from),
                opt_date(e.suspend_to),
                opt_date(e.dismiss_date),
                e.base_days,
                opt_date(e.accrual_from),
                e.note.trim(),
                e.id,
            ],
        )?;
        Ok(())
    }

    /// Deletes the employee together with their leaves.
    pub fn delete_employee(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM employees WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- leaves ------------------------------------------------------------------------

    /// Days that count against the annual balance: only the annual paid leave has them.
    fn balance_days(l: &Leave) -> (f64, f64) {
        if l.kind.is_paid() {
            (l.base_days, l.extra_days)
        } else {
            (0.0, 0.0)
        }
    }

    /// The end date is kept only for the kinds of leave that are given by two dates.
    fn stored_end(l: &Leave) -> Option<String> {
        (!l.kind.is_paid()).then(|| opt_date(l.end)).flatten()
    }

    pub fn insert_leave(&self, l: &Leave) -> Result<i64> {
        l.check().map_err(|e| anyhow::anyhow!(e))?;
        let (base, extra) = Self::balance_days(l);
        self.conn.execute(
            "INSERT INTO leaves(employee_id, start_date, base_days, extra_days, note, kind, end_date) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![l.employee_id, opt_date(l.start), base, extra, l.note.trim(), l.kind.code(), Self::stored_end(l)],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_leave(&self, l: &Leave) -> Result<()> {
        l.check().map_err(|e| anyhow::anyhow!(e))?;
        let (base, extra) = Self::balance_days(l);
        self.conn.execute(
            "UPDATE leaves SET employee_id = ?1, start_date = ?2, base_days = ?3, extra_days = ?4, note = ?5, kind = ?6, end_date = ?7 WHERE id = ?8",
            params![l.employee_id, opt_date(l.start), base, extra, l.note.trim(), l.kind.code(), Self::stored_end(l), l.id],
        )?;
        Ok(())
    }

    pub fn delete_leave(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM leaves WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- vacation schedule -------------------------------------------------------------

    fn check_plan(p: &Plan) -> Result<String> {
        let Some(start) = p.start else { bail!("Укажите дату начала отпуска по графику") };
        if p.total_days() <= 0.0 {
            bail!("Укажите число дней отпуска по графику");
        }
        Ok(date::iso(start))
    }

    pub fn insert_plan(&self, p: &Plan) -> Result<i64> {
        let start = Self::check_plan(p)?;
        self.conn.execute(
            "INSERT INTO schedule(employee_id, start_date, base_days, extra_days, note) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![p.employee_id, start, p.base_days, p.extra_days, p.note.trim()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_plan(&self, p: &Plan) -> Result<()> {
        let start = Self::check_plan(p)?;
        self.conn.execute(
            "UPDATE schedule SET employee_id = ?1, start_date = ?2, base_days = ?3, extra_days = ?4, note = ?5 WHERE id = ?6",
            params![p.employee_id, start, p.base_days, p.extra_days, p.note.trim(), p.id],
        )?;
        Ok(())
    }

    pub fn delete_plan(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM schedule WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- bulk --------------------------------------------------------------------------

    /// Replaces all data with `import` in one transaction.
    pub fn replace_all(&mut self, import: &Import) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("DELETE FROM schedule; DELETE FROM leaves; DELETE FROM employees; DELETE FROM departments;")?;
        let mut dept_ids = std::collections::HashMap::new();
        for (i, name) in import.departments.iter().enumerate() {
            tx.execute("INSERT INTO departments(name, sort) VALUES (?1, ?2)", params![name, i as i64 + 1])?;
            dept_ids.insert(name.clone(), tx.last_insert_rowid());
        }
        let mut emp_ids = Vec::with_capacity(import.employees.len());
        for ie in &import.employees {
            let e = &ie.employee;
            let dept = ie.department.as_ref().and_then(|n| dept_ids.get(n)).copied();
            tx.execute(
                "INSERT INTO employees(fio, position, department_id, hire_date, status, extra_days, suspend_from,
                                       suspend_to, dismiss_date, base_days, accrual_from, note)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    e.fio,
                    e.position,
                    dept,
                    opt_date(e.hire_date),
                    e.status.code(),
                    e.extra_days,
                    opt_date(e.suspend_from),
                    opt_date(e.suspend_to),
                    opt_date(e.dismiss_date),
                    e.base_days,
                    opt_date(e.accrual_from),
                    e.note,
                ],
            )?;
            emp_ids.push(tx.last_insert_rowid());
        }
        for il in &import.leaves {
            let l = &il.leave;
            tx.execute(
                "INSERT INTO leaves(employee_id, start_date, base_days, extra_days, note, kind, end_date) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![emp_ids[il.employee], opt_date(l.start), l.base_days, l.extra_days, l.note, l.kind.code(), Self::stored_end(l)],
            )?;
        }
        if let Some(settings) = &import.settings {
            for (key, value) in [
                ("base_days", settings.base_days.to_string()),
                ("first_leave_months", settings.first_leave_months.to_string()),
                ("deadline_months", settings.deadline_months.to_string()),
                ("burning_days", settings.burning_days.to_string()),
                ("soon_days", settings.soon_days.to_string()),
                ("suspension_counts", if settings.suspension_counts { "1" } else { "0" }.to_string()),
            ] {
                tx.execute(
                    "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, value],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ---- files -------------------------------------------------------------------------

    /// A consistent copy of the database (works while the app keeps it open).
    pub fn backup_to(&self, target: &Path) -> Result<()> {
        if target.exists() {
            std::fs::remove_file(target).with_context(|| format!("cannot replace {}", target.display()))?;
        }
        self.conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])?;
        Ok(())
    }

    /// Checks that `file` is a usable database of this program and returns the number of employees.
    pub fn verify_file(file: &Path) -> Result<i64> {
        let conn = Connection::open_with_flags(file, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("cannot open {}", file.display()))?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 0 || version > SCHEMA_VERSION {
            bail!("Это не файл базы «Отпуска» (или он создан более новой версией)");
        }
        let ok: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if ok != "ok" {
            bail!("Файл базы повреждён: {ok}");
        }
        conn.query_row("SELECT COUNT(*) FROM employees", [], |r| r.get(0)).optional()?.context("empty result")
    }

    /// What is in a database file, for the chooser: read-only, without opening it for work.
    pub fn peek(file: &Path) -> Result<Peek> {
        let conn = Connection::open_with_flags(file, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("cannot open {}", file.display()))?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 0 || version > SCHEMA_VERSION {
            bail!("Это не файл базы «Отпуска» (или он создан более новой версией)");
        }
        let count = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
        // The main file is only rewritten on a checkpoint, so the write-ahead log counts too.
        let touched = |p: PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        let modified = [touched(file.to_path_buf()), touched(with_suffix(file, "-wal"))].into_iter().flatten().max();
        Ok(Peek { employees: count("SELECT COUNT(*) FROM employees")?, leaves: count("SELECT COUNT(*) FROM leaves")?, modified })
    }

    /// Rows per table; for tests and diagnostics.
    pub fn counts(&self) -> Result<(i64, i64, i64)> {
        let q = |t: &str| -> Result<i64> { Ok(self.conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))?) };
        Ok((q("departments")?, q("employees")?, q("leaves")?))
    }

    /// Number of entries in the vacation schedule.
    pub fn plan_count(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM schedule", [], |r| r.get(0))?)
    }
}

/// A quick look into a database file (see `Db::peek`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peek {
    pub employees: i64,
    pub leaves: i64,
    pub modified: Option<std::time::SystemTime>,
}

fn is_unique(e: &rusqlite::Error) -> bool {
    matches!(e, rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation)
}

// ---- import payload ---------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct ImportEmployee {
    pub employee: Employee,
    pub department: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ImportLeave {
    /// Index into `Import::employees`.
    pub employee: usize,
    pub leave: Leave,
}

#[derive(Clone, Debug, Default)]
pub struct Import {
    pub settings: Option<Settings>,
    pub departments: Vec<String>,
    pub employees: Vec<ImportEmployee>,
    pub leaves: Vec<ImportLeave>,
    pub warnings: Vec<String>,
}

/// Files as versions 1.0 and 1.1 wrote them, for the tests of every module that has to cope with them.
#[cfg(any(test, feature = "test-support"))]
pub mod v1 {
    use rusqlite::Connection;
    use std::path::Path;

    /// The schema of versions 1.0 and 1.1, as they created it.
    pub const SCHEMA: &str = "
        CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE departments (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, sort INTEGER NOT NULL);
        CREATE TABLE employees (
            id INTEGER PRIMARY KEY, fio TEXT NOT NULL, position TEXT NOT NULL DEFAULT '',
            department_id INTEGER REFERENCES departments(id) ON DELETE SET NULL, hire_date TEXT,
            status TEXT NOT NULL DEFAULT 'work' CHECK (status IN ('work', 'dismissed', 'suspended')),
            extra_days REAL, suspend_from TEXT, suspend_to TEXT, dismiss_date TEXT, base_days REAL, accrual_from TEXT,
            note TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')));
        CREATE TABLE leaves (
            id INTEGER PRIMARY KEY, employee_id INTEGER NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
            start_date TEXT, base_days REAL NOT NULL DEFAULT 0, extra_days REAL NOT NULL DEFAULT 0,
            note TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')));
        CREATE INDEX leaves_employee ON leaves(employee_id);
        CREATE INDEX employees_department ON employees(department_id);
    ";

    /// A file as version 1.1 leaves it, with a little of everything in it.
    pub fn file(path: &Path, rows: bool) {
        let conn = Connection::open(path).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        if rows {
            conn.execute_batch(
                "INSERT INTO departments(id, name, sort) VALUES (1, 'Цех', 1), (2, 'Офис', 2);
                 INSERT INTO settings VALUES ('base_days', '31'), ('burning_days', '45'), ('as_of', '');
                 INSERT INTO employees(id, fio, position, department_id, hire_date, status, extra_days, base_days, accrual_from, suspend_from, suspend_to, note)
                    VALUES (1, 'Иванов Иван', 'Мастер', 1, '2022-09-28', 'work', 7, NULL, NULL, NULL, NULL, 'заметка'),
                           (2, 'Петров Пётр', '', 2, '2024-02-29', 'suspended', NULL, 30, '2024-03-01', '2025-03-01', NULL, ''),
                           (3, 'Без даты', '', NULL, NULL, 'dismissed', NULL, NULL, NULL, NULL, NULL, '');
                 INSERT INTO leaves(id, employee_id, start_date, base_days, extra_days, note)
                    VALUES (1, 1, '2025-06-02', 14, 0, 'приказ 5'), (2, 1, '2025-12-01', 14, 7, ''), (3, 2, NULL, 10.5, 0, 'без даты');",
            )
            .unwrap();
        }
        conn.pragma_update(None, "user_version", 1).unwrap();
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn crud_roundtrip() {
        let db = Db::open_memory().unwrap();
        assert!(db.is_empty().unwrap());
        let dept = db.add_department("Цех №1").unwrap();
        assert!(db.add_department("Цех №1").is_err());
        let e = Employee {
            fio: " Иванов Иван ".into(),
            position: "Мастер".into(),
            department_id: Some(dept),
            hire_date: Some(d(2022, 9, 28)),
            extra_days: Some(7.0),
            suspend_from: Some(d(2025, 3, 1)),
            ..Default::default()
        };
        let id = db.insert_employee(&e).unwrap();
        let loaded = db.employees().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].fio, "Иванов Иван");
        assert_eq!(loaded[0].hire_date, Some(d(2022, 9, 28)));
        assert_eq!(loaded[0].extra_days, Some(7.0));
        assert_eq!(loaded[0].base_days, None);

        let mut upd = loaded[0].clone();
        upd.status = Status::Dismissed;
        upd.dismiss_date = Some(d(2026, 1, 5));
        db.update_employee(&upd).unwrap();
        assert_eq!(db.employees().unwrap()[0].status, Status::Dismissed);

        let lid = db.insert_leave(&Leave { employee_id: id, start: Some(d(2023, 12, 4)), base_days: 14.0, ..Default::default() }).unwrap();
        assert_eq!(db.leaves().unwrap().len(), 1);
        db.update_leave(&Leave { id: lid, employee_id: id, start: None, base_days: 10.5, extra_days: 3.0, note: "приказ 5".into(), ..Default::default() }).unwrap();
        let l = &db.leaves().unwrap()[0];
        assert_eq!((l.start, l.base_days, l.extra_days, l.note.as_str()), (None, 10.5, 3.0, "приказ 5"));

        // A used department cannot be deleted; deleting the employee removes their leaves.
        assert!(db.delete_department(dept).is_err());
        db.delete_employee(id).unwrap();
        assert!(db.leaves().unwrap().is_empty());
        db.delete_department(dept).unwrap();
        assert!(db.departments().unwrap().is_empty());
    }

    #[test]
    fn settings_roundtrip() {
        let db = Db::open_memory().unwrap();
        assert_eq!(db.settings().unwrap(), Settings::default());
        let s = Settings {
            as_of_manual: Some(d(2026, 12, 1)),
            base_days: 31.0,
            first_leave_months: 3,
            deadline_months: 18,
            burning_days: 60,
            soon_days: 120,
            suspension_counts: true,
            remind_near_days: 10,
            remind_far_months: 2,
            remind_end_days: 21,
        };
        db.save_settings(&s).unwrap();
        assert_eq!(db.settings().unwrap(), s);
        db.save_settings(&Settings { as_of_manual: None, ..s.clone() }).unwrap();
        assert_eq!(db.settings().unwrap().as_of_manual, None);
    }

    #[test]
    fn departments_reorder() {
        let db = Db::open_memory().unwrap();
        let a = db.add_department("А").unwrap();
        let b = db.add_department("Б").unwrap();
        let c = db.add_department("В").unwrap();
        db.move_department(c, true).unwrap();
        let names: Vec<_> = db.departments().unwrap().into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["А", "В", "Б"]);
        db.move_department(a, true).unwrap(); // already first: no-op
        db.move_department(b, false).unwrap(); // already last: no-op
        db.rename_department(b, "Бэ").unwrap();
        assert!(db.rename_department(b, "А").is_err());
        let names: Vec<_> = db.departments().unwrap().into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["А", "В", "Бэ"]);
    }

    #[test]
    fn replace_all_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open(&dir.path().join("a.db")).unwrap();
        db.add_department("Старое").unwrap();
        let import = Import {
            settings: Some(Settings { base_days: 30.0, ..Default::default() }),
            departments: vec!["Цех".into(), "Офис".into()],
            employees: vec![
                ImportEmployee { employee: Employee { fio: "А".into(), ..Default::default() }, department: Some("Офис".into()) },
                ImportEmployee { employee: Employee { fio: "Б".into(), ..Default::default() }, department: None },
            ],
            leaves: vec![ImportLeave { employee: 1, leave: Leave { base_days: 7.0, ..Default::default() } }],
            warnings: vec![],
        };
        db.replace_all(&import).unwrap();
        assert_eq!(db.counts().unwrap(), (2, 2, 1));
        let data = db.load().unwrap();
        assert_eq!(data.settings.base_days, 30.0);
        assert_eq!(data.employees[0].department_id, Some(data.departments[1].id));
        assert_eq!(data.leaves[0].employee_id, data.employees[1].id);

        let copy = dir.path().join("copy.db");
        db.backup_to(&copy).unwrap();
        assert_eq!(Db::verify_file(&copy).unwrap(), 2);
        db.backup_to(&copy).unwrap(); // overwriting an existing copy works
        std::fs::write(dir.path().join("junk.db"), b"not a database").unwrap();
        assert!(Db::verify_file(&dir.path().join("junk.db")).is_err());
    }

    // ---- upgrade from the first schema (versions 1.0 / 1.1) ---------------------------------------

    fn version_of(path: &Path) -> i32 {
        Connection::open(path).unwrap().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap()
    }

    type RawLeave = (i64, i64, Option<String>, f64, f64, String);

    /// The rows exactly as the old program reads them (its own queries, no new columns).
    fn old_view(path: &Path) -> (Vec<RawLeave>, Vec<(i64, String, Option<String>, String)>, Vec<(String, String)>) {
        let conn = Connection::open(path).unwrap();
        let leaves = conn
            .prepare("SELECT id, employee_id, start_date, base_days, extra_days, note FROM leaves ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let emps = conn
            .prepare("SELECT id, fio, hire_date, status FROM employees ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let settings = conn
            .prepare("SELECT key, value FROM settings ORDER BY key")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        (leaves, emps, settings)
    }

    #[test]
    fn upgrade_keeps_every_row_and_leaves_a_copy_in_the_old_format() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("otpuska.db");
        v1::file(&file, true);
        let before = old_view(&file);
        assert_eq!((before.0.len(), before.1.len(), before.2.len()), (3, 3, 3));
        assert_eq!(Db::open_as_is(&file).unwrap().upgrade(), None);
        assert_eq!(version_of(&file), 1, "opening as is changes nothing");

        let db = Db::open(&file).unwrap();
        let up = db.upgrade().expect("the file was upgraded").clone();
        assert_eq!(up.from, 1);
        let copy = up.backup.clone().expect("a copy was made first");
        assert!(copy.starts_with(dir.path().join("backups")) && copy.file_name().unwrap().to_string_lossy().starts_with("otpuska-before-v2-"));
        assert_eq!(version_of(&file), 2);

        // Every old row is still there, value for value, and reads back as annual leave.
        assert_eq!(old_view(&file), before);
        let data = db.load().unwrap();
        assert!(data.leaves.iter().all(|l| l.kind == LeaveType::Paid && l.end.is_none()));
        assert_eq!(data.leaves.iter().map(|l| (l.id, l.employee_id, l.base_days, l.extra_days)).collect::<Vec<_>>(), [(1, 1, 14.0, 0.0), (2, 1, 14.0, 7.0), (3, 2, 10.5, 0.0)]);
        assert_eq!(data.leaves[0].start, Some(d(2025, 6, 2)));
        assert_eq!(data.leaves[0].note, "приказ 5");
        assert_eq!(data.leaves[2].start, None);
        assert_eq!(data.employees[0].extra_days, Some(7.0));
        assert_eq!((data.employees[1].status, data.employees[1].base_days, data.employees[1].accrual_from), (Status::Suspended, Some(30.0), Some(d(2024, 3, 1))));
        assert_eq!(data.departments.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Цех", "Офис"]);
        assert!(data.plans.is_empty());
        // Old settings are kept; the new ones start at their defaults.
        assert_eq!((data.settings.base_days, data.settings.burning_days), (31.0, 45));
        assert_eq!((data.settings.remind_near_days, data.settings.remind_far_months, data.settings.remind_end_days), (14, 1, 14));

        // The copy is the old file as it was: version 1, the same rows, readable by the old program.
        assert_eq!(version_of(&copy), 1);
        assert_eq!(old_view(&copy), before);
        assert_eq!(Db::verify_file(&copy).unwrap(), 3);
        assert_eq!(Db::peek(&copy).unwrap().leaves, 3);

        // The upgraded file works for the new things.
        db.insert_leave(&Leave { employee_id: 1, kind: LeaveType::Unpaid, start: Some(d(2026, 1, 10)), end: Some(d(2026, 1, 30)), ..Default::default() }).unwrap();
        db.insert_plan(&Plan { employee_id: 1, start: Some(d(2026, 12, 1)), base_days: 14.0, ..Default::default() }).unwrap();
        assert_eq!((db.leaves().unwrap().len(), db.plans().unwrap().len()), (4, 1));

        // Opening again is quiet: no second upgrade, no second copy.
        drop(db);
        let db = Db::open(&file).unwrap();
        assert!(db.upgrade().is_none());
        assert_eq!(std::fs::read_dir(dir.path().join("backups")).unwrap().count(), 1);
    }

    #[test]
    fn an_empty_old_file_is_upgraded_without_a_copy() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("otpuska.db");
        v1::file(&file, false);
        let db = Db::open(&file).unwrap();
        assert_eq!(db.upgrade().map(|u| (u.from, u.backup.is_none())), Some((1, true)));
        assert!(!dir.path().join("backups").exists());
        assert_eq!(version_of(&file), 2);
    }

    #[test]
    fn an_upgraded_file_has_the_same_layout_as_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.db");
        v1::file(&old, true);
        drop(Db::open(&old).unwrap());
        let fresh = Db::open(&dir.path().join("new.db")).unwrap();
        let layout = |conn: &Connection| -> Vec<String> {
            let mut out = Vec::new();
            let tables: Vec<String> = conn
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            for t in tables {
                let mut stmt = conn.prepare(&format!("PRAGMA table_info({t})")).unwrap();
                let cols = stmt.query_map([], |r| Ok(format!("{t}.{} {} notnull={} default={:?}", r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i32>(3)?, r.get::<_, Option<String>>(4)?))).unwrap();
                out.extend(cols.map(|c| c.unwrap()));
            }
            out
        };
        let upgraded = Connection::open(&old).unwrap();
        assert_eq!(layout(&upgraded), layout(&fresh.conn));
        assert!(layout(&upgraded).iter().any(|c| c.starts_with("schedule.start_date")));
    }

    #[test]
    fn nothing_is_changed_when_the_copy_cannot_be_made_and_a_newer_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("otpuska.db");
        v1::file(&file, true);
        // A file where the folder for copies should be: the copy fails, so the upgrade must not start.
        std::fs::write(dir.path().join("backups"), b"in the way").unwrap();
        let err = Db::open(&file).err().expect("refused").to_string();
        assert!(err.contains("копию") || err.contains("копии"), "{err}");
        assert_eq!(version_of(&file), 1);
        assert_eq!(Db::open_as_is(&file).unwrap().employees().unwrap().len(), 3);

        let newer = dir.path().join("newer.db");
        v1::file(&newer, true);
        Connection::open(&newer).unwrap().pragma_update(None, "user_version", 3).unwrap();
        let err = Db::open(&newer).err().expect("refused").to_string();
        assert!(err.contains("более новой версией"), "{err}");
        assert_eq!(version_of(&newer), 3);
    }

    #[test]
    fn leave_kinds_and_schedule_roundtrip() {
        let db = Db::open_memory().unwrap();
        let emp = db.insert_employee(&Employee { fio: "А".into(), ..Default::default() }).unwrap();
        let long = |kind, start: Option<Date>, end: Option<Date>| Leave { employee_id: emp, kind, start, end, base_days: 99.0, extra_days: 9.0, ..Default::default() };
        // Unpaid, maternity and childcare leave need both dates and carry no balance days.
        for kind in [LeaveType::Unpaid, LeaveType::Maternity, LeaveType::Childcare] {
            assert!(db.insert_leave(&long(kind, Some(d(2026, 3, 1)), None)).is_err());
            assert!(db.insert_leave(&long(kind, None, Some(d(2026, 3, 1)))).is_err());
            assert!(db.insert_leave(&long(kind, Some(d(2026, 3, 5)), Some(d(2026, 3, 1)))).is_err());
            db.insert_leave(&long(kind, Some(d(2026, 3, 1)), Some(d(2026, 3, 20)))).unwrap();
        }
        let got = db.leaves().unwrap();
        assert_eq!(got.iter().map(|l| l.kind).collect::<Vec<_>>(), [LeaveType::Unpaid, LeaveType::Maternity, LeaveType::Childcare]);
        assert!(got.iter().all(|l| l.base_days == 0.0 && l.extra_days == 0.0 && l.end == Some(d(2026, 3, 20)) && l.length() == 20.0));
        // Turning a record into annual leave drops the end date and takes the days.
        let mut l = got[0].clone();
        l.kind = LeaveType::Paid;
        l.base_days = 14.0;
        db.update_leave(&l).unwrap();
        let back = db.leaves().unwrap().remove(0);
        assert_eq!((back.kind, back.end, back.base_days), (LeaveType::Paid, None, 14.0));

        // The schedule: a date and days are required; deleting the employee removes the entries.
        assert!(db.insert_plan(&Plan { employee_id: emp, start: None, base_days: 14.0, ..Default::default() }).is_err());
        assert!(db.insert_plan(&Plan { employee_id: emp, start: Some(d(2026, 7, 1)), ..Default::default() }).is_err());
        let pid = db.insert_plan(&Plan { employee_id: emp, start: Some(d(2026, 7, 1)), base_days: 14.0, note: " отпуск ".into(), ..Default::default() }).unwrap();
        db.update_plan(&Plan { id: pid, employee_id: emp, start: Some(d(2026, 8, 3)), base_days: 10.0, extra_days: 4.0, note: "перенос".into() }).unwrap();
        let plan = db.plans().unwrap().remove(0);
        assert_eq!((plan.start, plan.total_days(), plan.end(), plan.note.as_str()), (Some(d(2026, 8, 3)), 14.0, Some(d(2026, 8, 16)), "перенос"));
        assert_eq!(db.plan_count().unwrap(), 1);
        db.delete_employee(emp).unwrap();
        assert_eq!((db.plan_count().unwrap(), db.leaves().unwrap().len()), (0, 0));
    }
}
