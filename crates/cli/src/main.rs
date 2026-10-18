use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use freemiele::dataact::{self, Appliance, Consumption, Refused, Run};
use freemiele::{
    Actions, CARE_SCHEDULE, COFFEE_MACHINE, DISHWASHER, Device, Devices, FillingLevels, LastRun,
    Outcome, Program, Result, Session, WASHERS_AND_DRYERS, care_line, choose, dishwasher_line, get,
    history_line, name, twin_dos_line, utc_offset,
};
use std::io::{IsTerminal, Write};
use std::sync::atomic::AtomicBool;

#[derive(Parser)]
#[command(
    name = "miele",
    version,
    about = "Control Miele appliances from the terminal"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Log in to your Miele account (once); the login is kept in your system's password store
    Login,
    /// Log out: end the login at Miele and delete it from the Keychain
    Logout,
    /// Show the state of every appliance on your Miele account
    Status {
        /// Print one JSON array instead (no serial numbers), for scripts
        #[arg(long)]
        json: bool,
    },
    /// Print a line whenever an appliance changes, until you press Ctrl+C
    Watch,
    /// Print raw appliance data as JSON for a support report; serial numbers and names are removed
    Dump,
    /// Coffee machine: how often it has been cleaned, and the care schedule. Washer: TwinDos levels.
    /// Dishwasher: salt, rinse aid and PowerDisc levels
    Care,
    /// The last runs of your washer or dryer
    History {
        /// Which appliance, by name or model; needed if several qualify
        #[arg(long)]
        device: Option<String>,
    },
    /// Water and energy use per week and month (needs Miele's Data Act access)
    Usage {
        /// Show the latest runs instead
        #[arg(long)]
        runs: bool,
    },
    /// Print a Tab-completion script for your shell (see the README for where to put it)
    Completions { shell: Shell },
    /// Turn on the appliance that can be turned on (e.g. the coffee machine)
    On {
        /// Which appliance, by name or model (e.g. CM6160); needed if several qualify
        #[arg(long)]
        device: Option<String>,
    },
    /// Turn off the appliance that can be turned off
    Off {
        /// Which appliance, by name or model (e.g. CM6160); needed if several qualify
        #[arg(long)]
        device: Option<String>,
    },
    /// Make a drink, e.g. `miele brew flat white`. With no drink, list the choices.
    Brew {
        /// Which appliance, by name or model (e.g. CM6160); needed if several qualify
        #[arg(long)]
        device: Option<String>,
        drink: Vec<String>,
    },
}

fn main() {
    if let Err(e) = run(Cli::parse().cmd) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Login => login(),
        Cmd::Logout => logout(),
        Cmd::Status { json } => {
            let token = token()?;
            let devices: Devices = get(&token, "/devices")?;
            if json {
                println!("{}", status_json(&devices));
                return Ok(());
            }
            if devices.is_empty() {
                println!("No appliances on this Miele account.");
            }
            let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
            for (id, d) in &devices {
                let fault = (d.state.signal_failure
                    && freemiele::has_fault_text(d.ident.kind.value_raw))
                .then(|| freemiele::failure_line(&token, id))
                .flatten();
                let line = colored_status(d, color);
                match fault {
                    Some(text) => println!("{line}: {text}"),
                    None => println!("{line}"),
                }
            }
            Ok(())
        }
        Cmd::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "miele", &mut std::io::stdout());
            Ok(())
        }
        Cmd::Watch => {
            let mut session = Session::load()?;
            freemiele::watch(&mut session, |_, lines| {
                for line in lines {
                    println!("{}  {line}", clock());
                }
            })
        }
        Cmd::Care => care(),
        Cmd::History { device } => history(device.as_deref()),
        Cmd::Usage { runs } => usage(runs),
        Cmd::Dump => {
            println!(
                "{}",
                serde_json::to_string_pretty(&freemiele::dump(&token()?)?)?
            );
            eprintln!("Serial numbers and names removed. Check the file before sharing it.");
            Ok(())
        }
        Cmd::On { device } => power(true, device.as_deref()),
        Cmd::Off { device } => power(false, device.as_deref()),
        Cmd::Brew { device, drink } => brew(device.as_deref(), &drink.join(" ")),
    }
}

