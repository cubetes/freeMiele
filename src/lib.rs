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

/// One line per device, e.g. "CM6160 Coffee Machine: Off (mobile start off) — needs attention".
pub fn status_line(d: &Device) -> String {
    let mut line = format!("{}: {}", name(d), d.state.status.value_localized);
    let [h, m] = d.state.remaining_time;
    if h + m > 0 {
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
/// Unit unconfirmed, so the wording lives in `care_line` only.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillingLevels {
    pub descaling_counter: Option<i64>,
    pub degreasing_counter: Option<i64>,
    pub milk_cleaning_counter: Option<i64>,
}

/// `Ident.kind.value_raw` of a coffee machine.
pub const COFFEE_MACHINE: i64 = 17;

/// e.g. "  Care counters: descaling 20 · degreasing 13 · milk cleaning 12"; None if none is set.
pub fn care_line(f: &FillingLevels) -> Option<String> {
    let parts: Vec<_> = [
        ("descaling", f.descaling_counter),
        ("degreasing", f.degreasing_counter),
        ("milk cleaning", f.milk_cleaning_counter),
    ]
    .into_iter()
    .filter_map(|(label, n)| n.map(|n| format!("{label} {n}")))
    .collect();
    (!parts.is_empty()).then(|| format!("  Care counters: {}", parts.join(" · ")))
}

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
        let json = keychain()?
            .get_password()
            .map_err(|_| "Not logged in. Run `miele login` first.")?;
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
    fn old_session_with_legacy_field_still_loads() {
        let json = r#"{"client_id":"a","client_secret":"b","legacy":false,
            "access_token":"c","refresh_token":"d","expires_at":1}"#;
        assert!(serde_json::from_str::<Session>(json).is_ok());
    }

    #[test]
    fn parses_sample_and_prints_status() {
        let devices: Devices =
            serde_json::from_str(include_str!("../fixtures/devices.sample.json")).unwrap();
        let d = &devices["000123456789"];
        assert_eq!(
            status_line(d),
            "CM6160 Coffee Machine: Off (mobile start off) — needs attention (water, beans, tray or care)"
        );
    }

    #[test]
    fn shows_care_counters_from_sample() {
        let f: FillingLevels =
            serde_json::from_str(include_str!("../fixtures/fillingLevels.sample.json")).unwrap();
        assert_eq!(
            care_line(&f).unwrap(),
            "  Care counters: descaling 20 · degreasing 13 · milk cleaning 12"
        );
        let none = FillingLevels {
            descaling_counter: None,
            degreasing_counter: None,
            milk_cleaning_counter: None,
        };
        assert_eq!(care_line(&none), None);
    }

    #[test]
    fn finds_program_ignoring_case() {
        let programs: Vec<Program> =
            serde_json::from_str(include_str!("../fixtures/programs.sample.json")).unwrap();
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
            serde_json::from_str(include_str!("../fixtures/actions.sample.json")).unwrap();
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
