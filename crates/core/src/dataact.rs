//! Miele's Data Act API: run history and water/energy use. Spec: <https://developer.miele.com/docs/data-act-api/swagger.json>
//! Kept apart from the 3rd Party API in `lib.rs`: other base URL, other token, other types.
use crate::Result;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::fmt;

const API: &str = "https://api.mcs3.miele.com/dataact";

/// Miele answered with an HTTP error status (403: no Data Act access yet; 400: feature switched off).
#[derive(Debug)]
pub struct Refused(pub u16);

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Miele refused the request (HTTP {})", self.0)
    }
}

impl std::error::Error for Refused {}

/// `GET {API}{path}`. The API rejects requests without a `User-Agent`.
pub fn get<T: DeserializeOwned>(token: &str, path: &str) -> Result<T> {
    let reply = reqwest::blocking::Client::new()
        .get(format!("{API}{path}"))
        .bearer_auth(token)
        .header(
            "User-Agent",
            concat!("freemiele/", env!("CARGO_PKG_VERSION")),
        )
        .send()?;
    match reply.status() {
        s if s.is_success() => Ok(reply.json()?),
        s => Err(Refused(s.as_u16()).into()),
    }
}

// Every field is optional or defaulted: the spec allows nulls and Miele adds fields over time.

/// One entry of `GET /Appliances/Wifi`.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Appliance {
    pub id: String,
    pub ident: Ident,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Ident {
    pub appliance_name: Option<String>,
    pub localized_type: Option<String>,
}

/// The user's name for the appliance, or its type, e.g. "Washing machine".
pub fn appliance_name(a: &Appliance) -> String {
    [&a.ident.appliance_name, &a.ident.localized_type]
        .into_iter()
        .flatten()
        .find(|n| !n.is_empty())
        .cloned()
        .unwrap_or_else(|| a.id.clone())
}

/// A measured value such as water or energy. `value` is in Miele's raw unit; `data_format` says which.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Reading {
    pub data_format: Option<String>,
    pub value: Option<i64>,
}

/// One entry of `GET /Appliances/{id}/AdditionalFeatures/LatestOperationRuns`.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Run {
    pub operation_name: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub consumption_statistics: Option<Vec<Reading>>,
}