/// `status_line` with the state word coloured (256-colour ANSI near the design/ palette).
/// The word stays, so colour is never the only signal.
fn colored_status(d: &Device, color: bool) -> String {
    let line = freemiele::status_line(d);
    let word = &d.state.status.value_localized;
    let prefix = format!("{}: ", name(d));
    let status = d.state.status.value_raw;
    let code = if d.state.signal_failure || status == 8 {
        210 // fault
    } else if freemiele::needs_attention(d) {
        179 // attention
    } else if [0, 1, 255].contains(&status) {
        245 // off / not connected: dim
    } else if status == 5 {
        173 // running: copper
    } else {
        79 // ready: teal
    };
    match line.strip_prefix(&prefix) {
        Some(rest) if color => format!(
            "{prefix}\x1b[38;5;{code}m{word}\x1b[0m{}",
            &rest[word.len()..]
        ),
        _ => line,
    }
}

/// One JSON object per appliance. No serial numbers or device ids, and not the name you gave the
/// appliance (it could name your home): `name` is always model + type.
fn status_json(devices: &Devices) -> serde_json::Value {
    let mut list: Vec<_> = devices
        .values()
        .map(|d| {
            let (program, phase) = freemiele::activity(d);
            let [h, m] = d.state.remaining_time;
            let minutes =
                (h + m > 0 && d.state.status.value_raw != freemiele::END).then(|| h * 60 + m);
            let (model, kind) = (
                &d.ident.device_ident_label.tech_type,
                &d.ident.kind.value_localized,
            );
            serde_json::json!({
                "name": format!("{model} {kind}"),
                "model": model,
                "type": kind,
                "status": d.state.status.value_localized,
                "program": program,
                "phase": phase,
                "minutes_left": minutes,
            })
        })
        .collect();
    list.sort_by_key(|v| v["name"].as_str().map(String::from));
    list.into()
}

fn token() -> Result<String> {
    Session::load()?.access_token()
}

/// Local time as HH:MM:SS, e.g. "22:48:14".
fn clock() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |t| t.as_secs() as i64);
    let s = (now + utc_offset()).rem_euclid(86400);
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

/// GET from the Data Act API. A 403 ends the command (not an error: Miele hasn't opened it yet);
/// a 400 means Miele switched that feature off, so there is nothing to show.
fn data_act<T: serde::de::DeserializeOwned>(token: &str, path: &str) -> Result<Option<T>> {
    match dataact::get(token, path) {
        Ok(v) => Ok(Some(v)),
        Err(e) => match e.downcast_ref::<Refused>() {
            Some(Refused(403)) => {
                println!(
                    "Miele hasn't given this account Data Act access yet (see https://github.com/cubetes/freeMiele/issues/10)."
                );
                std::process::exit(0)
            }
            Some(Refused(400)) => {
                println!("  Miele has switched that feature off for your country or appliance.");
                Ok(None)
            }
            _ => Err(e),
        },
    }
}

fn usage(runs: bool) -> Result<()> {
    // lean: reuses the 3rd Party API token; a Data Act token flow comes when Miele explains it (#10)
    let token = token()?;
    let offset = utc_offset();
    let Some(appliances) = data_act::<Vec<Appliance>>(&token, "/Appliances/Wifi")? else {
        return Ok(());
    };
    if appliances.is_empty() {
        println!("No appliances in the Miele Data Act API.");
    }
    for a in &appliances {
        println!("{}", dataact::appliance_name(a));
        let base = format!("/Appliances/{}/AdditionalFeatures", a.id);
        if runs {
            let latest: Option<Vec<Run>> =
                data_act(&token, &format!("{base}/LatestOperationRuns"))?;
            for r in latest.iter().flatten() {
                println!("  {}", dataact::run_line(r, offset));
            }
        } else {
            for period in ["Weeks&timeAmount=4", "Months&timeAmount=3"] {
                let path = format!("{base}/ConsumptionStatistics?timePeriod={period}");
                if let Some(c) = data_act::<Consumption>(&token, &path)? {
                    print!("{}", dataact::usage_table(&c, offset));
                }
            }
        }
    }
    Ok(())
}

