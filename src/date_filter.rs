//! Shared native date filters. Calendar comparisons use local Windows dates,
//! not UTC days or rolling 24-hour windows; no media/files are opened here.
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: u32,
    pub month: u32,
    pub day: u32,
}
impl Date {
    fn month_days(year: u32, month: u32) -> u32 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                    29
                } else {
                    28
                }
            }
            _ => 0,
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<_> = text.trim().split('/').collect();
        if parts.len() != 3
            || parts
                .iter()
                .any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()))
        {
            return None;
        }
        let date = Self {
            day: parts[0].parse().ok()?,
            month: parts[1].parse().ok()?,
            year: parts[2].parse().ok()?,
        };
        ((1970..=9999).contains(&date.year)
            && date.day >= 1
            && date.day <= Self::month_days(date.year, date.month))
        .then_some(date)
    }
    pub fn display(self) -> String {
        format!("{:02}/{:02}/{:04}", self.day, self.month, self.year)
    }
    fn days(self) -> i64 {
        let before = |y: u32| {
            let y = i64::from(y) - 1;
            y * 365 + y / 4 - y / 100 + y / 400
        };
        before(self.year) - before(1970)
            + (1..self.month)
                .map(|m| i64::from(Self::month_days(self.year, m)))
                .sum::<i64>()
            + i64::from(self.day)
            - 1
    }
}
pub fn unix_time(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}
pub fn now() -> u64 {
    unix_time(SystemTime::now()).unwrap_or(0)
}

