use reqwest::Url;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    pub signal_info: bool,
    pub signal_failure: bool,
    pub remote_enable: RemoteEnable,
    #[serde(rename = "ProgramID", default)]
    pub program_id: Reading,
    #[serde(default)]
    pub program_phase: Reading,
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

pub fn put(token: &str, path: &str, body: serde_json::Value) -> Result<()> {
    let reply = reqwest::blocking::Client::new()
        .put(format!("{API}{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()?;
    let status = reply.status();
    if !status.is_success() {
        // Miele explains refusals in the body, e.g. why a program can't start now.
        return Err(format!("Miele refused ({status}): {}", reply.text()?).into());
    }
    Ok(())
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

// Codes from Home Assistant's Miele integration (Apache-2.0): homeassistant/components/miele/const.py
fn coffee_program_name(code: i64) -> Option<&'static str> {
    Some(match code {
        24750 | 24759 | 24773 | 24787 | 24788 => "Rinsing the machine",
        24758 => "Intermediate rinsing",
        24778 => "Automatic maintenance",
        24751 => "Descaling",
        24753 => "Degreasing the brew unit",
        24754 => "Rinsing the milk pipework",
        24789 => "Cleaning the milk pipework",
        24575 => "Descaling counter",
        24576 => "Degreasing counter",
        16016
        | 16018
        | 16019
        | 16020
        | 16021
        | 16027
        | 16033
        | 16035
        | 16037
        | 24500
        | 24502..=24504
        | 24506
        | 24513
        | 24516
        | 24537
        | 24542
        | 24549..=24558
        | 24560
        | 24562..=24569
        | 24571..=24574
        | 24800
        | 24801
        | 24813 => "Settings",
        _ => return None,
    })
}

fn coffee_phase_name(code: i64) -> Option<&'static str> {
    Some(match code {
        3073 => "Heating up",
        4353 => "Espresso",
        4354 => "Hot milk",
        4355 => "Milk foam",
        4361 | 4404 => "Dispensing",
        4369 => "Pre-brewing",
        4377 => "Grinding",
        4385 => "Second espresso",
        4393 => "Second pre-brewing",
        4401 => "Second grinding",
        4405 => "Rinse",
        _ => return None, // 0, 4352, 65535 = not running
    })
}

type NameOf = fn(i64) -> Option<&'static str>;

/// The API's own text if it has one, else our name for the code.
fn reading_text(r: &Reading, ours: NameOf) -> Option<String> {
    match r.value_localized.as_deref() {
        Some(t) if !t.is_empty() => Some(t.to_string()),
        _ => ours(r.value_raw?).map(String::from),
    }
}

/// One line per device, e.g. "CM6160 Coffee Machine: Off (mobile start off) — needs attention".
/// An appliance at work adds its program and phase: "In use, coffee · Grinding".
/// A washer or dryer that has finished says "Finished, Sportswear", with no phase or time left.
pub fn status_line(d: &Device) -> String {
    let mut line = format!("{}: {}", name(d), d.state.status.value_localized);
    let status = d.state.status.value_raw;
    // Coffee machines need our names; washers and dryers send their own text.
    let names: Option<(NameOf, NameOf)> = match d.ident.kind.value_raw {
        COFFEE_MACHINE => Some((coffee_program_name, coffee_phase_name)),
        WASHING_MACHINE | TUMBLE_DRYER | WASHER_DRYER => Some((|_| None, |_| None)),
        _ => None,
    };
    // 0 = Off, 255 = Not connected
    if let Some((program_name, phase_name)) = names
        && ![0, 255].contains(&status)
        && let Some(program) = reading_text(&d.state.program_id, program_name)
    {
        line += &format!(", {program}");
        if status != END
            && let Some(phase) = reading_text(&d.state.program_phase, phase_name)
        {
            line += &format!(" · {phase}");
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
    } else if d.state.signal_info {
        line += " — needs attention (water, beans, tray or care)";
    }
    line
}

/// Coffee machine care counters, from `GET /devices/{id}/fillingLevels` (null = not supported).
/// Each is a lifetime count of completed care programmes (confirmed on a CM 6160; milk by analogy).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillingLevels {
    pub descaling_counter: Option<i64>,
    pub degreasing_counter: Option<i64>,
    pub milk_cleaning_counter: Option<i64>,
}

/// `Ident.kind.value_raw` of a coffee machine.
pub const COFFEE_MACHINE: i64 = 17;
const WASHING_MACHINE: i64 = 1;
const TUMBLE_DRYER: i64 = 2;
const WASHER_DRYER: i64 = 24;
/// `State.status.value_raw` of a finished program.
const END: i64 = 7;

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

/// Our own short summary of the owner's manual care table, printed under each coffee machine.
pub const CARE_SCHEDULE: &str =
    "  When the machine asks: descale; degrease the brew unit (1 cleaning tablet)
  Every day: empty the drip tray and waste container, clean the milk flask and siphon
  Every week: rinse the brew unit under running water, clean the central spout
  Every month: clean the bean container and the water container filter
  Due-now reminders only appear on the machine's display.";

/// Login details, kept as one entry in the macOS Keychain.
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

/// Tell "no saved login" apart from "macOS refused access to it".
fn login_error(e: keyring::Error) -> String {
    match e {
        keyring::Error::NoEntry => NOT_LOGGED_IN.into(),
        e => format!(
            "macOS didn't allow access to the saved login. Run the command again and click Always Allow. ({e})"
        ),
    }
}

pub fn login_url(client_id: &str, state: &str) -> Url {
    let params = vec![
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT),
        ("state", state),
        ("scope", "openid mcs_thirdparty_read mcs_thirdparty_write"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keychain_errors_say_what_went_wrong() {
        assert_eq!(login_error(keyring::Error::NoEntry), NOT_LOGGED_IN);
        let refused = login_error(keyring::Error::NoStorageAccess("denied".into()));
        assert!(refused.starts_with("macOS didn't allow access"));
        assert!(refused.contains("denied"));
    }

    #[test]
    fn old_session_with_legacy_field_still_loads() {
        let json = r#"{"client_id":"a","client_secret":"b","legacy":false,
            "access_token":"c","refresh_token":"d","expires_at":1}"#;
        assert!(serde_json::from_str::<Session>(json).is_ok());
    }

    #[test]
    fn parses_sample_and_prints_status() {
        let devices: Devices =
            serde_json::from_str(include_str!("../../../fixtures/devices.sample.json")).unwrap();
        let d = &devices["000123456789"];
        assert_eq!(
            status_line(d),
            "CM6160 Coffee Machine: Off (mobile start off) — needs attention (water, beans, tray or care)"
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
        // Unknown drink without a name: nothing added.
        assert!(set(d, 5, 24000, "", 4377).starts_with("CM6160 Coffee Machine: In use (mobile"));
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
            "WTR860 Washer dryer: In use, Sportswear · Main wash, 0h23 left (mobile start off)"
        );
        d.state.status = Value {
            value_raw: 7,
            value_localized: "Finished".into(),
        };
        assert_eq!(
            status_line(d),
            "WTR860 Washer dryer: Finished, Sportswear (mobile start off)"
        );
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
}
