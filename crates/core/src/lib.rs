pub mod dataact;
mod names;

use reqwest::Url;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::io::{BufRead, BufReader, Write};
use std::mem::take;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const API: &str = "https://api.mcs3.miele.com/v1";
const AUTH: &str = "https://auth.domestic.miele-iot.com/partner/realms/mcs/protocol/openid-connect";
pub const REDIRECT: &str = "http://localhost:8765/";

#[derive(Deserialize)]
pub struct Device {
    pub ident: Ident,
    pub state: State,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ident {
    #[serde(rename = "type")]
    pub kind: Value,
    pub device_name: String,
    pub device_ident_label: Label,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub tech_type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub status: Value,
    // [hours, minutes]
    pub remaining_time: [u32; 2],
    // [hours, minutes]; empty or missing when the appliance is not running
    #[serde(default)]
    pub elapsed_time: Vec<u32>,
    // [hours, minutes] until a delayed start; [0, 0], empty or missing = none
    #[serde(default)]
    pub start_time: Vec<u32>,
    pub signal_info: bool,
    pub signal_failure: bool,
    // The door is open; missing on appliances without a door sensor.
    #[serde(default)]
    pub signal_door: bool,
    pub remote_enable: RemoteEnable,
    #[serde(rename = "ProgramID", default)]
    pub program_id: Reading,
    #[serde(default)]
    pub program_phase: Reading,
    // e.g. 4 = "Cleaning/Care programme"; used when a programme has no name.
    #[serde(default)]
    pub program_type: Reading,
    // Washers and dryers; value_raw is null when the programme has no spin speed.
    #[serde(default)]
    pub spinning_speed: Reading,
    #[serde(default)]
    pub target_temperature: Vec<Temperature>,
}

/// A temperature setting. `value_raw` is hundredths of °C (4000 = 40 °C); -32768 = not used.
/// value_localized is a number here, not text, so it is not read.
#[derive(Deserialize)]
pub struct Temperature {
    pub value_raw: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEnable {
    pub full_remote_control: bool,
    // Must be switched on at the machine before a drink can start remotely.
    pub mobile_start: bool,
}

#[derive(Deserialize)]
pub struct Value {
    pub value_raw: i64,
    pub value_localized: String,
}

/// Like `Value`, but the API may leave both fields out or empty (coffee machine programs and phases).
#[derive(Deserialize, Default)]
pub struct Reading {
    pub value_raw: Option<i64>,
    pub value_localized: Option<String>,
}

/// Device id (fabrication number) -> device, as returned by `GET /devices`.
pub type Devices = HashMap<String, Device>;

/// What an appliance allows right now, from `GET /devices/{id}/actions`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actions {
    pub power_on: bool,
    pub power_off: bool,
}

/// A drink or other program, from `GET /devices/{id}/programs`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Program {
    pub program_id: u32,
    pub program: String,
}

pub fn get<T: DeserializeOwned>(token: &str, path: &str) -> Result<T> {
    Ok(reqwest::blocking::Client::new()
        .get(format!("{API}{path}"))
        .bearer_auth(token)
        .send()?
        .error_for_status()?
        .json()?)
}

/// Miele answered a command with an HTTP error status. Shown as "Miele refused (500 …): <Miele's message>".
#[derive(Debug)]
pub struct Rejected(pub reqwest::StatusCode, pub String);

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Miele refused ({}): {}", self.0, self.1)
    }
}

impl std::error::Error for Rejected {}

/// A command that Miele may answer with a false server error: see `confirmed`.
#[derive(Clone, Copy)]
pub enum Request {
    On,
    Off,
    Brew,
}

/// Did the machine do what was asked? On: not Off (1) and not Not connected (255).
/// Off: Off (1). Brew: In use (5).
pub fn confirmed(request: Request, state_after: &State) -> bool {
    let status = state_after.status.value_raw;
    match request {
        Request::On => ![1, 255].contains(&status),
        Request::Off => status == 1,
        Request::Brew => status == 5,
    }
}