/// `GET /Appliances/{id}/AdditionalFeatures/ConsumptionStatistics`.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Consumption {
    pub weeks: Option<Vec<Period>>,
    pub months: Option<Vec<Period>>,
    pub years: Option<Vec<Period>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Period {
    pub from: Option<String>,
    pub overall: Overall,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Overall {
    pub number_of_runs: Runs,
    pub consumption: Option<Vec<Reading>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Runs {
    pub total: Option<i64>,
}

// Dates. The API sends ISO-8601 UTC; std has no time zones, so callers pass the UTC offset in seconds.

pub(crate) const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Seconds since 1970 for e.g. "2026-10-11T20:08:00Z" (also "+02:00" zones; fractions ignored).
/// Missing parts default as in the spec: month and day 1, time 00:00:00, zone UTC.
pub(crate) fn epoch(s: &str) -> Option<i64> {
    let (date, time) = s.split_once('T').unwrap_or((s, ""));
    let (clock, zone) = match time.find(['Z', '+', '-']) {
        Some(i) => time.split_at(i),
        None => (time, "Z"),
    };
    let zone = match zone {
        "Z" => 0,
        z => {
            let (h, m) = z[1..].split_once(':')?;
            let secs = h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60;
            if z.starts_with('-') { -secs } else { secs }
        }
    };
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (
        d.next()??,
        d.next().flatten().unwrap_or(1),
        d.next().flatten().unwrap_or(1),
    );
    let mut t = clock
        .split(':')
        .map(|p| p.split('.').next()?.parse::<i64>().ok());
    let mut part = || t.next().flatten().unwrap_or(0);
    let (h, mi, sec) = (part(), part(), part());
    // Days since 1970 (Howard Hinnant's days_from_civil).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + sec - zone)
}

/// (year, month 1-12, day, hour, minute) of an ISO-8601 string at the given UTC offset.
pub(crate) fn local(s: &str, offset: i64) -> Option<(i64, usize, i64, i64, i64)> {
    let t = epoch(s)? + offset;
    let (days, secs) = (t.div_euclid(86400), t.rem_euclid(86400));
    // Civil date from days since 1970 (Howard Hinnant's civil_from_days).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Some((year, month as usize, day, secs / 3600, secs % 3600 / 60))
}

/// "48 l · 0.9 kWh": the water and energy readings, whichever are present.
fn figures(readings: &Option<Vec<Reading>>) -> String {
    let mut out = Vec::new();
    for r in readings.iter().flatten() {
        let Some(v) = r.value else { continue };
        match r.data_format.as_deref() {
            Some("MILLILITER_AS_LITER") => out.push(format!("{:.0} l", v as f64 / 1000.0)),
            Some("WATTHOURS_AS_KILOWATTHOURS") => out.push(format!("{:.1} kWh", v as f64 / 1000.0)),
            _ => {}
        }
    }
    out.join(" · ")
}

/// "11 Oct 22:08–23:21  Sportswear  1h13  48 l · 0.9 kWh"; parts Miele left out are skipped.
pub fn run_line(r: &Run, offset: i64) -> String {
    let at = |s: &Option<String>| s.as_deref().and_then(|s| local(s, offset));
    let mut parts = Vec::new();
    if let Some((_, mo, d, h, mi)) = at(&r.start_at) {
        let mut when = format!("{d} {} {h:02}:{mi:02}", MONTHS[mo - 1]);
        if let Some((_, _, _, h, mi)) = at(&r.end_at) {
            when += &format!("–{h:02}:{mi:02}");
        }
        parts.push(when);
    }
    parts.extend(r.operation_name.clone().filter(|n| !n.is_empty()));
    if let Some(s) = r.duration_seconds {
        let mins = (s + 30) / 60;
        parts.push(match mins {
            0..60 => format!("{mins} min"),
            _ => format!("{}h{:02}", mins / 60, mins % 60),
        });
    }
    parts.push(figures(&r.consumption_statistics));
    parts.retain(|p| !p.is_empty());
    parts.join("  ")
}

/// A small table per period kind: "Weeks", then one line per week with runs, water and energy.
pub fn usage_table(c: &Consumption, offset: i64) -> String {
    let mut out = String::new();
    for (title, periods) in [
        ("Weeks", &c.weeks),
        ("Months", &c.months),
        ("Years", &c.years),
    ] {
        let Some(periods) = periods.as_ref().filter(|p| !p.is_empty()) else {
            continue;
        };
        out += &format!("  {title}\n");
        for p in periods {
            let label = match p.from.as_deref().and_then(|s| local(s, offset)) {
                Some((y, mo, d, ..)) => match title {
                    "Weeks" => format!("{d} {}", MONTHS[mo - 1]),
                    "Months" => format!("{} {y}", MONTHS[mo - 1]),
                    _ => y.to_string(),
                },
                None => "?".into(),
            };
            let runs = p.overall.number_of_runs.total.unwrap_or(0);
            let line = format!(
                "    {label:<9} {runs:>3} runs  {}",
                figures(&p.overall.consumption)
            );
            out += line.trim_end();
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CEST: i64 = 2 * 3600;

    #[test]
    fn runs_show_local_time_and_skip_missing_figures() {
        let runs: Vec<Run> = serde_json::from_str(include_str!(
            "../../../fixtures/dataact-latest-runs.sample.json"
        ))
        .unwrap();
        let lines: Vec<_> = runs.iter().map(|r| run_line(r, CEST)).collect();
        assert_eq!(
            lines[0],
            "11 Oct 22:08–23:20  Sportswear  1h12  48 l · 0.9 kWh"
        );
        assert_eq!(lines[1], "9 Oct 09:30–11:05  Cottons  1h35  1.2 kWh");
        assert_eq!(lines[2], "7 Oct 20:00  Quick wash");
    }

    #[test]
    fn usage_is_a_table_per_period() {
        let c: Consumption = serde_json::from_str(include_str!(
            "../../../fixtures/dataact-consumption.sample.json"
        ))
        .unwrap();
        assert_eq!(
            usage_table(&c, CEST),
            "  Weeks\n    28 Sep      4 runs  190 l · 3.8 kWh\n    5 Oct       3 runs  142 l · 2.8 kWh\n  \
             Months\n    Sep 2026   14 runs  640 l · 11.2 kWh\n    Oct 2026    7 runs\n"
        );
    }

    #[test]
    fn appliances_parse_and_unknown_or_missing_fields_are_fine() {
        let a: Vec<Appliance> = serde_json::from_str(include_str!(
            "../../../fixtures/dataact-appliances.sample.json"
        ))
        .unwrap();
        assert_eq!(appliance_name(&a[0]), "WWB000 Washer");
        assert_eq!(appliance_name(&a[1]), "Tumble dryer");
        let empty: Consumption = serde_json::from_str(r#"{"surprise": 1, "weeks": null}"#).unwrap();
        assert_eq!(usage_table(&empty, 0), "");
    }

    #[test]
    fn dates_with_offsets_and_missing_parts() {
        assert_eq!(epoch("1970-01-02T00:00:00Z"), Some(86400));
        assert_eq!(
            epoch("2026-10-11T22:08:00+02:00"),
            epoch("2026-10-11T20:08:00.5Z")
        );
        assert_eq!(epoch("2018-08"), epoch("2018-08-01T00:00:00Z"));
        assert_eq!(epoch("nonsense"), None);
    }
}