fn care() -> Result<()> {
    let token = token()?;
    let devices: Devices = get(&token, "/devices")?;
    let mut found = false;
    for (id, d) in devices
        .iter()
        .filter(|(_, d)| d.ident.kind.value_raw == COFFEE_MACHINE)
    {
        found = true;
        println!("{}", name(d));
        let levels: Result<FillingLevels> = get(&token, &format!("/devices/{id}/fillingLevels"));
        let line = levels.ok().as_ref().and_then(care_line);
        println!(
            "{}",
            line.unwrap_or_else(|| "  Care so far: not available".into())
        );
        println!("{CARE_SCHEDULE}");
    }
    for (id, d) in devices.iter().filter(|(_, d)| {
        let kind = d.ident.kind.value_raw;
        WASHERS_AND_DRYERS.contains(&kind) || kind == DISHWASHER
    }) {
        let levels: Result<FillingLevels> = get(&token, &format!("/devices/{id}/fillingLevels"));
        let line = levels
            .ok()
            .and_then(|f| twin_dos_line(&f).or_else(|| dishwasher_line(&f)));
        if let Some(line) = line {
            found = true;
            println!("{}\n  {line}", name(d));
        }
    }
    if !found {
        println!(
            "Nothing to show: no coffee machine, TwinDos washer or dishwasher on this Miele account."
        );
    }
    Ok(())
}

/// Miele answers 403 until statistics are switched on for the appliance. `get` keeps only the
/// status text, not the body, so a bare 403 counts too.
fn consent_missing(e: &dyn std::error::Error) -> bool {
    let text = e.to_string();
    text.contains("MISSING_CONSENT") || text.contains("403 Forbidden")
}