pub fn put(token: &str, path: &str, body: serde_json::Value) -> Result<()> {
    let reply = reqwest::blocking::Client::new()
        .put(format!("{API}{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()?;
    let status = reply.status();
    if !status.is_success() {
        // Miele explains refusals in the body, e.g. why a program can't start now.
        return Err(Rejected(status, reply.text()?).into());
    }
    Ok(())
}

/// Read-only copy of every appliance's raw replies, safe to share: see `strip_identifiers`.
pub fn dump(token: &str) -> Result<serde_json::Value> {
    use serde_json::{Value, json};
    // A failed part is recorded, not fatal. The reqwest error would carry the URL (and the id).
    let part = |id: &str, what: &str| -> Value {
        get(token, &format!("/devices/{id}{what}?language=en")).unwrap_or_else(|e| {
            let msg = match e.downcast::<reqwest::Error>() {
                Ok(e) => e.without_url().to_string(),
                Err(e) => e.to_string(),
            };
            json!({ "error": msg })
        })
    };
    let devices: HashMap<String, Value> = get(token, "/devices?language=en")?;
    // Sorted by model so appliance-1, appliance-2… are the same on every run.
    let mut devices: Vec<_> = devices.iter().collect();
    devices.sort_by_key(|(_, d)| d["ident"]["deviceIdentLabel"]["techType"].to_string());
    let appliances: Vec<Value> = devices
        .into_iter()
        .enumerate()
        .map(|(i, (id, device))| {
            json!({
                "id": format!("appliance-{}", i + 1),
                "type": device["ident"]["type"]["value_localized"],
                "model": device["ident"]["deviceIdentLabel"]["techType"],
                "device": part(id, ""),
                "actions": part(id, "/actions"),
                "programs": part(id, "/programs"),
                "fillingLevels": part(id, "/fillingLevels"),
                "lastUsedPrograms": part(id, "/lastUsedPrograms"),
                "failureDetails": part(id, "/failureDetails"),
            })
        })
        .collect();
    let mut out = json!({ "freemiele": env!("CARGO_PKG_VERSION"), "appliances": appliances });
    strip_identifiers(&mut out);
    Ok(out)
}

/// Remove what identifies a unit or a person: serial, index and material numbers, software ids,
/// the user-set name, and any run of 10+ digits inside text (ids in error messages).
pub fn strip_identifiers(v: &mut serde_json::Value) {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            map.retain(|k, _| {
                !["fabNumber", "fabIndex", "matNumber", "swids"].contains(&k.as_str())
            });
            for (k, child) in map.iter_mut() {
                match child {
                    Value::String(_) if k == "deviceName" => *child = "".into(),
                    _ => strip_identifiers(child),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip_identifiers),
        Value::String(s) => *s = redact_long_numbers(s),
        _ => {}
    }
}

/// Replace each run of 10+ digits with "[removed]", keeping the rest of the text readable.
fn redact_long_numbers(s: &str) -> String {
    let (mut out, mut run) = (String::new(), String::new());
    for c in s.chars().chain(std::iter::once('\0')) {
        if c.is_ascii_digit() {
            run.push(c);
            continue;
        }
        out += if run.len() >= 10 { "[removed]" } else { &run };
        run.clear();
        if c != '\0' {
            out.push(c);
        }
    }
    out
}

/// How a power or brew command ended, judged by what the machine reports, not by Miele's reply.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Done,
    /// Miele answered 5xx, but the machine did it.
    DoneDespiteServerError,
    /// Miele answered, but the machine never showed the change (e.g. empty water tank).
    NotConfirmed,
}

/// Wait for the machine to show the change: `states` are readings taken in turn, newest last.
fn decide(
    request: Request,
    server_error: bool,
    states: impl IntoIterator<Item = State>,
) -> Outcome {
    match (
        states.into_iter().any(|s| confirmed(request, &s)),
        server_error,
    ) {
        (true, false) => Outcome::Done,
        (true, true) => Outcome::DoneDespiteServerError,
        (false, _) => Outcome::NotConfirmed,
    }
}

// Miele's state lags a command, so re-read every 2 s.
const POLL_EVERY: Duration = Duration::from_secs(2);
// Drinks start slowly (heating, grinding); power changes show sooner.
const BREW_WAIT: Duration = Duration::from_secs(15);
const POWER_WAIT: Duration = Duration::from_secs(10);

/// Sends a command, then polls the machine until it shows the change or `wait` runs out.
/// Miele may answer 2xx for a command that did nothing, or 5xx for one that worked.
/// A 4xx is an immediate refusal.
// lean: polling; the live stream (`Sse`) could confirm instead if the polling load matters.
fn command(
    token: &str,
    id: &str,
    what: &str,
    body: serde_json::Value,
    request: Request,
    wait: Duration,
) -> Result<Outcome> {
    let path = format!("/devices/{id}");
    let server_error = match put(token, &format!("{path}/{what}"), body) {
        Ok(()) => false,
        Err(e) if matches!(e.downcast_ref::<Rejected>(), Some(r) if r.0.is_server_error()) => true,
        Err(e) => return Err(e),
    };
    let readings = (0..wait.as_secs() / POLL_EVERY.as_secs()).filter_map(|_| {
        std::thread::sleep(POLL_EVERY);
        get::<Device>(token, &path).ok().map(|d| d.state)
    });
    Ok(decide(request, server_error, readings))
}

/// Turn a device on or off.
pub fn power(token: &str, id: &str, on: bool) -> Result<Outcome> {
    let (key, request) = if on {
        ("powerOn", Request::On)
    } else {
        ("powerOff", Request::Off)
    };
    command(
        token,
        id,
        "actions",
        serde_json::json!({ key: true }),
        request,
        POWER_WAIT,
    )
}

/// Start a drink.
pub fn brew(token: &str, id: &str, program_id: u32) -> Result<Outcome> {
    let body = serde_json::json!({ "programId": program_id });
    command(token, id, "programs", body, Request::Brew, BREW_WAIT)
}

/// Case-insensitive match on the program name, e.g. "flat white" -> "Flat white".
/// Never matches an empty name: the API lists a nameless program that must not start by accident.
pub fn find_program<'a>(programs: &'a [Program], name: &str) -> Option<&'a Program> {
    let name = name.trim().to_lowercase();
    programs
        .iter()
        .find(|p| !name.is_empty() && p.program.to_lowercase() == name)
}

/// The user's name for the device, or model + type, e.g. "CM6160 Coffee machine".
pub fn name(d: &Device) -> String {
    match d.ident.device_name.as_str() {
        "" => format!(
            "{} {}",
            d.ident.device_ident_label.tech_type, d.ident.kind.value_localized
        ),
        n => n.to_string(),
    }
}

/// Lower case, no spaces, so "cm 6160" matches "CM6160".
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<String>().to_lowercase()
}

/// Pick the one device to act on from those that allow the action (id, device).
/// With `wanted`, it must equal a device's name or model (ignoring case and spaces).
/// Without it, there must be only one. Otherwise the error lists the choices.
pub fn choose<'a>(
    candidates: &[(&'a str, &'a Device)],
    wanted: Option<&str>,
) -> Result<(&'a str, &'a Device)> {
    let list = |c: &[(&str, &Device)]| {
        c.iter()
            .map(|(_, d)| name(d))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let matches: Vec<_> = match wanted {
        None => candidates.to_vec(),
        Some(w) => {
            let w = squash(w);
            let hit = |(_, d): &&(&str, &Device)| {
                squash(&name(d)) == w || squash(&d.ident.device_ident_label.tech_type) == w
            };
            candidates.iter().filter(hit).copied().collect()
        }
    };
    match (&matches[..], wanted) {
        ([one], _) => Ok(*one),
        ([], _) => Err(format!(
            "No matching appliance can do that right now. Available: {}.",
            list(candidates)
        )
        .into()),
        (many, Some(_)) => Err(format!(
            "Several appliances match: {}. Be more specific.",
            list(many)
        )
        .into()),
        (many, None) => Err(format!(
            "Several appliances can do that: {}. Choose one with --device.",
            list(many)
        )
        .into()),
    }
}

/// Programme code a coffee machine reports while its display asks for care, e.g. "empty the drip
/// tray" (Home Assistant: `check_appliance`). Seen on a CM 6160 with `signalInfo` still false.
const CHECK_APPLIANCE: i64 = 17004;

/// The appliance's display is asking for something: the official info flag, or a coffee machine
/// sitting on its "check appliance" code.
pub fn needs_attention(d: &Device) -> bool {
    d.state.signal_info
        || (d.ident.kind.value_raw == COFFEE_MACHINE
            && d.state.program_id.value_raw == Some(CHECK_APPLIANCE))
}

/// The programme and phase of an appliance at work, e.g. ("Sportswear", "Main wash").
/// Each is Miele's own text, else our name for the code (with the code, unless confirmed), else
/// for a programme the programme type text, else the raw code: unknown codes stay visible.
/// None when it is off, not connected or the code means "not running". No phase once finished.
pub fn activity(d: &Device) -> (Option<String>, Option<String>) {
    let status = d.state.status.value_raw;
    let kind = d.ident.kind.value_raw;
    // 1 = Off (0 is unused but treated the same), 255 = Not connected
    if [0, 1, 255].contains(&status) {
        return (None, None);
    }
    // Codes mean nothing to people, so a name we don't have is left out, never shown as a number.
    let show = |r: &Reading, ours: fn(i64, i64) -> Option<&'static str>| {
        if let Some(t) = r.value_localized.as_deref().filter(|t| !t.is_empty()) {
            return Some(t.to_string());
        }
        let code = r.value_raw?;
        names::confirmed_name(kind, code)
            .or_else(|| ours(kind, code))
            .filter(|n| !n.is_empty())
            .map(String::from)
    };
    let program_type = d
        .state
        .program_type
        .value_localized
        .clone()
        .filter(|t| !t.is_empty());
    let phase = (status != END)
        .then(|| show(&d.state.program_phase, names::phase_name))
        .flatten();
    let program = show(&d.state.program_id, names::program_name).or(program_type);
    (program, phase)
}

