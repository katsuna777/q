//! Reads the old workbook («Отпуска.xlsx»): the sheets «Сотрудники», «Журнал отпусков» and
//! «Настройки». Columns are found by their header text, so small layout changes do not matter.

use crate::date::{self, Date};
use crate::db::{Import, ImportEmployee, ImportLeave};
use crate::model::*;
use anyhow::{bail, Context, Result};
use calamine::{open_workbook_auto, Data as Cell, Range, Reader};
use std::collections::HashMap;
use std::path::Path;

struct Grid(Range<Cell>);

impl Grid {
    fn get(&self, r: u32, c: u32) -> &Cell {
        self.0.get_value((r, c)).unwrap_or(&Cell::Empty)
    }

    fn text(&self, r: u32, c: u32) -> String {
        match self.get(r, c) {
            Cell::String(s) | Cell::DateTimeIso(s) => s.trim().to_string(),
            Cell::Float(f) if f.fract() == 0.0 => format!("{}", *f as i64),
            Cell::Float(f) => f.to_string(),
            Cell::Int(i) => i.to_string(),
            Cell::DateTime(d) => date::from_serial(d.as_f64()).map(date::fmt).unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn num(&self, r: u32, c: u32) -> Option<f64> {
        match self.get(r, c) {
            Cell::Float(f) => Some(*f),
            Cell::Int(i) => Some(*i as f64),
            Cell::String(s) => s.trim().replace(',', ".").parse().ok(),
            _ => None,
        }
    }

    fn date(&self, r: u32, c: u32) -> Option<Date> {
        match self.get(r, c) {
            Cell::DateTime(d) => date::from_serial(d.as_f64()),
            Cell::Float(f) => date::from_serial(*f),
            Cell::Int(i) => date::from_serial(*i as f64),
            Cell::String(s) | Cell::DateTimeIso(s) => date::parse_ru(s),
            _ => None,
        }
        .filter(|d| date::in_range(*d))
    }

    fn end_row(&self) -> u32 {
        self.0.end().map(|(r, _)| r + 1).unwrap_or(0)
    }

    fn end_col(&self) -> u32 {
        self.0.end().map(|(_, c)| c + 1).unwrap_or(0)
    }
}

/// Header text with spaces and line breaks removed, lower-cased: «Дата приема» → "датаприема".
fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).flat_map(|c| c.to_lowercase()).collect()
}

fn find_sheet(names: &[String], wanted: &str) -> Option<String> {
    names.iter().find(|n| norm(n) == norm(wanted)).cloned()
}

/// First row (within the top 15) that has a cell equal to «ФИО»; returns the row and the
/// header texts by column.
fn find_header(g: &Grid, also: &str) -> Option<(u32, Vec<(u32, String)>)> {
    for r in 0..15.min(g.end_row()) {
        let cells: Vec<(u32, String)> = (0..g.end_col()).map(|c| (c, norm(&g.text(r, c)))).collect();
        if cells.iter().any(|(_, t)| t == "фио") && cells.iter().any(|(_, t)| t.starts_with(also)) {
            return Some((r, cells));
        }
    }
    None
}

fn employee_key(h: &str) -> Option<&'static str> {
    Some(if h == "фио" {
        "fio"
    } else if h == "должность" {
        "position"
    } else if h == "подразделение" {
        "dept"
    } else if h.starts_with("датаприем") {
        "hire"
    } else if h.starts_with("статус") {
        "status"
    } else if h.starts_with("доп.завредн") {
        "extra"
    } else if h.starts_with("приостановкатд:с") {
        "susp_from"
    } else if h.starts_with("приостановкатд:по") {
        "susp_to"
    } else if h.starts_with("датауволн") {
        "dismiss"
    } else if h.starts_with("осн.отпуск") {
        "base"
    } else if h.starts_with("учётс") || h.starts_with("учетс") {
        "accrual"
    } else if h.starts_with("примечание") {
        "note"
    } else {
        return None;
    })
}

fn leave_key(h: &str) -> Option<&'static str> {
    Some(if h == "фио" {
        "fio"
    } else if h.starts_with("началоотпуска") {
        "start"
    } else if h.starts_with("основной") {
        "base"
    } else if h.starts_with("доп.завредн") {
        "extra"
    } else if h.starts_with("примечание") {
        "note"
    } else {
        return None;
    })
}

