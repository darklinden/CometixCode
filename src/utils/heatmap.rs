//! Maps to: CC `utils/heatmap.ts`.
//!
//! The grid/date/intensity algorithm is preserved. Coloring remains owned by
//! the surrounding themed `Ansi`/Text renderer; this helper returns the same
//! visible glyph matrix without hardcoded RGB values.

use crate::utils::stats::DailyActivity;
use chrono::{Datelike, Local, TimeZone, Utc};
use std::collections::BTreeMap;
#[cfg(test)]
use chrono::NaiveDate;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeatmapOptions {
    pub terminal_width: Option<usize>,
    pub show_month_labels: Option<bool>,
    #[cfg(test)]
    pub today: Option<NaiveDate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Percentiles {
    p25: u64,
    p50: u64,
    p75: u64,
}

fn calculate_percentiles(activity: &[DailyActivity]) -> Option<Percentiles> {
    let mut counts = activity
        .iter()
        .map(|day| day.message_count)
        .filter(|count| *count > 0)
        .collect::<Vec<_>>();
    if counts.is_empty() {
        return None;
    }
    counts.sort_unstable();
    Some(Percentiles {
        p25: counts[counts.len() * 25 / 100],
        p50: counts[counts.len() * 50 / 100],
        p75: counts[counts.len() * 75 / 100],
    })
}

fn intensity(message_count: u64, percentiles: Option<Percentiles>) -> usize {
    let Some(percentiles) = percentiles else {
        return 0;
    };
    if message_count == 0 {
        0
    } else if message_count >= percentiles.p75 {
        4
    } else if message_count >= percentiles.p50 {
        3
    } else if message_count >= percentiles.p25 {
        2
    } else {
        1
    }
}

fn heatmap_char(level: usize) -> char {
    match level {
        1 => '░',
        2 => '▒',
        3 => '▓',
        4 => '█',
        _ => '·',
    }
}

/// Maps to: CC `utils/heatmap.ts#generateHeatmap`.
pub fn generate_heatmap(activity: &[DailyActivity], options: HeatmapOptions) -> String {
    let terminal_width = options.terminal_width.unwrap_or(80);
    let show_month_labels = options.show_month_labels.unwrap_or(true);
    let width = terminal_width.saturating_sub(4).clamp(10, 52);
    let activity_by_date = activity
        .iter()
        .map(|day| (day.date.as_str(), day))
        .collect::<BTreeMap<_, _>>();
    let percentiles = calculate_percentiles(activity);
    #[cfg(test)]
    let today = options.today.unwrap_or_else(|| Local::now().date_naive());
    #[cfg(not(test))]
    let today = Local::now().date_naive();
    let current_week_start = today
        .checked_sub_days(chrono::Days::new(
            today.weekday().num_days_from_sunday() as u64
        ))
        .unwrap_or(today);
    let start_date = current_week_start
        .checked_sub_days(chrono::Days::new((width.saturating_sub(1) * 7) as u64))
        .unwrap_or(current_week_start);

    let mut grid = vec![vec![' '; width]; 7];
    let mut month_starts = Vec::<(u32, usize)>::new();
    let mut last_month = None;
    let mut current_date = start_date;
    for week in 0..width {
        for row in grid.iter_mut().take(7) {
            if current_date <= today {
                if current_date.weekday().num_days_from_sunday() == 0 {
                    let month = current_date.month0();
                    if last_month != Some(month) {
                        month_starts.push((month, week));
                        last_month = Some(month);
                    }
                }
                // CC constructs local-midnight Date objects, then calls
                // `toISOString()` for the map key. Preserve that timezone
                // projection rather than treating the calendar cell as UTC.
                let local_midnight = current_date.and_hms_opt(0, 0, 0).unwrap();
                let utc_date = Local
                    .from_local_datetime(&local_midnight)
                    .earliest()
                    .map(|value| value.with_timezone(&Utc).date_naive())
                    .unwrap_or(current_date);
                let key = utc_date.format("%Y-%m-%d").to_string();
                let count = activity_by_date
                    .get(key.as_str())
                    .map(|day| day.message_count)
                    .unwrap_or(0);
                row[week] = heatmap_char(intensity(count, percentiles));
            }
            if let Some(next) = current_date.succ_opt() {
                current_date = next;
            }
        }
    }

    let mut lines = Vec::new();
    if show_month_labels {
        let month_names = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let label_width = width / month_starts.len().max(1);
        let labels = month_starts
            .iter()
            .map(|(month, _)| {
                format!(
                    "{:<width$}",
                    month_names[*month as usize],
                    width = label_width
                )
            })
            .collect::<String>();
        lines.push(format!("    {labels}"));
    }
    let day_labels = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    for (day, row) in grid.into_iter().enumerate() {
        let label = if [1, 3, 5].contains(&day) {
            day_labels[day]
        } else {
            "   "
        };
        lines.push(format!("{label} {}", row.into_iter().collect::<String>()));
    }
    lines.push(String::new());
    lines.push("    Less ░ ▒ ▓ █ More".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heatmap_matches_official_width_labels_and_percentile_glyphs() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 19).unwrap();
        let activity = vec![
            DailyActivity {
                date: "2026-07-17".to_string(),
                message_count: 1,
                ..DailyActivity::default()
            },
            DailyActivity {
                date: "2026-07-18".to_string(),
                message_count: 10,
                ..DailyActivity::default()
            },
        ];
        let output = generate_heatmap(
            &activity,
            HeatmapOptions {
                terminal_width: Some(20),
                show_month_labels: Some(true),
                today: Some(today),
            },
        );
        assert!(output.contains("Jul"), "{output}");
        assert!(output.contains('█'), "{output}");
        assert!(output.contains("Less ░ ▒ ▓ █ More"), "{output}");
        assert_eq!(output.lines().count(), 10);
    }
}