/// The programme settings of a washer or dryer at work, e.g. "40 °C · 1200 rpm".
/// Parts the machine leaves out (missing, null, 0 or -32768) are skipped; None if none is left.
pub fn details(d: &Device) -> Option<String> {
    if !WASHERS_AND_DRYERS.contains(&d.ident.kind.value_raw)
        || [0, 1, 255].contains(&d.state.status.value_raw)
    {
        return None;
    }
    let temp = d
        .state
        .target_temperature
        .first()
        .map(|t| t.value_raw / 100);
    let parts: Vec<_> = [
        temp.filter(|&t| t > 0).map(|t| format!("{t} °C")),
        d.state
            .spinning_speed
            .value_raw
            .filter(|&r| r > 0)
            .map(|r| format!("{r} rpm")),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// One line per device, e.g. "CM6160 Coffee Machine: Off (mobile start off) — needs attention".
/// An appliance at work adds its program and phase: "In use, coffee · Grinding".
/// A washer or dryer that has finished says "Finished, Sportswear", with no phase or time left.
pub fn status_line(d: &Device) -> String {
    let mut line = format!("{}: {}", name(d), d.state.status.value_localized);
    let status = d.state.status.value_raw;
    let (program, phase) = activity(d);
    let shown: Vec<_> = [program, phase].into_iter().flatten().collect();
    if !shown.is_empty() {
        line += &format!(", {}", shown.join(" · "));
        if let Some(details) = details(d) {
            line += &format!(" ({details})");
        }
    }
    if status == WAITING_TO_START {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |t| t.as_secs() as i64);
        if let Some(at) = starts_at(&d.state.start_time, now + utc_offset()) {
            line += &format!(", starts at {at}");
        }
    }
    let [h, m] = d.state.remaining_time;
    if h + m > 0 && status != END {
        line += &format!(", {h}h{m:02} left");
    }
    let remote = &d.state.remote_enable;
    if remote.full_remote_control && !remote.mobile_start {
        line += " (mobile start off)";
    }
    if d.state.signal_failure {
        line += " — FAULT";
    } else if needs_attention(d) {
        line += " — needs attention: check the machine's display";
    }
    line
}

/// "14:30": the local clock time `start` ([hours, minutes]) after `now` (local seconds since 1970).
/// None when no delayed start is set.
fn starts_at(start: &[u32], now: i64) -> Option<String> {
    let &[h, m] = start else { return None };
    let mins = (now / 60 + i64::from(h * 60 + m)).rem_euclid(24 * 60);
    (h + m > 0).then(|| format!("{:02}:{:02}", mins / 60, mins % 60))
}

/// Seconds east of UTC now, from the system clock's zone.
/// lean: one offset for all dates, so runs across a daylight-saving change are off by an hour.
pub fn utc_offset() -> i64 {
    // std has no time zones, so ask the system: `date` on macOS and Linux, PowerShell on Windows.
    #[cfg(windows)]
    let (cmd, args) = (
        "powershell",
        ["-NoProfile", "-Command", "[DateTime]::Now.ToString('zzz')"].as_slice(),
    );
    #[cfg(not(windows))]
    let (cmd, args) = ("date", ["+%z"].as_slice());
    let out = std::process::Command::new(cmd).args(args).output();
    out.map_or(0, |o| offset_secs(&String::from_utf8_lossy(&o.stdout)))
}

/// "+0200" (`date`) or "-03:30" (PowerShell) in seconds east of UTC; 0 if it is neither.
fn offset_secs(z: &str) -> i64 {
    let z = z.trim().replace(':', "");
    let (Some(sign @ ('+' | '-')), true) = (z.chars().next(), z.len() == 5) else {
        return 0;
    };
    let secs = z[1..3].parse::<i64>().unwrap_or(0) * 3600 + z[3..].parse::<i64>().unwrap_or(0) * 60;
    if sign == '-' { -secs } else { secs }
}

/// Builds Server-Sent Events messages from lines. A blank line ends a message: (event, data).
#[derive(Default)]
pub struct Sse {
    event: String,
    data: String,
}

impl Sse {
    pub fn line(&mut self, line: &str) -> Option<(String, String)> {
        if line.is_empty() {
            let done = !self.event.is_empty() || !self.data.is_empty();
            return done.then(|| (take(&mut self.event), take(&mut self.data)));
        }
        if let Some(v) = line.strip_prefix("event:") {
            self.event = v.trim_start().into();
        } else if let Some(v) = line.strip_prefix("data:") {
            if !self.data.is_empty() {
                self.data.push('\n');
            }
            self.data += v.trim_start();
        }
        None // comments (":") and other fields are ignored
    }
}

/// Lines to print for devices whose status line differs from `seen` (which it updates).
/// The first call prints every device once.
pub fn changes(seen: &mut HashMap<String, String>, devices: &Devices) -> Vec<String> {
    let mut ids: Vec<_> = devices.keys().collect();
    ids.sort();
    ids.into_iter()
        .filter_map(|id| {
            let line = status_line(&devices[id]);
            (seen.insert(id.clone(), line.clone()) != Some(line.clone())).then_some(line)
        })
        .collect()
}

/// No event, not even a ping, for this long means the connection is dead.
const SILENCE: Duration = Duration::from_secs(60);

/// One connection to `GET /devices/all/events`. Always ends in an error: it dropped or went silent.
fn stream(
    token: &str,
    seen: &mut HashMap<String, String>,
    wait: &mut u64,
    show: &mut impl FnMut(&Devices, Vec<String>),
) -> String {
    let user_agent = concat!("freeMiele/", env!("CARGO_PKG_VERSION"));
    let connect = || -> Result<_> {
        // No overall timeout: it would cut a long stream.
        Ok(reqwest::blocking::Client::builder()
            .timeout(None)
            .user_agent(user_agent)
            .build()?
            .get(format!("{API}/devices/all/events"))
            .bearer_auth(token)
            .header("Accept", "text/event-stream")
            .send()?
            .error_for_status()?)
    };
    let reply = match connect() {
        Ok(r) => r,
        Err(e) => return e.to_string(),
    };
    // blocking reqwest has no read timeout, so a thread reads and this side waits with a limit.
    // lean: after a silent connection its thread lingers until the socket dies; fine for a CLI.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(reply).lines() {
            if tx.send(line.map_err(|e| e.to_string())).is_err() {
                return;
            }
        }
    });
    let mut sse = Sse::default();
    loop {
        let line = match rx.recv_timeout(SILENCE) {
            Ok(Ok(line)) => line,
            Ok(Err(e)) => return e,
            Err(RecvTimeoutError::Timeout) => return "nothing heard for 60 s".into(),
            Err(RecvTimeoutError::Disconnected) => return "connection closed".into(),
        };
        let Some((event, data)) = sse.line(&line) else {
            continue;
        };
        *wait = 2; // the connection works: the next drop starts the backoff again
        if event == "devices" {
            match serde_json::from_str::<Devices>(&data) {
                Ok(devices) => {
                    let lines = changes(seen, &devices);
                    show(&devices, lines);
                }
                Err(e) => eprintln!("Could not read an update ({e})."),
            }
        }
    }
}