pub fn local_parts(seconds: u64) -> Option<(Date, u32, u32)> {
    if seconds > 253_402_300_799 {
        return None;
    }
    #[cfg(windows)]
    {
        #[repr(C)]
        struct FileTime {
            low: u32,
            high: u32,
        }
        #[repr(C)]
        #[derive(Default)]
        struct WinTime {
            year: u16,
            month: u16,
            weekday: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            millisecond: u16,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn FileTimeToSystemTime(file: *const FileTime, out: *mut WinTime) -> i32;
            fn SystemTimeToTzSpecificLocalTime(
                zone: *const std::ffi::c_void,
                utc: *const WinTime,
                local: *mut WinTime,
            ) -> i32;
        }
        let ticks = (seconds + 11_644_473_600) * 10_000_000;
        let file = FileTime {
            low: ticks as u32,
            high: (ticks >> 32) as u32,
        };
        let mut utc = WinTime::default();
        let mut local = WinTime::default();
        if unsafe {
            FileTimeToSystemTime(&file, &mut utc) != 0
                && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) != 0
        } {
            return Some((
                Date {
                    year: local.year.into(),
                    month: local.month.into(),
                    day: local.day.into(),
                },
                local.hour.into(),
                local.minute.into(),
            ));
        }
    }
    // Portable fallback for non-Windows builds, or failed timezone conversion.
    let mut days = seconds / 86400;
    let mut year = 1970;
    let mut month = 1;
    loop {
        let count = if Date::month_days(year, 2) == 29 {
            366
        } else {
            365
        };
        if days < count {
            break;
        }
        days -= count;
        year += 1;
    }
    while days >= u64::from(Date::month_days(year, month)) {
        days -= u64::from(Date::month_days(year, month));
        month += 1;
    }
    Some((
        Date {
            year,
            month,
            day: days as u32 + 1,
        },
        ((seconds % 86400) / 3600) as u32,
        ((seconds % 3600) / 60) as u32,
    ))
}
pub fn timestamp(seconds: Option<u64>) -> String {
    seconds
        .and_then(local_parts)
        .map(|(d, h, m)| format!("{} {h:02}:{m:02}", d.display()))
        .unwrap_or_else(|| "Chưa rõ ngày".into())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Period {
    #[default]
    All,
    Today,
    Yesterday,
    Last7,
    Last30,
    Day,
    Range,
}
impl Period {
    fn label(self) -> &'static str {
        match self {
            Self::All => "Tất cả ngày",
            Self::Today => "Hôm nay",
            Self::Yesterday => "Hôm qua",
            Self::Last7 => "7 ngày gần đây",
            Self::Last30 => "30 ngày gần đây",
            Self::Day => "Ngày cụ thể",
            Self::Range => "Khoảng ngày",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DateFilter {
    pub period: Period,
    pub from: String,
    pub to: String,
}
impl Default for DateFilter {
    fn default() -> Self {
        let today = local_parts(now())
            .map(|(d, _, _)| d.display())
            .unwrap_or_else(|| "01/01/1970".into());
        Self {
            period: Period::All,
            from: today.clone(),
            to: today,
        }
    }
}
impl DateFilter {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, path: &Path) {
        let _ = crate::model::write_json(path, self);
    }
    pub fn error(&self) -> Option<&'static str> {
        if !matches!(self.period, Period::Day | Period::Range) {
            return None;
        }
        let Some(from) = Date::parse(&self.from) else {
            return Some("Nhập ngày hợp lệ theo DD/MM/YYYY, ví dụ 24/10/2026.");
        };
        if self.period == Period::Range {
            let Some(to) = Date::parse(&self.to) else {
                return Some("Ngày kết thúc cần đúng DD/MM/YYYY.");
            };
            if from > to {
                return Some("Ngày bắt đầu phải trước hoặc bằng ngày kết thúc.");
            }
        }
        None
    }
    pub fn matches(&self, seconds: Option<u64>, today: Date) -> bool {
        if self.period == Period::All {
            return true;
        }
        if self.error().is_some() {
            return false;
        }
        let Some((day, _, _)) = seconds.and_then(local_parts) else {
            return false;
        };
        let delta = today.days() - day.days();
        match self.period {
            Period::All => true,
            Period::Today => delta == 0,
            Period::Yesterday => delta == 1,
            Period::Last7 => (0..7).contains(&delta),
            Period::Last30 => (0..30).contains(&delta),
            Period::Day => Date::parse(&self.from) == Some(day),
            Period::Range => Date::parse(&self.from)
                .zip(Date::parse(&self.to))
                .is_some_and(|(from, to)| from <= day && day <= to),
        }
    }
    /// Responsive controls return their rectangles for native layout checks.
    pub fn ui(&mut self, ui: &mut egui::Ui, id: &str) -> Vec<egui::Rect> {
        let rects = ui.horizontal_wrapped(|ui| self.inline_ui(ui, id)).inner;
        if let Some(error) = self.error() {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        rects
    }
    /// Join an existing wrapped row without starting a second row.
    pub fn inline_ui(&mut self, ui: &mut egui::Ui, id: &str) -> Vec<egui::Rect> {
        let mut rects = vec![];
        ui.label("Lọc ngày");
        let combo = egui::ComboBox::from_id_salt((id, "period"))
            .width(160.)
            .selected_text(self.period.label())
            .show_ui(ui, |ui| {
                for mode in [
                    Period::All,
                    Period::Today,
                    Period::Yesterday,
                    Period::Last7,
                    Period::Last30,
                    Period::Day,
                    Period::Range,
                ] {
                    ui.selectable_value(&mut self.period, mode, mode.label());
                }
            });
        rects.push(combo.response.rect);
        if matches!(self.period, Period::Day | Period::Range) {
            rects.push(
                ui.add(
                    egui::TextEdit::singleline(&mut self.from)
                        .id_salt((id, "from"))
                        .desired_width(108.)
                        .hint_text("DD/MM/YYYY")
                        .char_limit(10),
                )
                .on_hover_text("Ngày bắt đầu (DD/MM/YYYY)")
                .rect,
            );
            if self.period == Period::Range {
                ui.label("đến");
                rects.push(
                    ui.add(
                        egui::TextEdit::singleline(&mut self.to)
                            .id_salt((id, "to"))
                            .desired_width(108.)
                            .hint_text("DD/MM/YYYY")
                            .char_limit(10),
                    )
                    .on_hover_text("Ngày kết thúc (DD/MM/YYYY)")
                    .rect,
                );
            }
        }
        if self.period != Period::All {
            let reset = ui.button("Bỏ lọc");
            rects.push(reset.rect);
            if reset.clicked() {
                self.period = Period::All;
            }
        }
        rects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_validate_leap_days_and_ranges() {
        assert!(Date::parse("29/02/2024").is_some());
        assert!(Date::parse("29/02/2025").is_none());
        for bad in [
            "31/04/2026",
            "00/10/2026",
            "24/13/2026",
            "2026-10-24",
            "1/1/1969",
        ] {
            assert!(Date::parse(bad).is_none());
        }
        assert_eq!(Date::parse("01/01/1970").unwrap().days(), 0);
        assert_eq!(
            Date::parse("01/03/2024").unwrap().days() - Date::parse("28/02/2024").unwrap().days(),
            2
        );
        let filter = DateFilter {
            period: Period::Range,
            from: "25/10/2026".into(),
            to: "24/10/2026".into(),
        };
        assert!(filter.error().is_some());
    }
    #[test]
    fn filtering_uses_inclusive_calendar_dates_and_excludes_future_days() {
        let base = 1_792_800_000;
        let today = local_parts(base).unwrap().0;
        let mut filter = DateFilter {
            period: Period::Today,
            ..Default::default()
        };
        assert!(filter.matches(Some(base), today));
        assert!(!filter.matches(Some(base - 86400), today));
        filter.period = Period::Yesterday;
        assert!(filter.matches(Some(base - 86400), today));
        filter.period = Period::Last7;
        assert!(filter.matches(Some(base - 6 * 86400), today));
        assert!(!filter.matches(Some(base - 7 * 86400), today));
        assert!(!filter.matches(Some(base + 86400), today));
        filter.period = Period::Range;
        filter.from = today.display();
        filter.to = today.display();
        assert!(filter.matches(Some(base), today));
        filter.period = Period::All;
        assert!(filter.matches(None, today));
    }
    #[test]
    fn controls_wrap_and_preferences_survive_restart() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("dates.json");
        let mut filter = DateFilter {
            period: Period::Range,
            ..Default::default()
        };
        filter.save(&path);
        assert_eq!(DateFilter::load(&path), filter);
        for width in [280., 540., 1080.] {
            let ctx = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(width, 500.));
            for _ in 0..3 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            for rect in filter.ui(ui, "test") {
                                assert!(
                                    screen.contains_rect(rect),
                                    "Date control outside {width}px: {rect:?}"
                                );
                            }
                        });
                    },
                );
            }
        }
    }
}