fn history(device: Option<&str>) -> Result<()> {
    let token = token()?;
    let devices: Devices = get(&token, "/devices")?;
    let candidates: Vec<_> = devices
        .iter()
        .filter(|(_, d)| WASHERS_AND_DRYERS.contains(&d.ident.kind.value_raw))
        .map(|(id, d)| (id.as_str(), d))
        .collect();
    if candidates.is_empty() {
        println!("No washer or dryer on this Miele account.");
        return Ok(());
    }
    let chosen;
    let shown = match device {
        Some(_) => {
            chosen = [choose(&candidates, device)?];
            &chosen[..]
        }
        None => &candidates[..],
    };
    let offset = utc_offset();
    for (id, d) in shown {
        println!("{}", name(d));
        match get::<Vec<LastRun>>(&token, &format!("/devices/{id}/lastUsedPrograms")) {
            Ok(runs) if runs.is_empty() => println!("  No runs recorded yet."),
            Ok(runs) => runs
                .iter()
                .for_each(|r| println!("  {}", history_line(r, offset))),
            Err(e) if consent_missing(e.as_ref()) => println!(
                "  Miele needs your consent first: in the Miele app, turn on statistics for this appliance."
            ),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn power(on: bool, device: Option<&str>) -> Result<()> {
    let token = token()?;
    let devices: Devices = get(&token, "/devices")?;
    let mut candidates = Vec::new();
    for (id, d) in &devices {
        let actions: Actions = get(&token, &format!("/devices/{id}/actions"))?;
        if (on && actions.power_on) || (!on && actions.power_off) {
            candidates.push((id.as_str(), d));
        }
    }
    if candidates.is_empty() {
        return Err(format!(
            "No appliance can be turned {} right now (already {0}, or offline).",
            if on { "on" } else { "off" }
        )
        .into());
    }
    let (id, d) = choose(&candidates, device)?;
    let word = if on { "on" } else { "off" };
    let outcome = freemiele::power(&token, id, on)?;
    if outcome == Outcome::NotConfirmed {
        return Err(format!("The machine didn't turn {word}. Check its display.").into());
    }
    println!("{}: turning {word}.", name(d));
    if outcome == Outcome::DoneDespiteServerError {
        println!("(Miele reported a server error, but the machine is {word}.)");
    }
    Ok(())
}

fn brew(device: Option<&str>, drink: &str) -> Result<()> {
    let token = token()?;
    let devices: Devices = get(&token, "/devices")?;
    let mut candidates = Vec::new();
    let mut menus = std::collections::HashMap::new();
    for (id, d) in &devices {
        let programs: Vec<Program> = get(&token, &format!("/devices/{id}/programs"))?;
        if !programs.is_empty() {
            candidates.push((id.as_str(), d));
            menus.insert(id.as_str(), programs);
        }
    }
    if candidates.is_empty() {
        return Err("No appliance can make drinks right now (is it on and online?).".into());
    }
    let (id, d) = choose(&candidates, device)?;
    let programs = &menus[id];
    let Some(p) = freemiele::find_program(programs, drink) else {
        let names: Vec<_> = programs
            .iter()
            .map(|p| p.program.as_str())
            .filter(|n| !n.is_empty())
            .collect();
        if !drink.is_empty() {
            println!("No drink called \"{drink}\".");
        }
        println!("{} can make: {}", name(d), names.join(", "));
        return Ok(());
    };
    if !d.state.remote_enable.mobile_start {
        return Err(format!(
            "{} won't start drinks remotely yet: turn on Mobile Start at the machine, then run this again.\n\
             (It seems to switch off after each drink, as a safety step.)",
            name(d)
        )
        .into());
    }
    let outcome = freemiele::brew(&token, id, p.program_id)?;
    if outcome == Outcome::NotConfirmed {
        return Err("The machine didn't start the drink. Check its display: water tank, beans or drip tray.".into());
    }
    println!("{}: making {}.", name(d), p.program);
    if outcome == Outcome::DoneDespiteServerError {
        println!("(Miele reported a server error, but the machine started the drink.)");
    }
    Ok(())
}

fn logout() -> Result<()> {
    println!(
        "{}",
        if freemiele::logout()? {
            "Logged out."
        } else {
            "Not logged in."
        }
    );
    Ok(())
}

fn login() -> Result<()> {
    print!("Client ID: ");
    std::io::stdout().flush()?;
    let mut client_id = String::new();
    std::io::stdin().read_line(&mut client_id)?;
    let client_secret = rpassword::prompt_password("Client secret (hidden): ")?;

    let code = freemiele::wait_for_login(
        client_id.trim(),
        |url| {
            println!("Log in to Miele in your browser. If it doesn't open, visit:\n{url}");
            std::process::Command::new("open")
                .arg(url.as_str())
                .status()?;
            Ok(())
        },
        &AtomicBool::new(false),
    )?;

    Session::login(client_id.trim().to_string(), client_secret, &code)?.save()?;
    println!("Logged in. Try `miele status`.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{colored_status, consent_missing, status_json};
    use freemiele::Devices;

    #[test]
    fn json_status_has_no_serial_and_colour_keeps_the_word() {
        let devices: Devices = serde_json::from_str(include_str!(
            "../../../fixtures/devices.washer-running.sample.json"
        ))
        .unwrap();
        let json = status_json(&devices);
        assert_eq!(json[0]["model"], "WTR860");
        assert_eq!(json[0]["program"], "Sportswear");
        assert_eq!(json[0]["minutes_left"], 23);
        assert!(!json.to_string().contains("000000000024"));
        let d = devices.values().next().unwrap();
        assert!(colored_status(d, true).contains("\x1b[38;5;173mIn use\x1b[0m, Sportswear"));
        assert_eq!(colored_status(d, false), freemiele::status_line(d));
    }

    #[test]
    fn consent_is_found_in_a_403_only() {
        let e = |m: &str| std::io::Error::other(m.to_string());
        assert!(consent_missing(&e(
            r#"Miele refused (403 Forbidden): {"code":"MISSING_CONSENT"}"#
        )));
        assert!(consent_missing(&e(
            "HTTP status client error (403 Forbidden) for url"
        )));
        assert!(!consent_missing(&e(
            "HTTP status client error (404 Not Found)"
        )));
    }
}