/// Live status: calls `show` with all devices and the lines to print, on connect and on every update.
/// Reconnects with a fresh token, waiting 2, 4, 8 ... up to 60 s. Never returns.
pub fn watch(session: &mut Session, mut show: impl FnMut(&Devices, Vec<String>)) -> ! {
    let mut seen = HashMap::new();
    let mut wait = 2;
    loop {
        let why = match session.access_token() {
            Ok(token) => stream(&token, &mut seen, &mut wait, &mut show),
            Err(e) => e.to_string(),
        };
        eprintln!("Connection lost ({why}). Trying again in {wait} s.");
        std::thread::sleep(Duration::from_secs(wait));
        wait = (wait * 2).min(60);
    }
}

/// Coffee machine care counters, from `GET /devices/{id}/fillingLevels` (null = not supported).
/// Each is a lifetime count of completed care programmes (confirmed on a CM 6160; milk by analogy).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillingLevels {
    pub descaling_counter: Option<i64>,
    pub degreasing_counter: Option<i64>,
    pub milk_cleaning_counter: Option<i64>,
    // Percent full (the owner's WTR860: 0 = empty tank).
    pub twin_dos_container1_filling_level: Option<i64>,
    pub twin_dos_container2_filling_level: Option<i64>,
    // Dishwashers, percent full.
    pub salt_filling_level: Option<i64>,
    pub rinse_aid_filling_level: Option<i64>,
    pub power_disc_filling_level: Option<i64>,
}

/// One finished run, from `GET /devices/{id}/lastUsedPrograms` (a JSON array). The consumption
/// fields are always null on our machines, so they are not read.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastRun {
    #[serde(rename = "ProgramID")]
    pub program_id: Reading,
    #[serde(default)]
    pub start_at: Option<String>,
    #[serde(default)]
    pub end_at: Option<String>,
}

/// "Mon 2 Mar 18:00 · Rinse out lint · 8 min": start in local time at `offset` seconds from UTC.
pub fn history_line(r: &LastRun, offset: i64) -> String {
    use dataact::{MONTHS, epoch, local};
    let name = match (&r.program_id.value_localized, r.program_id.value_raw) {
        (Some(t), _) if !t.is_empty() => t.clone(),
        (_, code) => code
            .and_then(|c| names::program_name(WASHER_DRYER, c))
            .filter(|n| !n.is_empty())
            .map_or_else(|| "Unnamed programme".into(), String::from),
    };
    let mut parts = Vec::new();
    if let (Some(start), Some((_, mo, d, h, mi))) = (
        r.start_at.as_deref().and_then(epoch),
        r.start_at.as_deref().and_then(|s| local(s, offset)),
    ) {
        let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"]
            [(start + offset).div_euclid(86400).rem_euclid(7) as usize];
        parts.push(format!("{weekday} {d} {} {h:02}:{mi:02}", MONTHS[mo - 1]));
    }
    parts.push(name);
    let at = |t: &Option<String>| t.as_deref().and_then(epoch);
    if let (Some(a), Some(b)) = (at(&r.start_at), at(&r.end_at))
        && b >= a
    {
        let mins = (b - a + 30) / 60;
        parts.push(match mins {
            0..60 => format!("{mins} min"),
            _ => format!("{} h {:02} min", mins / 60, mins % 60),
        });
    }
    parts.join(" · ")
}

/// `Ident.kind.value_raw` of a coffee machine.
pub const COFFEE_MACHINE: i64 = 17;
const WASHING_MACHINE: i64 = 1;
pub const TUMBLE_DRYER: i64 = 2;
const WASHER_DRYER: i64 = 24;
/// `Ident.kind.value_raw` of a dishwasher.
pub const DISHWASHER: i64 = 7;
/// The types Miele keeps a run history for, and that report spin speed and temperature.
pub const WASHERS_AND_DRYERS: [i64; 3] = [WASHING_MACHINE, TUMBLE_DRYER, WASHER_DRYER];
/// `State.status.value_raw` of a finished program.
pub const END: i64 = 7;
/// `State.status.value_raw` of a programme set to start later.
const WAITING_TO_START: i64 = 4;

/// e.g. "  Care so far: descaled 20× · degreased 13× · milk system cleaned 12×"; None if none is set.
pub fn care_line(f: &FillingLevels) -> Option<String> {
    let parts: Vec<_> = [
        ("descaled", f.descaling_counter),
        ("degreased", f.degreasing_counter),
        ("milk system cleaned", f.milk_cleaning_counter),
    ]
    .into_iter()
    .filter_map(|(label, n)| n.map(|n| format!("{label} {n}×")))
    .collect();
    (!parts.is_empty()).then(|| format!("  Care so far: {}", parts.join(" · ")))
}

/// "TwinDos: 1 empty · 2 28%" for washers with automatic dosing; None without TwinDos.
pub fn twin_dos_line(f: &FillingLevels) -> Option<String> {
    let tanks = [
        f.twin_dos_container1_filling_level,
        f.twin_dos_container2_filling_level,
    ];
    let parts: Vec<_> = (1..)
        .zip(tanks)
        .filter_map(|(i, level)| percent(&i.to_string(), level))
        .collect();
    (!parts.is_empty()).then(|| format!("TwinDos: {}", parts.join(" · ")))
}

