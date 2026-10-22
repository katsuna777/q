//! SQLite storage. One file, WAL mode; the whole data set is small (hundreds of employees,
//! a couple of thousand journal rows), so reads load everything and writes are single statements.

use crate::date::{self, Date};
use crate::model::*;
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i32 = 1;

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
    created_at  INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);
CREATE INDEX leaves_employee ON leaves(employee_id);
CREATE INDEX employees_department ON employees(department_id);
";

pub struct Db {
    conn: Connection,
    path: PathBuf,
}

fn opt_date(d: Option<Date>) -> Option<String> {
    d.map(date::iso)
}

fn read_date(s: Option<String>) -> Option<Date> {
    s.as_deref().and_then(date::parse_iso)
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        Self::init(conn, path.to_path_buf())
    }

    pub fn open_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory()?, PathBuf::from(":memory:"))
    }

    fn init(conn: Connection, path: PathBuf) -> Result<Db> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let db = Db { conn, path };
        db.migrate()?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&self) -> Result<()> {
        let version: i32 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match version {
            0 => {
                self.conn.execute_batch(SCHEMA)?;
                self.conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            }
            v if v == SCHEMA_VERSION => {}
            v => bail!("База создана более новой версией программы (схема {v}, поддерживается {SCHEMA_VERSION})"),
        }
        Ok(())
    }

    // ---- reading ---------------------------------------------------------------------

    pub fn load(&self) -> Result<Data> {
        Ok(Data {
            settings: self.settings()?,
            departments: self.departments()?,
            employees: self.employees()?,
            leaves: self.leaves()?,
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
        let mut stmt = self.conn.prepare("SELECT id, employee_id, start_date, base_days, extra_days, note FROM leaves ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Leave {
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
        let pairs: [(&str, String); 7] = [
            ("as_of", s.as_of_manual.map(date::iso).unwrap_or_default()),
            ("base_days", s.base_days.to_string()),
            ("first_leave_months", s.first_leave_months.to_string()),
            ("deadline_months", s.deadline_months.to_string()),
            ("burning_days", s.burning_days.to_string()),
            ("soon_days", s.soon_days.to_string()),
            ("suspension_counts", if s.suspension_counts { "1" } else { "0" }.to_string()),
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

    pub fn insert_leave(&self, l: &Leave) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO leaves(employee_id, start_date, base_days, extra_days, note) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![l.employee_id, opt_date(l.start), l.base_days, l.extra_days, l.note.trim()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_leave(&self, l: &Leave) -> Result<()> {
        self.conn.execute(
            "UPDATE leaves SET employee_id = ?1, start_date = ?2, base_days = ?3, extra_days = ?4, note = ?5 WHERE id = ?6",
            params![l.employee_id, opt_date(l.start), l.base_days, l.extra_days, l.note.trim(), l.id],
        )?;
        Ok(())
    }

    pub fn delete_leave(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM leaves WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- bulk --------------------------------------------------------------------------

    /// Replaces all data with `import` in one transaction.
    pub fn replace_all(&mut self, import: &Import) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("DELETE FROM leaves; DELETE FROM employees; DELETE FROM departments;")?;
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
                "INSERT INTO leaves(employee_id, start_date, base_days, extra_days, note) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![emp_ids[il.employee], opt_date(l.start), l.base_days, l.extra_days, l.note],
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

    /// Last `limit` rows of any table are not needed; this is for tests and diagnostics.
    pub fn counts(&self) -> Result<(i64, i64, i64)> {
        let q = |t: &str| -> Result<i64> { Ok(self.conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))?) };
        Ok((q("departments")?, q("employees")?, q("leaves")?))
    }
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
        db.update_leave(&Leave { id: lid, employee_id: id, start: None, base_days: 10.5, extra_days: 3.0, note: "приказ 5".into() }).unwrap();
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
}
