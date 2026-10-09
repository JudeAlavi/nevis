//! Mermaid Gantt charts (```` ```mermaid ```` blocks starting with `gantt`): tasks read with
//! their dates (from a date, after other tasks, or after the one before; lasting a while, to a
//! date, or until another starts), laid out along a time axis by section, and drawn.

use std::collections::HashMap;

use gtk::{gdk, glib, graphene, gsk, pango, prelude::*};

use crate::flowchart::Rect;
use crate::{render, theme};

/// A task: its name, its section (index), when it starts and ends (seconds since 1970, UTC),
/// and what kind it is.
#[derive(Debug, PartialEq)]
pub struct Task {
    pub name: String,
    pub section: usize,
    pub start: f64,
    pub end: f64,
    pub done: bool,
    pub active: bool,
    pub crit: bool,
    pub milestone: bool,
}

#[derive(Debug, PartialEq)]
pub struct Gantt {
    pub title: String,
    pub sections: Vec<String>,
    pub tasks: Vec<Task>,
    /// `axisFormat`, as strftime writes it (`%Y-%m-%d` unless given).
    pub axis: String,
}

const DAY: f64 = 86_400.0;

/// The Gantt chart in a Mermaid block, or nothing when it is another kind of diagram.
pub fn parse(source: &str) -> Option<Gantt> {
    let mut lines = source.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with("%%"));
    if lines.next()? != "gantt" {
        return None;
    }
    let mut gantt = Gantt { title: String::new(), sections: Vec::new(), tasks: Vec::new(), axis: "%Y-%m-%d".to_owned() };
    let mut format = "YYYY-MM-DD".to_owned();
    let mut weekends = false;
    // Each task's timing (kinds left out), and the tasks by id.
    let mut timings: Vec<Vec<String>> = Vec::new();
    let mut ids: HashMap<String, usize> = HashMap::new();
    for line in lines {
        let (word, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(word, rest)| (word, rest.trim()));
        match word {
            "title" => gantt.title = rest.to_owned(),
            "dateFormat" => format = rest.to_owned(),
            "axisFormat" => gantt.axis = rest.to_owned(),
            "excludes" => weekends = rest.split(',').any(|day| day.trim() == "weekends"),
            "section" => gantt.sections.push(rest.to_owned()),
            "todayMarker" | "tickInterval" | "includes" | "weekday" | "displayMode" | "inclusiveEndDates" | "topAxis" | "click" | "accTitle" | "accDescr" => {}
            _ => {
                let Some((name, timing)) = line.split_once(':') else { continue };
                // Kinds first (`done`, `active`, `crit`, `milestone`), then maybe an id, a start
                // and an end.
                let mut items: Vec<String> = timing.split(',').map(|item| item.trim().to_owned()).filter(|item| !item.is_empty()).collect();
                let mut kind = |word: &str| items.iter().position(|item| item == word).map(|at| items.remove(at)).is_some();
                let (done, active, crit, milestone) = (kind("done"), kind("active"), kind("crit"), kind("milestone"));
                if items.len() >= 3 {
                    ids.insert(items.remove(0), gantt.tasks.len());
                }
                let section = gantt.sections.len().saturating_sub(1);
                gantt.tasks.push(Task { name: name.trim().to_owned(), section, start: 0.0, end: 0.0, done, active, crit, milestone });
                timings.push(items);
            }
        }
    }
    // Worked out again until they settle: a task may name one that comes after it.
    for _ in 0..=gantt.tasks.len() {
        let mut changed = false;
        for (index, timing) in timings.iter().enumerate() {
            let (start, end) = when(&gantt.tasks, index, timing, &format, weekends, &ids);
            let task = &mut gantt.tasks[index];
            if (task.start, task.end) != (start, end) {
                (task.start, task.end) = (start, end);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Some(gantt)
}

/// When the task at `index` starts and ends, from its `timing` (start and end, or the end
/// alone), as `tasks` stand now: after the tasks named (the last of them to end), at a date,
/// or after the task before; until another starts, at a date, or lasting a while.
fn when(tasks: &[Task], index: usize, timing: &[String], format: &str, weekends: bool, ids: &HashMap<String, usize>) -> (f64, f64) {
    let (start, end) = match timing {
        [] => (None, None),
        [end] => (None, Some(end.as_str())),
        [start, end, ..] => (Some(start.as_str()), Some(end.as_str())),
    };
    let before = index.checked_sub(1).map_or(0.0, |previous| tasks[previous].end);
    let named = |id: &str| ids.get(id.trim()).map(|&other| &tasks[other]);
    let start = match start {
        Some(after) if after.starts_with("after ") => {
            let ends = after[6..].split_whitespace().filter_map(named).map(|task| task.end).fold(f64::NEG_INFINITY, f64::max);
            if ends.is_finite() { ends } else { before }
        }
        Some(date) => moment(date, format).unwrap_or(before),
        None => before,
    };
    let end = match end {
        Some(until) if until.starts_with("until ") => named(&until[6..]).map_or(start, |task| task.start),
        Some(text) => match length(text) {
            Some((seconds, days)) if weekends && days => workdays(start, seconds / DAY),
            Some((seconds, _)) => start + seconds,
            None => moment(text, format).unwrap_or(start),
        },
        None => start,
    };
    (start, end)
}

/// A length (`3d`, `2w`, `12h`, `30m`, `1.5d`): in seconds, and whether it counts days.
fn length(text: &str) -> Option<(f64, bool)> {
    let digits = text.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let number: f64 = text[..digits].parse().ok()?;
    let (unit, days) = match &text[digits..] {
        "ms" => (0.001, false),
        "s" => (1.0, false),
        "m" => (60.0, false),
        "h" => (3600.0, false),
        "d" => (DAY, true),
        "w" => (7.0 * DAY, false),
        "M" => (30.0 * DAY, false),
        "y" => (365.0 * DAY, false),
        _ => return None,
    };
    Some((number * unit, days))
}

/// `days` working days from `start`, Saturdays and Sundays not counting.
fn workdays(start: f64, days: f64) -> f64 {
    let mut day = (start / DAY).floor() as i64;
    let mut left = days.ceil() as i64;
    while left > 0 {
        // 1970-01-01 was a Thursday: 0 is a Sunday here.
        if !matches!((day + 4).rem_euclid(7), 0 | 6) {
            left -= 1;
        }
        day += 1;
    }
    day as f64 * DAY
}

/// A date (and time) written as `format` says (`YYYY-MM-DD`, `YYYY-MM-DD HH:mm`…): seconds
/// since 1970, UTC.
fn moment(text: &str, format: &str) -> Option<f64> {
    let (mut year, mut month, mut day, mut hour, mut minute, mut second) = (1970i64, 1u32, 1u32, 0i64, 0i64, 0i64);
    let (text, mut rest) = (text.trim(), format.trim());
    let mut at = 0;
    let number = |at: &mut usize, most: usize| -> Option<i64> {
        let digits: String = text[*at..].chars().take_while(char::is_ascii_digit).take(most).collect();
        *at += digits.len();
        digits.parse().ok()
    };
    while !rest.is_empty() {
        let token = ["YYYY", "YY", "MM", "M", "DD", "D", "HH", "H", "mm", "ss", "X"].into_iter().find(|token| rest.starts_with(token));
        match token {
            Some("YYYY") => year = number(&mut at, 4)?,
            Some("YY") => year = 2000 + number(&mut at, 2)?,
            Some("MM" | "M") => month = number(&mut at, 2)? as u32,
            Some("DD" | "D") => day = number(&mut at, 2)? as u32,
            Some("HH" | "H") => hour = number(&mut at, 2)?,
            Some("mm") => minute = number(&mut at, 2)?,
            Some("ss") => second = number(&mut at, 2)?,
            Some(_) => return number(&mut at, 12).map(|seconds| seconds as f64),
            None => {
                // A literal: the same in the text.
                let c = rest.chars().next()?;
                if !text[at..].starts_with(c) {
                    return None;
                }
                at += c.len_utf8();
                rest = &rest[c.len_utf8()..];
                continue;
            }
        }
        rest = &rest[token.map_or(0, str::len)..];
    }
    Some(days_from_civil(year, month, day) as f64 * DAY + (hour * 3600 + minute * 60 + second) as f64)
}

/// Where everything goes, in pixels from the top left.
#[derive(Debug)]
pub struct Layout {
    pub width: f64,
    pub height: f64,
    /// Each task's bar (a milestone's is where its diamond goes).
    pub bars: Vec<Rect>,
    /// Each section's band, across the chart.
    pub bands: Vec<Rect>,
    /// The axis's ticks: where across, and the time each marks.
    pub ticks: Vec<(f64, f64)>,
    /// How far down the axis is.
    pub axis: f64,
}

/// A row, a bar in it, the space round the chart, under the title, and least between ticks.
const ROW: f64 = 28.0;
const BAR: f64 = 20.0;
const MARGIN: f64 = 8.0;
const UNDER_TITLE: f64 = 12.0;
const TICK_GAP: f64 = 70.0;

/// Lays `gantt` out `room` pixels wide (the bars' part), given the size of each section's name
/// and the title's.
pub fn layout(gantt: &Gantt, sections: &[(f64, f64)], title: (f64, f64), room: f64) -> Layout {
    let first = gantt.tasks.iter().map(|task| task.start).fold(f64::INFINITY, f64::min);
    let last = gantt.tasks.iter().map(|task| task.end).fold(f64::NEG_INFINITY, f64::max);
    let (first, last) = if first.is_finite() { (first, last.max(first + DAY)) } else { (0.0, DAY) };
    let scale = room / (last - first);
    // Section names in a column of their own, left of the bars.
    let column = sections.iter().map(|(width, _)| *width).fold(0.0, f64::max);
    let left = MARGIN + if column > 0.0 { column + 16.0 } else { 0.0 };
    let top = MARGIN + if title.1 > 0.0 { title.1 + UNDER_TITLE } else { 0.0 };
    let bars: Vec<Rect> = gantt
        .tasks
        .iter()
        .enumerate()
        .map(|(row, task)| {
            let y = top + row as f64 * ROW + (ROW - BAR) / 2.0;
            let x = left + (task.start - first) * scale;
            if task.milestone {
                Rect { x: x - BAR / 2.0, y, width: BAR, height: BAR }
            } else {
                Rect { x, y, width: ((task.end - task.start) * scale).max(2.0), height: BAR }
            }
        })
        .collect();
    let bands = (0..gantt.sections.len())
        .filter_map(|section| {
            let rows: Vec<usize> = gantt.tasks.iter().enumerate().filter(|(_, task)| task.section == section).map(|(row, _)| row).collect();
            let (&first, &last) = (rows.first()?, rows.last()?);
            Some(Rect { x: MARGIN, y: top + first as f64 * ROW, width: left - MARGIN + room, height: (last - first + 1) as f64 * ROW })
        })
        .collect();
    let axis = top + gantt.tasks.len() as f64 * ROW + 4.0;
    // Ticks every so often: the shortest step that leaves room for their dates.
    let steps = [3600.0, 3.0 * 3600.0, 6.0 * 3600.0, 12.0 * 3600.0, DAY, 2.0 * DAY, 7.0 * DAY, 14.0 * DAY, 30.0 * DAY, 91.0 * DAY, 365.0 * DAY];
    let step = steps.into_iter().find(|step| step * scale >= TICK_GAP).unwrap_or(365.0 * DAY);
    let mut ticks = Vec::new();
    let mut time = (first / step).ceil() * step;
    // Weeks start on Mondays (1970-01-05 was one), longer steps on the first of a month.
    if step >= 7.0 * DAY {
        let day = (first / DAY).ceil() as i64;
        time = if step >= 30.0 * DAY {
            let (year, month, date) = civil_from_days(day);
            let (year, month) = if date == 1 { (year, month) } else if month == 12 { (year + 1, 1) } else { (year, month + 1) };
            days_from_civil(year, month, 1) as f64 * DAY
        } else {
            (day + (4 - day).rem_euclid(7)) as f64 * DAY
        };
    }
    while time <= last + 1.0 {
        ticks.push((left + (time - first) * scale, time));
        time = if step >= 30.0 * DAY { next_month(time, step) } else { time + step };
    }
    Layout { width: left + room + MARGIN, height: axis + 24.0 + MARGIN, bars, bands, ticks, axis }
}

/// Wider charts are shown smaller.
const WIDEST: f64 = 900.0;

/// The chart drawn as a picture; `text` lays out words in the rendered view's font.
pub fn picture(gantt: &Gantt, text: &dyn Fn(&str) -> pango::Layout) -> gtk::Picture {
    let size = |layout: &pango::Layout| {
        let (width, height) = layout.pixel_size();
        (f64::from(width), f64::from(height))
    };
    let title = (!gantt.title.is_empty()).then(|| {
        let layout = text("");
        layout.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(&gantt.title)));
        layout
    });
    let names: Vec<pango::Layout> = gantt.tasks.iter().map(|task| text(&task.name)).collect();
    let sections: Vec<pango::Layout> = gantt.sections.iter().map(|section| text(section)).collect();
    let laid = layout(
        gantt,
        &sections.iter().map(size).collect::<Vec<_>>(),
        title.as_ref().map_or((0.0, 0.0), size),
        640.0,
    );

    let colors = theme::colors();
    let tint = |color, alpha| render::rgba(color, alpha);
    let snapshot = gtk::Snapshot::new();
    let put = |layout: &pango::Layout, x: f64, y: f64, ink: &gdk::RGBA| {
        snapshot.save();
        snapshot.translate(&graphene::Point::new(x as f32, y as f32));
        snapshot.append_layout(layout, ink);
        snapshot.restore();
    };
    if let Some(title) = &title {
        put(title, (laid.width - size(title).0) / 2.0, MARGIN, &tint(colors.text, 1.0));
    }
    // Sections: every other one on a band, its name at the left.
    for (index, (band, name)) in laid.bands.iter().zip(&sections).enumerate() {
        if index % 2 == 0 {
            snapshot.append_color(&tint(colors.quote_background, 0.6), &graphene::Rect::new(band.x as f32, band.y as f32, band.width as f32, band.height as f32));
        }
        put(name, band.x + 6.0, band.y + 6.0, &tint(colors.dim, 1.0));
    }
    // The axis: a line down at each tick, its date under the rows.
    let rows_top = laid.bands.first().map_or(laid.axis - ROW * gantt.tasks.len() as f64 - 4.0, |band| band.y);
    for &(x, time) in &laid.ticks {
        snapshot.append_color(&tint(colors.rule, 0.8), &graphene::Rect::new(x as f32, rows_top as f32, 1.0, (laid.axis - rows_top) as f32));
        let label = text(&strftime(&gantt.axis, time));
        put(&label, x - size(&label).0 / 2.0, laid.axis + 4.0, &tint(colors.dim, 1.0));
    }
    // Tasks: a bar coloured by its kind, its name in it when it fits, else after it; a milestone
    // a diamond.
    for ((task, bar), name) in gantt.tasks.iter().zip(&laid.bars).zip(&names) {
        let rect = graphene::Rect::new(bar.x as f32, bar.y as f32, bar.width as f32, bar.height as f32);
        let color = if task.crit {
            colors.removed
        } else if task.active {
            colors.accent
        } else if task.done {
            colors.dim
        } else {
            colors.link
        };
        let (width, height) = size(name);
        if task.milestone {
            let path = gsk::PathBuilder::new();
            let (cx, cy, r) = (rect.x() + rect.width() / 2.0, rect.y() + rect.height() / 2.0, rect.width() / 2.0);
            path.move_to(cx, cy - r);
            path.line_to(cx + r, cy);
            path.line_to(cx, cy + r);
            path.line_to(cx - r, cy);
            path.close();
            snapshot.append_fill(&path.to_path(), gsk::FillRule::Winding, &tint(color, 1.0));
            put(name, bar.x + bar.width + 6.0, bar.y + (bar.height - height) / 2.0, &tint(colors.text, 1.0));
            continue;
        }
        let rounded = gsk::RoundedRect::from_rect(rect, 4.0);
        snapshot.push_rounded_clip(&rounded);
        snapshot.append_color(&tint(color, if task.done { 0.35 } else { 0.55 }), &rect);
        snapshot.pop();
        snapshot.append_border(&rounded, &[1.0; 4], &[tint(color, 1.0); 4]);
        if width + 12.0 <= bar.width {
            put(name, bar.x + (bar.width - width) / 2.0, bar.y + (bar.height - height) / 2.0, &tint(colors.text, 1.0));
        } else {
            put(name, bar.x + bar.width + 6.0, bar.y + (bar.height - height) / 2.0, &tint(colors.text, 1.0));
        }
    }

    // Names after their bars may go past the right: the picture grows to hold them.
    let right = gantt
        .tasks
        .iter()
        .zip(&laid.bars)
        .zip(&names)
        .map(|((_, bar), name)| bar.x + bar.width + 6.0 + size(name).0 + MARGIN)
        .fold(laid.width, f64::max);
    let picture = match snapshot.to_paintable(Some(&graphene::Size::new(right as f32, laid.height as f32))) {
        Some(paintable) => gtk::Picture::for_paintable(&paintable),
        None => gtk::Picture::new(),
    };
    picture.set_can_shrink(true);
    let width = right.min(WIDEST);
    picture.set_size_request(width as i32, (laid.height * width / right.max(1.0)) as i32);
    picture.set_alternative_text(Some("Gantt chart"));
    picture
}