/// "Salt 40% · Rinse aid empty · PowerDisc 80%" for dishwashers; None if it reports none.
pub fn dishwasher_line(f: &FillingLevels) -> Option<String> {
    let parts: Vec<_> = [
        ("Salt", f.salt_filling_level),
        ("Rinse aid", f.rinse_aid_filling_level),
        ("PowerDisc", f.power_disc_filling_level),
    ]
    .into_iter()
    .filter_map(|(label, level)| percent(label, level))
    .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// "Salt 40%", or "Salt empty" at 0; None when the level is not reported.
fn percent(label: &str, level: Option<i64>) -> Option<String> {
    Some(match level? {
        0 => format!("{label} empty"),
        n => format!("{label} {n}%"),
    })
}

/// Our own short summary of the owner's manual care table, printed under each coffee machine.
pub const CARE_SCHEDULE: &str =
    "  When the machine asks: descale; degrease the brew unit (1 cleaning tablet)
  Every day: empty the drip tray and waste container, clean the milk flask and siphon
  Every week: rinse the brew unit under running water, clean the central spout
  Every month: clean the bean container and the water container filter
  Due-now reminders only appear on the machine's display.";

/// Login details, kept as one entry in the system password store (the Keychain on a Mac).
#[derive(Serialize, Deserialize)]
pub struct Session {
    client_id: String,
    client_secret: String,
    access_token: String,
    refresh_token: String,
    expires_at: u64,
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn keychain() -> Result<keyring::Entry> {
    Ok(keyring::Entry::new("miele-cli", "session")?)
}

pub const NOT_LOGGED_IN: &str = "Not logged in. Run `miele login` first.";

/// Tell "no saved login" apart from "the password store refused access to it".
fn login_error(e: keyring::Error) -> String {
    match e {
        keyring::Error::NoEntry => NOT_LOGGED_IN.into(),
        e => format!(
            "Your system's password store didn't allow access to the saved login. Run the command again and allow access (on a Mac: click Always Allow). ({e})"
        ),
    }
}

pub fn login_url(client_id: &str, state: &str) -> Url {
    let params = vec![
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT),
        ("state", state),
        // mcs_dataact: read access to the EU Data Act API (Miele, by email, 2026-10-12).
        (
            "scope",
            "openid mcs_thirdparty_read mcs_thirdparty_write mcs_dataact",
        ),
    ];
    Url::parse_with_params(&format!("{AUTH}/auth"), &params).unwrap()
}

/// Pull the login code out of the browser's request line, e.g. "GET /?code=abc&state=xyz HTTP/1.1".
/// Returns None if the state doesn't match (a request we didn't start) or there is no code.
pub fn code_from_request(line: &str, state: &str) -> Option<String> {
    let path = line.split_whitespace().nth(1)?;
    let url = Url::parse(&format!("http://localhost{path}")).ok()?;
    let q: HashMap<_, _> = url.query_pairs().collect();
    if q.get("state")? != state {
        return None;
    }
    q.get("code").map(|c| c.to_string())
}

/// How long to wait for the browser to come back from Miele's sign-in.
const LOGIN_WAIT: Duration = Duration::from_secs(300);

/// Wait on 127.0.0.1:8765 for Miele's sign-in to send the browser back, and return the login code.
/// `open` gets the sign-in address and should show it to the user (open the browser).
/// Ends with an error if the port is busy, `cancel` is set, or nobody signs in within 5 minutes.
pub fn wait_for_login(
    client_id: &str,
    open: impl FnOnce(&Url) -> Result<()>,
    cancel: &AtomicBool,
) -> Result<String> {
    // Random value that ties the browser's answer to this login attempt.
    let state = format!("{:x}", RandomState::new().hash_one(std::process::id()));
    let listener = match TcpListener::bind("127.0.0.1:8765") {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            return Err("Another login is already waiting. Close it and try again.".into());
        }
        Err(e) => return Err(e.into()),
    };
    listener.set_nonblocking(true)?; // so a cancel or the time limit can end the wait
    open(&login_url(client_id.trim(), &state))?;
    let deadline = Instant::now() + LOGIN_WAIT;
    // Wait for the browser to come back with the login code; ignore stray requests (favicon etc).
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("Login cancelled.".into());
        }
        if Instant::now() > deadline {
            return Err("Miele sign-in took too long. Try again.".into());
        }
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?; // a silent stray must not block us
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        if !line.contains("state=") {
            stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")?;
            continue;
        }
        let code = code_from_request(&line, &state);
        let msg = if code.is_some() {
            "Logged in. You can close this tab."
        } else {
            "Login failed. Go back to where you started the login."
        };
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{msg}",
            msg.len()
        )?;
        return code.ok_or_else(|| {
            format!(
                "Miele did not return a login code. Browser said: {}",
                line.trim()
            )
            .into()
        });
    }
}

impl Session {
    /// Trade the code from the browser login for tokens.
    pub fn login(client_id: String, client_secret: String, code: &str) -> Result<Self> {
        let mut s = Session {
            client_id,
            client_secret,
            access_token: String::new(),
            refresh_token: String::new(),
            expires_at: 0,
        };
        s.token_request(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT),
        ])?;
        Ok(s)
    }

    fn token_request(&mut self, form: &[(&str, &str)]) -> Result<()> {
        let url = format!("{AUTH}/token");
        let mut form = form.to_vec();
        form.extend([
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
        ]);
        let reply: TokenReply = reqwest::blocking::Client::new()
            .post(url)
            .form(&form)
            .send()?
            .error_for_status()?
            .json()?;
        self.access_token = reply.access_token;
        if let Some(r) = reply.refresh_token {
            self.refresh_token = r;
        }
        // Refresh a minute early so a token never expires mid-request.
        self.expires_at = now() + reply.expires_in.saturating_sub(60);
        Ok(())
    }

    pub fn load() -> Result<Self> {
        let json = keychain()?.get_password().map_err(login_error)?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn save(&self) -> Result<()> {
        Ok(keychain()?.set_password(&serde_json::to_string(self)?)?)
    }

    /// Ask Miele to end the session and kill its tokens.
    // Keycloak `/logout` with the refresh token: it ends the whole session. `/revoke` exists too.
    pub fn end_at_miele(&self) -> Result<()> {
        reqwest::blocking::Client::new()
            .post(format!("{AUTH}/logout"))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("refresh_token", self.refresh_token.as_str()),
            ])
            .send()?
            .error_for_status()?;
        Ok(())
    }

    pub fn forget() -> Result<()> {
        match keychain()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// A valid access token, refreshed (and saved) if it has expired.
    pub fn access_token(&mut self) -> Result<String> {
        if now() >= self.expires_at {
            let refresh = self.refresh_token.clone();
            self.token_request(&[("grant_type", "refresh_token"), ("refresh_token", &refresh)])
                .map_err(|e| format!("Login expired ({e}). Run `miele login` again."))?;
            self.save()?;
        }
        Ok(self.access_token.clone())
    }
}