fn columns(header: &[(u32, String)], key_of: fn(&str) -> Option<&'static str>) -> HashMap<&'static str, u32> {
    let mut map = HashMap::new();
    for (c, h) in header {
        if let Some(k) = key_of(h) {
            map.entry(k).or_insert(*c);
        }
    }
    map
}

fn read_settings(g: &Grid, warnings: &mut Vec<String>) -> (Settings, Vec<String>) {
    let mut s = Settings::default();
    let mut found = 0;
    let mut departments = Vec::new();
    for r in 0..g.end_row().min(80) {
        let label = norm(&g.text(r, 1));
        let value = |c: u32| g.num(r, c);
        if label.starts_with("основнойотпуск,днейвгод") {
            s.base_days = value(2).unwrap_or(s.base_days);
            found += 1;
        } else if label.starts_with("правонапервыйотпуск") {
            s.first_leave_months = value(2).map_or(s.first_leave_months, |v| v as i32);
            found += 1;
        } else if label.starts_with("срокиспользования") {
            s.deadline_months = value(2).map_or(s.deadline_months, |v| v as i32);
            found += 1;
        } else if label.starts_with("«горит»") {
            s.burning_days = value(2).map_or(s.burning_days, |v| v as i32);
            found += 1;
        } else if label.starts_with("«скоро»") {
            s.soon_days = value(2).map_or(s.soon_days, |v| v as i32);
            found += 1;
        } else if label.starts_with("приостановкатд") {
            s.suspension_counts = norm(&g.text(r, 2)).starts_with("да");
            found += 1;
        } else if label.starts_with("подразделения(список") {
            for rr in r + 1..(r + 60).min(g.end_row()) {
                let name = g.text(rr, 1);
                if name.is_empty() {
                    break;
                }
                departments.push(name);
            }
        }
    }
    if found == 0 {
        warnings.push("Лист «Настройки» не распознан — параметры оставлены по умолчанию".into());
    }
    (s, departments)
}

pub fn read_workbook(path: &Path) -> Result<Import> {
    let mut wb = open_workbook_auto(path).with_context(|| format!("Не удалось открыть {}", path.display()))?;
    let names = wb.sheet_names();
    let mut import = Import::default();

    let emp_sheet = find_sheet(&names, "Сотрудники").context("В книге нет листа «Сотрудники»")?;
    let grid = Grid(wb.worksheet_range(&emp_sheet).context("Не удалось прочитать лист «Сотрудники»")?);

    // Settings and the department list.
    let mut settings_departments = Vec::new();
    if let Some(name) = find_sheet(&names, "Настройки") {
        if let Ok(range) = wb.worksheet_range(&name) {
            let (s, deps) = read_settings(&Grid(range), &mut import.warnings);
            import.settings = Some(s);
            settings_departments = deps;
        }
    } else {
        import.warnings.push("В книге нет листа «Настройки» — параметры оставлены по умолчанию".into());
    }
    import.departments = settings_departments;

    // Employees.
    let (hrow, header) = find_header(&grid, "должность").context("На листе «Сотрудники» не найдена строка заголовков (ФИО, Должность, …)")?;
    let col = columns(&header, employee_key);
    let fio_col = *col.get("fio").context("Нет колонки «ФИО»")?;
    let mut by_fio: HashMap<String, usize> = HashMap::new();
    let mut duplicates = 0;
    for r in hrow + 1..grid.end_row() {
        let fio = grid.text(r, fio_col);
        if fio.is_empty() {
            continue;
        }
        let get_text = |k: &str| col.get(k).map(|c| grid.text(r, *c)).unwrap_or_default();
        let get_num = |k: &str| col.get(k).and_then(|c| grid.num(r, *c));
        let get_date = |k: &str| col.get(k).and_then(|c| grid.date(r, *c));
        let dept = get_text("dept");
        let department = if dept.is_empty() {
            None
        } else {
            if !import.departments.contains(&dept) {
                import.departments.push(dept.clone());
                import.warnings.push(format!("Подразделение «{dept}» не было в списке «Настройки» — добавлено"));
            }
            Some(dept)
        };
        let employee = Employee {
            fio: fio.clone(),
            position: get_text("position"),
            hire_date: get_date("hire"),
            status: Status::from_label(&get_text("status")),
            extra_days: get_num("extra").filter(|v| *v > 0.0),
            suspend_from: get_date("susp_from"),
            suspend_to: get_date("susp_to"),
            dismiss_date: get_date("dismiss"),
            base_days: get_num("base").filter(|v| *v > 0.0),
            accrual_from: get_date("accrual"),
            note: get_text("note"),
            ..Default::default()
        };
        if by_fio.insert(fio, import.employees.len()).is_some() {
            duplicates += 1;
        }
        import.employees.push(ImportEmployee { employee, department });
    }
    if import.employees.is_empty() {
        bail!("На листе «Сотрудники» нет ни одного сотрудника");
    }
    if duplicates > 0 {
        import.warnings.push(format!("{duplicates} сотрудников с одинаковыми ФИО: их отпуска из журнала отнесены к первому из них"));
    }

    // Journal.
    if let Some(name) = find_sheet(&names, "Журнал отпусков") {
        let grid = Grid(wb.worksheet_range(&name).context("Не удалось прочитать лист «Журнал отпусков»")?);
        if let Some((hrow, header)) = find_header(&grid, "началоотпуска") {
            let col = columns(&header, leave_key);
            let fio_col = *col.get("fio").context("В журнале нет колонки «ФИО»")?;
            let (mut unknown, mut empty) = (0, 0);
            for r in hrow + 1..grid.end_row() {
                let fio = grid.text(r, fio_col);
                if fio.is_empty() {
                    continue;
                }
                let num = |k: &str| col.get(k).and_then(|c| grid.num(r, *c)).unwrap_or(0.0).max(0.0);
                let (base, extra) = (num("base"), num("extra"));
                let Some(&employee) = by_fio.get(&fio) else {
                    unknown += 1;
                    continue;
                };
                if base + extra == 0.0 {
                    empty += 1;
                    continue;
                }
                import.leaves.push(ImportLeave {
                    employee,
                    leave: Leave {
                        start: col.get("start").and_then(|c| grid.date(r, *c)),
                        base_days: base,
                        extra_days: extra,
                        note: col.get("note").map(|c| grid.text(r, *c)).unwrap_or_default(),
                        ..Default::default()
                    },
                });
            }
            if unknown > 0 {
                import.warnings.push(format!("{unknown} записей журнала пропущено: такого ФИО нет среди сотрудников"));
            }
            if empty > 0 {
                import.warnings.push(format!("{empty} записей журнала без дней отпуска пропущено"));
            }
        } else {
            import.warnings.push("На листе «Журнал отпусков» не найдена строка заголовков — журнал не загружен".into());
        }
    } else {
        import.warnings.push("В книге нет листа «Журнал отпусков» — журнал не загружен".into());
    }
    Ok(import)
}

impl Import {
    pub fn summary(&self) -> String {
        format!("{} сотрудников, {} записей журнала, {} подразделений", self.employees.len(), self.leaves.len(), self.departments.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_normalisation() {
        assert_eq!(norm("Дата приема"), "датаприема");
        assert_eq!(norm("Приостановка ТД: с"), "приостановкатд:с");
        assert_eq!(norm("Доп. за\nвредн., дн./год"), "доп.завредн.,дн./год");
        assert_eq!(employee_key(&norm("Осн. отпуск, дн./год (не 28)")), Some("base"));
        assert_eq!(employee_key(&norm("Доп. за вредн., дн./год")), Some("extra"));
        assert_eq!(employee_key(&norm("Приостановка ТД: по (возврат)")), Some("susp_to"));
        assert_eq!(employee_key(&norm("Учёт с (годовщина приема)")), Some("accrual"));
        assert_eq!(employee_key(&norm("Остаток всего, дн.")), None);
        assert_eq!(leave_key(&norm("Начало отпуска")), Some("start"));
        assert_eq!(leave_key(&norm("Основной, дн.")), Some("base"));
        assert_eq!(leave_key(&norm("Доп. за вредн., дн.")), Some("extra"));
    }
}
