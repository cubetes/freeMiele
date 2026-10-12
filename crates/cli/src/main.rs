use clap::{Parser, Subcommand};
use freemiele::dataact::{self, Appliance, Consumption, Refused, Run};
use freemiele::{
    Actions, CARE_SCHEDULE, COFFEE_MACHINE, Device, Devices, FillingLevels, NOT_LOGGED_IN, Program,
    Rejected, Request, Result, Session, care_line, choose, confirmed, get, name, put,
};
use serde_json::json;
use std::hash::{BuildHasher, RandomState};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

#[derive(Parser)]
#[command(name = "miele", about = "Control Miele appliances from the terminal")]
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
    /// How often your coffee machine has been cleaned, and the care schedule
    Care,
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
            freemiele::watch(&mut session, |lines| {
                for line in lines {
                    println!("{}  {line}", clock());
                }
            })
        }
        Cmd::Care => care(),
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
    if !found {
        println!("No coffee machine on this Miele account.");
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
    let key = if on { "powerOn" } else { "powerOff" };
    let request = if on { Request::On } else { Request::Off };
    let note = command(&token, id, "actions", json!({ key: true }), request)?;
    println!("{}: turning {}.", name(d), if on { "on" } else { "off" });
    if note {
        println!(
            "(Miele reported a server error, but the machine is {}.)",
            if on { "on" } else { "off" }
        );
    }
    Ok(())
}

/// Sends a command. Miele sometimes answers 5xx for one that worked, so after a 5xx we
/// re-read the machine. Ok(true) = it did what was asked despite the error.
fn command(
    token: &str,
    id: &str,
    what: &str,
    body: serde_json::Value,
    request: Request,
) -> Result<bool> {
    let path = format!("/devices/{id}");
    let Err(e) = put(token, &format!("{path}/{what}"), body) else {
        return Ok(false);
    };
    if !matches!(e.downcast_ref::<Rejected>(), Some(r) if r.0.is_server_error()) {
        return Err(e);
    }
    // lean: one fixed 2 s wait before re-reading; add a short retry if slow machines report false errors.
    std::thread::sleep(std::time::Duration::from_secs(2));
    match get::<Device>(token, &path) {
        Ok(d) if confirmed(request, &d.state) => Ok(true),
        _ => Err(e),
    }
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
    let note = command(
        &token,
        id,
        "programs",
        json!({ "programId": p.program_id }),
        Request::Brew,
    )?;
    println!("{}: making {}.", name(d), p.program);
    if note {
        println!("(Miele reported a server error, but the machine started the drink.)");
    }
    Ok(())
}

fn logout() -> Result<()> {
    let session = match Session::load() {
        Ok(s) => s,
        // No entry, or one too damaged to read: clear it. A refused Keychain access is reported.
        // lean: "no entry" is matched by message text; use an error enum if load gets more cases.
        Err(e) if e.is::<serde_json::Error>() || e.to_string() == NOT_LOGGED_IN => {
            Session::forget()?;
            println!("Not logged in.");
            return Ok(());
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
    println!("Logged out.");
    Ok(())
}

fn login() -> Result<()> {
    print!("Client ID: ");
    std::io::stdout().flush()?;
    let mut client_id = String::new();
    std::io::stdin().read_line(&mut client_id)?;
    let client_secret = rpassword::prompt_password("Client secret (hidden): ")?;

    // Random value that ties the browser's answer to this login attempt.
    let state = format!("{:x}", RandomState::new().hash_one(std::process::id()));
    let listener = TcpListener::bind("127.0.0.1:8765")?;
    let url = freemiele::login_url(client_id.trim(), &state);
    println!("Log in to Miele in your browser. If it doesn't open, visit:\n{url}");
    std::process::Command::new("open")
        .arg(url.as_str())
        .status()?;

    // Wait for the browser to come back with the login code; ignore stray requests (favicon etc).
    let code = loop {
        let (mut stream, _) = listener.accept()?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line)?;
        if !line.contains("state=") {
            stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")?;
            continue;
        }
        let code = freemiele::code_from_request(&line, &state);
        let msg = if code.is_some() {
            "Logged in. You can close this tab."
        } else {
            "Login failed. Check the terminal."
        };
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{msg}",
            msg.len()
        )?;
        break code.ok_or(format!(
            "Miele did not return a login code. Browser said: {}",
            line.trim()
        ))?;
    };

    Session::login(client_id.trim().to_string(), client_secret, &code)?.save()?;
    println!("Logged in. Try `miele status`.");
    Ok(())
}
