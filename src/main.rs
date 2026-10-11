use clap::{Parser, Subcommand};
use freemiele::{Actions, Devices, Program, Result, Session, choose, get, name, put};
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
            let token = token()?;
            let devices: Devices = get(&token, "/devices")?;
            if devices.is_empty() {
                println!("No appliances on this Miele account.");
            }
            for (id, d) in &devices {
                println!("{}", freemiele::status_line(d));
                if d.ident.kind.value_raw == freemiele::COFFEE_MACHINE {
                    // A failed extra lookup must not hide the status line.
                    if let Ok(f) = get(&token, &format!("/devices/{id}/fillingLevels"))
                        && let Some(line) = freemiele::care_line(&f)
                    {
                        println!("{line}");
                    }
                }
            }
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
    put(
        &token,
        &format!("/devices/{id}/actions"),
        json!({ key: true }),
    )?;
    println!("{}: turning {}.", name(d), if on { "on" } else { "off" });
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
    put(
        &token,
        &format!("/devices/{id}/programs"),
        json!({ "programId": p.program_id }),
    )?;
    println!("{}: making {}.", name(d), p.program);
    Ok(())
}

fn logout() -> Result<()> {
    let Ok(session) = Session::load() else {
        Session::forget()?; // also clears an entry too damaged to read
        println!("Not logged in.");
        return Ok(());
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