/// Log out: end the session at Miele and delete the saved login. `Ok(false)`: there was none.
pub fn logout() -> Result<bool> {
    let session = match Session::load() {
        Ok(s) => s,
        // No entry, or one too damaged to read: clear it. A refused Keychain access is reported.
        // lean: "no entry" is matched by message text; use an error enum if load gets more cases.
        Err(e) if e.is::<serde_json::Error>() || e.to_string() == NOT_LOGGED_IN => {
            Session::forget()?;
            return Ok(false);
        }
        Err(e) => return Err(e),
    };
    let at_miele = session.end_at_miele();
    Session::forget()?; // delete locally even if Miele failed
    if let Err(e) = at_miele {
        return Err(format!(
            "Deleted the local login, but Miele did not confirm the logout ({e})."
        )
        .into());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(status: i64) -> State {
        serde_json::from_value(serde_json::json!({
            "status": {"value_raw": status, "value_localized": ""},
            "remainingTime": [0, 0], "signalInfo": false, "signalFailure": false,
            "remoteEnable": {"fullRemoteControl": true, "mobileStart": false}
        }))
        .unwrap()
    }

    #[test]
    fn decide_waits_for_the_machine() {
        let seen = |codes: &[i64]| codes.iter().map(|&c| after(c)).collect::<Vec<_>>();
        // Machine on, drink never starts (empty tank): not confirmed, whatever Miele replied.
        assert_eq!(
            decide(Request::Brew, false, seen(&[2, 2, 2])),
            Outcome::NotConfirmed
        );
        assert_eq!(
            decide(Request::Brew, true, seen(&[2, 2])),
            Outcome::NotConfirmed
        );
        assert_eq!(
            decide(Request::Brew, false, seen(&[])),
            Outcome::NotConfirmed
        );
        // Changes later on count.
        assert_eq!(decide(Request::Brew, false, seen(&[2, 5])), Outcome::Done);
        assert_eq!(
            decide(Request::Brew, true, seen(&[2, 5])),
            Outcome::DoneDespiteServerError
        );
        assert_eq!(decide(Request::On, false, seen(&[1, 1, 2])), Outcome::Done);
        assert_eq!(
            decide(Request::Off, true, seen(&[2, 1])),
            Outcome::DoneDespiteServerError
        );
    }

    #[test]
    fn confirmed_checks_what_the_machine_did() {
        for (request, status, want) in [
            (Request::On, 2, true),
            (Request::On, 5, true),
            (Request::On, 1, false),
            (Request::On, 255, false),
            (Request::Off, 1, true),
            (Request::Off, 2, false),
            (Request::Brew, 5, true),
            (Request::Brew, 2, false),
            (Request::Brew, 1, false),
        ] {
            assert_eq!(confirmed(request, &after(status)), want, "{status}");
        }
    }

    #[test]
    fn dump_strips_serials_and_names() {
        let mut v: serde_json::Value = serde_json::from_str(
            r#"{"freemiele":"0.2.0","appliances":[{"id":"appliance-1","type":"Washer-dryer","model":"WTR860",
            "device":{"ident":{"deviceName":"Anna's washer","deviceIdentLabel":{"fabNumber":"000123456789",
            "fabIndex":"64","techType":"WTR860","matNumber":"11223344"},
            "xkmIdentLabel":{"techType":"EK037","releaseVersion":"02.72"},
            "swids":["4497","1234567890"]},"state":{"status":{"value_raw":5}}},
            "actions":{"error":"404 Not Found for /devices/000123456789/actions"},
            "programs":[{"program":"Cottons","programId":1}],"fillingLevels":{"note":"123456789012"}}]}"#,
        )
        .unwrap();
        strip_identifiers(&mut v);
        let out = serde_json::to_string(&v).unwrap();
        for gone in [
            "000123456789",
            "123456789012",
            "11223344",
            "1234567890",
            "Anna",
            "fab",
            "swids",
        ] {
            assert!(!out.contains(gone), "{gone} survived: {out}");
        }
        assert!(
            !out.split(|c: char| !c.is_ascii_digit())
                .any(|r| r.len() >= 10)
        );
        assert!(
            out.contains("WTR860") && out.contains("Cottons") && out.contains(r#""deviceName":"""#)
        );
        // The error keeps its meaning; only the id goes.
        assert!(out.contains("404 Not Found for /devices/[removed]/actions"));
    }

    #[test]
    fn login_wait_reports_a_busy_port_and_a_cancel() {
        let never = |_: &Url| -> Result<()> { panic!("must not open the browser") };
        let Ok(held) = TcpListener::bind("127.0.0.1:8765") else {
            return; // something else on this machine is using the port
        };
        let busy = wait_for_login("id", never, &AtomicBool::new(false)).unwrap_err();
        assert_eq!(
            busy.to_string(),
            "Another login is already waiting. Close it and try again."
        );
        drop(held);
        let mut opened = String::new();
        let open = |url: &Url| -> Result<()> {
            opened = url.to_string();
            Ok(())
        };
        let cancelled = wait_for_login("my-id", open, &AtomicBool::new(true)).unwrap_err();
        assert_eq!(cancelled.to_string(), "Login cancelled.");
        assert!(opened.contains("client_id=my-id"));
    }

    #[test]
    fn keychain_errors_say_what_went_wrong() {
        assert_eq!(login_error(keyring::Error::NoEntry), NOT_LOGGED_IN);
        let refused = login_error(keyring::Error::NoStorageAccess("denied".into()));
        assert!(refused.starts_with("Your system's password store didn't allow access"));
        assert!(refused.contains("denied"));
    }

    #[test]
    fn old_session_with_legacy_field_still_loads() {
        let json = r#"{"client_id":"a","client_secret":"b","legacy":false,
            "access_token":"c","refresh_token":"d","expires_at":1}"#;
        assert!(serde_json::from_str::<Session>(json).is_ok());
    }

    #[test]
    fn check_appliance_code_needs_attention() {
        let mut devices: Devices =
            serde_json::from_str(include_str!("../../../fixtures/devices.sample.json")).unwrap();
        let d = devices.get_mut("000123456789").unwrap();
        d.state.signal_info = false;
        d.state.status.value_raw = 2;
        assert!(!needs_attention(d));
        d.state.program_id.value_raw = Some(17004);
        assert!(needs_attention(d));
        assert!(status_line(d).contains("needs attention"));
    }

    #[test]
    fn parses_sample_and_prints_status() {
        let devices: Devices =
            serde_json::from_str(include_str!("../../../fixtures/devices.sample.json")).unwrap();
        let d = &devices["000123456789"];
        assert_eq!(
            status_line(d),
            "CM6160 Coffee Machine: Off (mobile start off) — needs attention: check the machine's display"
        );
    }

    #[test]
    fn names_coffee_program_and_phase() {
        let mut devices: Devices =
            serde_json::from_str(include_str!("../../../fixtures/devices.sample.json")).unwrap();
        let d = devices.get_mut("000123456789").unwrap();
        let set = |d: &mut Device, status, id, id_text: &str, phase| {
            d.state.status = Value {
                value_raw: status,
                value_localized: if status == 5 { "In use" } else { "Off" }.into(),
            };
            d.state.program_id = Reading {
                value_raw: Some(id),
                value_localized: Some(id_text.into()),
            };
            d.state.program_phase.value_raw = Some(phase);
            status_line(d)
        };
        // Care programme: the API gives no text, so ours is used; phase 4352 = not running.
        let line = set(d, 5, 24753, "", 4352);
        assert!(line.starts_with("CM6160 Coffee Machine: In use, Degreasing the brew unit"));
        assert!(!line.contains("(Not"));
        // A drink: the API's own name, plus our phase.
        let line = set(d, 5, 24000, "coffee", 4377);
        assert!(line.starts_with("CM6160 Coffee Machine: In use, coffee · Grinding"));
        // Our table name that nobody confirmed: the name only, never the code.
        let line = set(d, 5, 24750, "", 4377);
        assert!(line.starts_with("CM6160 Coffee Machine: In use, Appliance rinse · Grinding"));
        // Codes we have no name for are left out, never shown as numbers.
        let line = set(d, 5, 24760, "", 4410);
        assert!(!line.contains("24760") && !line.contains("4410"));
        assert!(line.starts_with("CM6160 Coffee Machine: In use"));
        // Off stays as it was.
        assert!(set(d, 0, 24753, "", 4352).starts_with("CM6160 Coffee Machine: Off (mobile"));
    }

    #[test]
    fn shows_washer_dryer_program_phase_and_time() {
        let mut devices: Devices = serde_json::from_str(include_str!(
            "../../../fixtures/devices.washer-running.sample.json"
        ))
        .unwrap();
        let d = devices.get_mut("000000000024").unwrap();
        assert_eq!(
            status_line(d),
            "WTR860 Washer dryer: In use, Sportswear · Main wash (40 °C · 1200 rpm), 0h23 left (mobile start off)"
        );
        d.state.status = Value {
            value_raw: 7,
            value_localized: "Finished".into(),
        };
        assert_eq!(
            status_line(d),
            "WTR860 Washer dryer: Finished, Sportswear (40 °C · 1200 rpm) (mobile start off)"
        );
    }

    #[test]
    fn unnamed_washer_programme_uses_its_type() {
        let mut devices: Devices = serde_json::from_str(include_str!(
            "../../../fixtures/devices.washer-running.sample.json"
        ))
        .unwrap();
        let d = devices.get_mut("000000000024").unwrap();
        // Code we have no name for, no text from Miele: the programme type (#31).
        d.state.program_id = Reading {
            value_raw: Some(9999),
            value_localized: Some("".into()),
        };
        d.state.program_type = Reading {
            value_raw: Some(4),
            value_localized: Some("Cleaning/Care programme".into()),
        };
        d.state.program_phase.value_localized = Some("Rinse out fluff".into());
        assert!(status_line(d).starts_with(
            "WTR860 Washer dryer: In use, Cleaning/Care programme · Rinse out fluff (40 °C"
        ));
        d.state.status = Value {
            value_raw: 7,
            value_localized: "Finished".into(),
        };
        assert!(
            status_line(d).starts_with("WTR860 Washer dryer: Finished, Cleaning/Care programme (")
        );
        // No type text either: no programme shown, and never the code.
        d.state.program_type = Reading::default();
        assert!(status_line(d).starts_with("WTR860 Washer dryer: Finished ("));
        assert!(!status_line(d).contains("9999"));
        // Washer-dryer 48 has a table name: the name only.
        d.state.program_id.value_raw = Some(48);
        assert!(status_line(d).contains("Finished, Rinse out lint ("));
    }

    #[test]
    fn details_skip_unused_settings_and_other_appliances() {
        let mut devices: Devices = serde_json::from_str(include_str!(
            "../../../fixtures/devices.washer-running.sample.json"
        ))
        .unwrap();
        let d = devices.get_mut("000000000024").unwrap();
        assert_eq!(details(d).as_deref(), Some("40 °C · 1200 rpm"));
        d.state.target_temperature[0].value_raw = -32768;
        assert_eq!(details(d).as_deref(), Some("1200 rpm"));
        d.state.spinning_speed.value_raw = None;
        assert_eq!(details(d), None);
        let coffee: Devices =
            serde_json::from_str(include_str!("../../../fixtures/devices.sample.json")).unwrap();
        assert_eq!(details(&coffee["000123456789"]), None);
    }

    #[test]
    fn twin_dos_line_says_empty_and_percent() {
        let f: FillingLevels = serde_json::from_str(
            r#"{"twinDosContainer1FillingLevel":0,"twinDosContainer2FillingLevel":28}"#,
        )
        .unwrap();
        assert_eq!(
            twin_dos_line(&f).as_deref(),
            Some("TwinDos: 1 empty · 2 28%")
        );
        let coffee: FillingLevels =
            serde_json::from_str(include_str!("../../../fixtures/fillingLevels.sample.json"))
                .unwrap();
        assert_eq!(twin_dos_line(&coffee), None);
    }

    #[test]
    fn utc_offsets_from_date_and_powershell() {
        assert_eq!(offset_secs("+0200\n"), 7200);
        assert_eq!(offset_secs("-03:30"), -12600);
        assert_eq!(offset_secs("-0100"), -3600);
        assert_eq!(offset_secs("7200"), 0);
        assert_eq!(offset_secs(""), 0);
    }

    #[test]
    fn dishwasher_line_skips_null_levels() {
        let f: FillingLevels = serde_json::from_str(
            r#"{"saltFillingLevel":0,"rinseAidFillingLevel":null,"powerDiscFillingLevel":80}"#,
        )
        .unwrap();
        assert_eq!(
            dishwasher_line(&f).as_deref(),
            Some("Salt empty · PowerDisc 80%")
        );
        assert_eq!(twin_dos_line(&f), None);
    }

    #[test]
    fn dishwasher_waiting_shows_programme_and_start() {
        let devices: Devices = serde_json::from_str(include_str!(
            "../../../fixtures/devices.dishwasher-waiting.sample.json"
        ))
        .unwrap();
        let line = status_line(&devices["000000000007"]);
        assert!(
            line.starts_with("G7310 Dishwasher: Waiting to start, ECO, starts at "),
            "{line}"
        );
        assert!(line.ends_with(", 3h15 left"), "{line}");
        // 2h30 after 23:00 local is 01:30; [0, 0] or missing means no delayed start.
        let late = 23 * 3600 + 15;
        assert_eq!(starts_at(&[2, 30], late).as_deref(), Some("01:30"));
        assert_eq!(starts_at(&[0, 0], late), None);
        assert_eq!(starts_at(&[], late), None);
    }

    #[test]
    fn history_lines_use_local_time_and_fall_back_to_our_names() {
        let runs: Vec<LastRun> = serde_json::from_str(include_str!(
            "../../../fixtures/washer.lastUsedPrograms.sample.json"
        ))
        .unwrap();
        let lines: Vec<_> = runs.iter().map(|r| history_line(r, 3600)).collect();
        assert_eq!(lines[0], "Mon 2 Mar 18:00 · Rinse out lint · 8 min");
        assert_eq!(lines[1], "Mon 2 Mar 16:30 · Warm air · 46 min");
        assert_eq!(lines[2], "Sun 1 Mar 09:05 · Automatic plus · 1 h 34 min");
        // No end, or an end before the start: no duration rather than an error or a negative one.
        let odd: Vec<LastRun> = serde_json::from_str(
            r#"[{"ProgramID":{"value_raw":31,"value_localized":"Automatic plus"},"startAt":"2026-03-01T08:05:00Z"},
                {"ProgramID":{"value_raw":31,"value_localized":"Automatic plus"},"startAt":"2026-03-01T08:05:00Z","endAt":"2026-03-01T08:00:00Z"}]"#,
        )
        .unwrap();
        for r in &odd {
            assert_eq!(history_line(r, 3600), "Sun 1 Mar 09:05 · Automatic plus");
        }
    }

    #[test]
    fn shows_care_counters_from_sample() {
        let f: FillingLevels =
            serde_json::from_str(include_str!("../../../fixtures/fillingLevels.sample.json"))
                .unwrap();
        assert_eq!(
            care_line(&f).unwrap(),
            "  Care so far: descaled 20× · degreased 13× · milk system cleaned 12×"
        );
        let none = FillingLevels {
            descaling_counter: None,
            degreasing_counter: None,
            milk_cleaning_counter: None,
            twin_dos_container1_filling_level: None,
            twin_dos_container2_filling_level: None,
            salt_filling_level: None,
            rinse_aid_filling_level: None,
            power_disc_filling_level: None,
        };
        assert_eq!(care_line(&none), None);
    }

    #[test]
    fn chooses_device() {
        let sample = || -> Device {
            let mut m: Devices =
                serde_json::from_str(include_str!("../../../fixtures/devices.sample.json"))
                    .unwrap();
            m.remove("000123456789").unwrap()
        };
        let err = |c: &[(&str, &Device)], w| choose(c, w).err().unwrap().to_string();
        let (a, mut b) = (sample(), sample());
        b.ident.device_name = "Office".into();
        let both = [("a", &a), ("b", &b)];
        // Without --device, one candidate is fine; two are not.
        assert_eq!(choose(&both[..1], None).unwrap().0, "a");
        assert!(err(&both, None).contains("--device"));
        // Match by model (ignoring case and spaces) or by name.
        assert_eq!(choose(&both[1..], Some("cm 6160")).unwrap().0, "b");
        assert_eq!(choose(&both, Some("office")).unwrap().0, "b");
        // Both have the model; the message lists them.
        let e = err(&both, Some("CM6160"));
        assert!(e.contains("specific") && e.contains("Office"));
        // No match lists the names; an empty --device matches nothing.
        let e = err(&both, Some("washer"));
        assert!(e.contains("Available") && e.contains("Office"));
        assert!(choose(&both, Some("")).is_err());
    }

    #[test]
    fn finds_program_ignoring_case() {
        let programs: Vec<Program> =
            serde_json::from_str(include_str!("../../../fixtures/programs.sample.json")).unwrap();
        assert_eq!(
            find_program(&programs, "flat white").unwrap().program_id,
            24012
        );
        assert_eq!(
            find_program(&programs, "CAFFÈ LATTE").unwrap().program_id,
            24009
        );
        assert!(find_program(&programs, "mocha").is_none());
        assert!(find_program(&programs, "").is_none());

        let actions: Actions =
            serde_json::from_str(include_str!("../../../fixtures/actions.sample.json")).unwrap();
        assert!(actions.power_on && !actions.power_off);
    }

    #[test]
    fn reads_login_code_only_with_matching_state() {
        let line = "GET /?state=abc&code=x%2Fy HTTP/1.1";
        assert_eq!(code_from_request(line, "abc").as_deref(), Some("x/y"));
        assert_eq!(code_from_request(line, "other"), None);
        assert_eq!(code_from_request("GET /favicon.ico HTTP/1.1", "abc"), None);
        assert_eq!(
            code_from_request("GET /?state=abc&error=access_denied HTTP/1.1", "abc"),
            None
        );
    }

    #[test]
    fn reads_events_and_reports_changes() {
        let mut sse = Sse::default();
        let mut seen = HashMap::new();
        let (mut count, mut printed) = (HashMap::new(), Vec::new());
        for line in include_str!("../../../fixtures/events.sample.txt").lines() {
            if let Some((event, data)) = sse.line(line) {
                *count.entry(event.clone()).or_insert(0) += 1;
                if event == "devices" {
                    printed.extend(changes(&mut seen, &serde_json::from_str(&data).unwrap()));
                }
            }
        }
        assert_eq!((count["devices"], count["actions"]), (9, 9));
        assert!(count["ping"] > 0);
        // On connect every appliance is printed once; after that only what changed.
        assert_eq!(printed.len(), 9);
        assert_eq!(printed[0], "CM6160 Coffee machine: Off (mobile start off)");
        assert!(
            printed[2]
                .ends_with("Sportswear · Rinsing (40 °C · 1200 rpm), 0h23 left (mobile start off)")
        );
        // Same state again prints nothing.
        let devices: Devices =
            serde_json::from_str(include_str!("../../../fixtures/devices.sample.json")).unwrap();
        assert_eq!(changes(&mut seen, &devices).len(), 1);
        assert!(changes(&mut seen, &devices).is_empty());
    }
}
