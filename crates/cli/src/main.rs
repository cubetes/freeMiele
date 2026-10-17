use clap::{Parser, Subcommand};
use freemiele::dataact::{self, Appliance, Consumption, Refused, Run};
use freemiele::{
    Actions, CARE_SCHEDULE, COFFEE_MACHINE, Devices, FillingLevels, LastRun, Outcome, Program,
    Result, Session, WASHERS_AND_DRYERS, care_line, choose, get, history_line, name, twin_dos_line,
};
use std::io::Write;
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
    /// Log in to your Miele account (once); the login is kept in the macOS Keychain
    Login,
    /// Log out: end the login at Miele and delete it from the Keychain
    Logout,
    /// Show the state of every appliance on your Miele account
    Status,
    /// Print a line whenever an appliance changes, until you press Ctrl+C
    Watch,
    /// Print raw appliance data as JSON for a support report; serial numbers and names are removed
    Dump,
    /// Coffee machine: how often it has been cleaned, and the care schedule. Washer: TwinDos levels
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
        Cmd::Status => {
            let devices: Devices = get(&token()?, "/devices")?;
            if devices.is_empty() {
                println!("No appliances on this Miele account.");
            }
            devices
                .values()
                .for_each(|d| println!("{}", freemiele::status_line(d)));
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

fn token() -> Result<String> {
    Session::load()?.access_token()
}

/// Local time as HH:MM:SS, e.g. "22:48:14".
fn clock() -> String {
    // lean: asks `date` (always on macOS) to avoid a time crate; swap for chrono if this runs elsewhere.
    let out = std::process::Command::new("date").arg("+%T").output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
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

/// Seconds east of UTC now, from the system clock's zone (macOS `date`).
/// lean: one offset for all dates, so runs across a daylight-saving change are off by an hour.
fn utc_offset() -> i64 {
    let out = std::process::Command::new("date").arg("+%z").output();
    let z = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    match z.as_deref() {
        Ok(z) if z.len() == 5 => {
            let secs = z[1..3].parse::<i64>().unwrap_or(0) * 3600
                + z[3..].parse::<i64>().unwrap_or(0) * 60;
            if z.starts_with('-') { -secs } else { secs }
        }
        _ => 0,
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
    for (id, d) in devices
        .iter()
        .filter(|(_, d)| WASHERS_AND_DRYERS.contains(&d.ident.kind.value_raw))
    {
        let levels: Result<FillingLevels> = get(&token, &format!("/devices/{id}/fillingLevels"));
        if let Some(line) = levels.ok().as_ref().and_then(twin_dos_line) {
            found = true;
            println!("{}\n  {line}", name(d));
        }
    }
    if !found {
        println!("Nothing to show: no coffee machine or TwinDos washer on this Miele account.");
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
    use super::consent_missing;

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