/// `time` (seconds since 1970, UTC) as `format` (strftime's: `%Y`, `%m`, `%d`, `%b`, `%a`,
/// `%H`, `%M`…) writes it.
fn strftime(format: &str, time: f64) -> String {
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    let days = (time / DAY).floor() as i64;
    let (year, month, day) = civil_from_days(days);
    let seconds = (time - days as f64 * DAY) as i64;
    let weekday = (days + 4).rem_euclid(7) as usize;
    let mut out = String::new();
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&year.to_string()),
            Some('y') => out.push_str(&format!("{:02}", year.rem_euclid(100))),
            Some('m') => out.push_str(&format!("{month:02}")),
            Some('d') => out.push_str(&format!("{day:02}")),
            Some('e') => out.push_str(&day.to_string()),
            Some('b') => out.push_str(&MONTHS[month as usize - 1][..3]),
            Some('B') => out.push_str(MONTHS[month as usize - 1]),
            Some('a') => out.push_str(&DAYS[weekday][..3]),
            Some('A') => out.push_str(DAYS[weekday]),
            Some('H') => out.push_str(&format!("{:02}", seconds / 3600)),
            Some('M') => out.push_str(&format!("{:02}", seconds % 3600 / 60)),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// The first of the month a month (or a quarter, or a year) of `step` after `time`'s.
fn next_month(time: f64, step: f64) -> f64 {
    let (year, month, _) = civil_from_days((time / DAY).floor() as i64);
    let months = (step / (30.0 * DAY)).round().max(1.0) as u32;
    let month = month - 1 + months;
    days_from_civil(year + i64::from(month / 12), month % 12 + 1, 1) as f64 * DAY
}

/// The date of a day counted from 1970-01-01: (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let of_era = z - era * 146_097;
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = (of_year - (153 * shifted + 2) / 5 + 1) as u32;
    let month = if shifted < 10 { shifted + 3 } else { shifted - 9 } as u32;
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

/// Days since 1970-01-01 of a date (proleptic Gregorian).
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let month = i64::from(month);
    let of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let of_era_days = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_era_days - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: f64 = 86_400.0;

    fn date(year: i64, month: u32, day: u32) -> f64 {
        days_from_civil(year, month, day) as f64 * DAY
    }

    /// (name, section, start, end) of each task.
    fn spans(gantt: &Gantt) -> Vec<(&str, usize, f64, f64)> {
        gantt.tasks.iter().map(|task| (task.name.as_str(), task.section, task.start, task.end)).collect()
    }

    #[test]
    fn days_count_from_1970() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2024, 3, 1), 19_783);
    }

    #[test]
    fn the_first_line_says_gantt() {
        assert!(parse("graph TD\nA-->B").is_none());
        assert!(parse("gantt").is_some());
    }

    #[test]
    fn tasks_start_at_a_date_after_another_or_after_the_one_before() {
        let source = "gantt\n    title Plans\n    dateFormat YYYY-MM-DD\n    section One\n    A :a1, 2024-01-01, 3d\n    B :after a1, 2d\n    C :2024-01-10, 2024-01-12\n    section Two\n    D :1w";
        let gantt = parse(source).unwrap();
        assert_eq!(gantt.title, "Plans");
        assert_eq!(gantt.sections, ["One", "Two"]);
        assert_eq!(
            spans(&gantt),
            vec![
                ("A", 0, date(2024, 1, 1), date(2024, 1, 4)),
                ("B", 0, date(2024, 1, 4), date(2024, 1, 6)),
                ("C", 0, date(2024, 1, 10), date(2024, 1, 12)),
                ("D", 1, date(2024, 1, 12), date(2024, 1, 19)),
            ]
        );
    }

    #[test]
    fn kinds_milestones_hours_and_until() {
        let source = "gantt\nDone :done, d1, 2024-01-01, 2d\nNow :active, crit, 2024-01-03, 12h\nShip :milestone, m1, 2024-01-05, 0d\nBefore :2024-01-01, until m1";
        let gantt = parse(source).unwrap();
        let kinds: Vec<(bool, bool, bool, bool)> = gantt.tasks.iter().map(|t| (t.done, t.active, t.crit, t.milestone)).collect();
        assert_eq!(kinds, vec![(true, false, false, false), (false, true, true, false), (false, false, false, true), (false, false, false, false)]);
        assert_eq!(gantt.tasks[1].end, date(2024, 1, 3) + 12.0 * 3600.0);
        assert_eq!((gantt.tasks[2].start, gantt.tasks[2].end), (date(2024, 1, 5), date(2024, 1, 5)));
        assert_eq!((gantt.tasks[3].start, gantt.tasks[3].end), (date(2024, 1, 1), date(2024, 1, 5)));
    }

    #[test]
    fn a_task_may_name_one_that_comes_after_it() {
        let gantt = parse("gantt\nEarly :2024-01-01, until late\nAfter :after late, 1d\nLate :late, 2024-01-04, 2d").unwrap();
        assert_eq!(spans(&gantt)[..2], [("Early", 0, date(2024, 1, 1), date(2024, 1, 4)), ("After", 0, date(2024, 1, 6), date(2024, 1, 7))]);
    }

    #[test]
    fn weekends_are_skipped_when_excluded() {
        // A Friday and two days: Friday, then Monday.
        let gantt = parse("gantt\nexcludes weekends\nA :2024-01-05, 2d").unwrap();
        assert_eq!((gantt.tasks[0].start, gantt.tasks[0].end), (date(2024, 1, 5), date(2024, 1, 9)));
    }

    #[test]
    fn bars_follow_time_across_and_tasks_down() {
        let gantt = parse("gantt\nsection S\nA :2024-01-01, 2d\nB :2024-01-03, 4d").unwrap();
        let laid = layout(&gantt, &[(30.0, 16.0)], (0.0, 0.0), 600.0);
        let (a, b) = (laid.bars[0], laid.bars[1]);
        // B starts as A ends, lasts twice as long, and is a row lower.
        assert!((a.x + a.width - b.x).abs() < 0.5);
        assert!((b.width - 2.0 * a.width).abs() < 0.5);
        assert!(a.y + a.height <= b.y);
        // The axis's ticks go along the time, under the bars.
        assert!(laid.ticks.len() >= 2 && laid.ticks.windows(2).all(|pair| pair[0].0 < pair[1].0 && pair[0].1 < pair[1].1));
        assert!(laid.axis >= b.y + b.height);
    }
}
