//! The freeMiele menu-bar app: a tray icon and a panel with one card per appliance.
//! The coffee card can brew and switch the machine on or off. All Miele logic lives in the
//! `freemiele` core.

use freemiele::{
    COFFEE_MACHINE, Device, Devices, END, Outcome, Program, Rejected, Session, activity,
    find_program, get,
};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Rect, State, WebviewWindow,
    WindowEvent,
};

const IN_USE: i64 = 5;
const NOT_CONNECTED: i64 = 255;

/// What the page draws for one appliance.
#[derive(Serialize, Clone, PartialEq, Debug)]
struct Card {
    /// "Coffee machine · CM6160"
    label: String,
    /// ready, running, attention, fault or off: picks the chip colour
    tone: &'static str,
    chip: String,
    title: String,
    sub: Option<String>,
    /// Percent done, for appliances that report elapsed time.
    progress: Option<u8>,
    /// Set when the page should draw the coffee controls.
    coffee: Option<Coffee>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
struct Coffee {
    id: String,
    on: bool,
    mobile_start: bool,
}

/// A drink the machine can make: its program id and the name to show.
#[derive(Serialize, Deserialize)]
struct Drink {
    id: u32,
    name: String,
}

#[derive(Serialize)]
struct Drinks {
    favourites: Vec<Drink>,
    /// Everything the machine can make, for the "Other drink" menu.
    all: Vec<Drink>,
}

// lean: fixed list; the Settings window (#11) makes it editable.
const FAVOURITES: [&str; 5] = [
    "Espresso",
    "Coffee",
    "Cappuccino",
    "Flat white",
    "Latte macchiato",
];

fn drinks_of(programs: &[Program]) -> Drinks {
    let favourites = FAVOURITES
        .iter()
        .filter_map(|n| {
            find_program(programs, n).map(|p| Drink {
                id: p.program_id,
                name: n.to_string(),
            })
        })
        .collect();
    let all = programs
        .iter()
        .filter(|p| !p.program.is_empty()) // a nameless program must never be offered
        .map(|p| {
            let mut chars = p.program.chars();
            let first = chars.next().map(|c| c.to_uppercase().to_string());
            Drink {
                id: p.program_id,
                name: first.unwrap_or_default() + chars.as_str(),
            }
        })
        .collect();
    Drinks { favourites, all }
}

#[derive(Default)]
struct Panel {
    cards: Mutex<Vec<Card>>,
    /// The "…" menu is open: the panel loses focus but must stay.
    menu_open: AtomicBool,
    /// When the panel last hid itself, so the click on the icon that caused it doesn't reopen it.
    hidden_at: Mutex<Option<Instant>>,
}

fn time_left([h, m]: [u32; 2]) -> String {
    match h {
        0 => format!("{m} min"),
        _ => format!("{h} h {m:02} min"),
    }
}

fn card(id: &str, d: &Device) -> Card {
    let s = &d.state;
    let status = s.status.value_raw;
    let (program, phase) = activity(d);
    let [h, m] = s.remaining_time;
    let left = (h + m > 0 && status != END).then(|| time_left(s.remaining_time));
    let mobile_off = s.remote_enable.full_remote_control && !s.remote_enable.mobile_start;
    let attention = freemiele::needs_attention(d);
    let tone = match status {
        _ if s.signal_failure => "fault",
        1 | NOT_CONNECTED => "off",
        IN_USE => "running",
        _ if attention || mobile_off => "attention",
        _ => "ready",
    };
    let mut chip = match status {
        END => "Finished".to_string(),
        _ => s.status.value_localized.clone(),
    };
    let base = match status {
        END => "Finished".to_string(),
        _ => phase
            .clone()
            .or_else(|| program.clone())
            .unwrap_or_else(|| s.status.value_localized.clone()),
    };
    let mut title = match &left {
        Some(left) => format!("{base} · {left} left"),
        None => base,
    };
    let mut sub = if phase.is_some() || status == END {
        program
    } else if s.signal_failure {
        Some("Look at the appliance.".into())
    } else if attention {
        Some("Needs attention: check the machine's display.".into())
    } else if mobile_off && status != 1 {
        Some("Mobile start is off. Turn it on at the machine.".into())
    } else {
        None
    };
    // Controls unless a drink is being made or the machine has a fault.
    let coffee = (d.ident.kind.value_raw == COFFEE_MACHINE
        && !matches!(status, IN_USE | NOT_CONNECTED)
        && !s.signal_failure)
        .then(|| Coffee {
            id: id.to_string(),
            on: status != 1,
            mobile_start: s.remote_enable.mobile_start,
        });
    if let Some(c) = &coffee {
        let (t, hint) = match (c.on, c.mobile_start) {
            (false, _) => ("Off", "Turn the machine on to make a drink."),
            (true, true) => (
                "Mobile Start is on",
                "Pick a drink. The machine makes one, then Mobile Start turns off.",
            ),
            (true, false) => (
                "Turn on Mobile Start",
                "Brew unlocks by itself a few seconds later. You need Mobile Start again for every drink.",
            ),
        };
        title = t.into();
        if !(c.on && attention) {
            sub = Some(hint.into()); // "needs attention" outranks the hint
        }
        if c.on && !c.mobile_start {
            chip = "Mobile Start off".into();
        }
    }
    let done = match s.elapsed_time[..] {
        [eh, em] => eh * 60 + em,
        _ => 0,
    };
    let total = done + h * 60 + m;
    let progress = (status == IN_USE && d.ident.kind.value_raw != COFFEE_MACHINE && total > 0)
        .then(|| (done * 100 / total) as u8);
    Card {
        label: format!(
            "{} · {}",
            d.ident.kind.value_localized, d.ident.device_ident_label.tech_type
        ),
        tone,
        chip,
        title,
        sub,
        progress,
        coffee,
    }
}

fn sorted(devices: &Devices) -> Vec<(&String, &Device)> {
    let mut ids: Vec<_> = devices.keys().collect();
    ids.sort();
    ids.into_iter().map(|id| (id, &devices[id])).collect()
}

/// Menu-bar text: minutes left of a running laundry programme, or the drink being made.
fn tray_title(devices: &Devices) -> String {
    sorted(devices)
        .into_iter()
        .filter(|(_, d)| d.state.status.value_raw == IN_USE)
        .find_map(|(_, d)| {
            if d.ident.kind.value_raw == COFFEE_MACHINE {
                activity(d).0
            } else {
                let t = d.state.remaining_time;
                (t[0] + t[1] > 0).then(|| time_left(t))
            }
        })
        .unwrap_or_default()
}

fn show(app: &AppHandle, cards: Vec<Card>, title: String, offline: bool) {
    let icon = if offline {
        include_bytes!("../icons/tray-off.png").as_slice()
    } else {
        include_bytes!("../icons/tray.png").as_slice()
    };
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_title(Some(title));
        if let Ok(icon) = Image::from_bytes(icon) {
            let _ = tray.set_icon(Some(icon));
            let _ = tray.set_icon_as_template(true);
        }
    }
    let _ = app.emit("cards", &cards);
    *app.state::<Panel>().cards.lock().unwrap() = cards;
}

fn offline_card(message: String) -> Card {
    Card {
        label: "freeMiele".into(),
        tone: "attention",
        chip: "Login needed".into(),
        title: "Can't reach Miele".into(),
        sub: Some(message),
        progress: None,
        coffee: None,
    }
}

fn watch(app: AppHandle) {
    match Session::load() {
        Ok(mut session) => freemiele::watch(&mut session, |devices, _| {
            let offline = devices
                .values()
                .all(|d| d.state.status.value_raw == NOT_CONNECTED);
            let cards = sorted(devices)
                .into_iter()
                .map(|(id, d)| card(id, d))
                .collect();
            show(&app, cards, tray_title(devices), offline);
        }),
        Err(e) => show(&app, vec![offline_card(e.to_string())], String::new(), true),
    }
}

#[tauri::command]
fn cards(panel: State<Panel>) -> Vec<Card> {
    panel.cards.lock().unwrap().clone()
}

/// Miele's refusal in its own words (its reply is JSON with a "message"), else the error as is.
fn plain(e: Box<dyn std::error::Error>) -> String {
    let message = e.downcast_ref::<Rejected>().and_then(|r| {
        let reply: serde_json::Value = serde_json::from_str(&r.1).ok()?;
        reply["message"].as_str().map(String::from)
    });
    message.unwrap_or_else(|| e.to_string())
}

/// Runs a Miele request off the main thread, with a fresh token. Errors come back as plain text.
async fn miele<T: Send + 'static>(
    f: impl FnOnce(&str) -> freemiele::Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Session::load()
            .and_then(|mut s| s.access_token())
            .and_then(|token| f(&token))
            .map_err(plain)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The machine's drinks, favourites first. Empty while the machine is off.
#[tauri::command]
async fn drinks(device_id: String) -> Result<Drinks, String> {
    miele(move |token| {
        let programs: Vec<Program> = get(token, &format!("/devices/{device_id}/programs"))?;
        Ok(drinks_of(&programs))
    })
    .await
}

/// What the card says once the machine has (or has not) shown the change.
fn said(outcome: Outcome, done: &str, failed: &str) -> String {
    match outcome {
        Outcome::Done => format!("{done}."),
        Outcome::DoneDespiteServerError => {
            format!("{done}. Miele reported a server error, but the machine did it.")
        }
        Outcome::NotConfirmed => failed.into(),
    }
}

/// Start a drink. Only the page's Brew button calls this. Returns the note for the card.
#[tauri::command]
async fn brew(device_id: String, program_id: u32, drink: String) -> Result<String, String> {
    miele(move |token| {
        let outcome = freemiele::brew(token, &device_id, program_id)?;
        Ok(said(
            outcome,
            &format!("Making {drink}"),
            "The machine didn't start the drink. Check its display: water tank, beans or drip tray.",
        ))
    })
    .await
}

#[tauri::command]
async fn power(device_id: String, on: bool) -> Result<String, String> {
    miele(move |token| {
        let word = if on { "on" } else { "off" };
        let outcome = freemiele::power(token, &device_id, on)?;
        Ok(said(
            outcome,
            &format!("Turned {word}"),
            &format!("The machine didn't turn {word}. Check its display."),
        ))
    })
    .await
}

/// "Other drink": a native menu under the pointer. The pick comes back as a "drink" event.
#[tauri::command]
fn other_drinks(
    window: WebviewWindow,
    app: AppHandle,
    panel: State<Panel>,
    drinks: Vec<Drink>,
) -> tauri::Result<()> {
    let menu = Menu::new(&app)?;
    for d in &drinks {
        menu.append(&MenuItemBuilder::with_id(format!("drink:{}", d.id), &d.name).build(&app)?)?;
    }
    panel.menu_open.store(true, Ordering::SeqCst);
    let shown = window.popup_menu(&menu);
    panel.menu_open.store(false, Ordering::SeqCst);
    shown
}

/// The page tells us how tall its content is.
#[tauri::command]
fn fit(window: WebviewWindow, height: f64) {
    let _ = window.set_size(LogicalSize::new(340.0, height));
}

/// The "…" button: a native menu under the pointer.
#[tauri::command]
fn more(window: WebviewWindow, app: AppHandle, panel: State<Panel>) -> tauri::Result<()> {
    let quit = MenuItem::with_id(&app, "quit", "Quit freeMiele", true, Some("Cmd+Q"))?;
    panel.menu_open.store(true, Ordering::SeqCst);
    let shown = window.popup_menu(&Menu::with_items(&app, &[&quit])?);
    panel.menu_open.store(false, Ordering::SeqCst);
    shown
}

/// Open the panel centred under the tray icon, or close it.
fn toggle(app: &AppHandle, icon: Rect) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    let just_hidden = app
        .state::<Panel>()
        .hidden_at
        .lock()
        .unwrap()
        .is_some_and(|t| t.elapsed() < Duration::from_millis(300));
    if just_hidden {
        return;
    }
    let scale = window.scale_factor().unwrap_or(2.0);
    let (at, size) = (
        icon.position.to_physical::<f64>(scale),
        icon.size.to_physical::<f64>(scale),
    );
    let width = window.outer_size().map_or(680.0, |s| s.width as f64);
    let mut x = at.x + size.width / 2.0 - width / 2.0;
    // Keep the whole panel on the screen the icon is on.
    if let Ok(Some(screen)) = app.monitor_from_point(at.x, at.y) {
        let right = screen.position().x as f64 + screen.size().width as f64;
        x = x.min(right - width - 8.0 * scale);
    }
    let _ = window.set_position(PhysicalPosition::new(x, at.y + size.height + 4.0 * scale));
    let _ = window.show();
    let _ = window.set_focus();
}

/// Liquid Glass on macOS 26 and later, the standard popover material before that.
#[cfg(target_os = "macos")]
fn glass(window: &WebviewWindow) {
    use window_vibrancy::{
        LiquidGlassOptions, NSGlassEffectViewStyle, NSVisualEffectMaterial, NSVisualEffectState,
        apply_liquid_glass, apply_vibrancy,
    };
    let options = LiquidGlassOptions::new(NSGlassEffectViewStyle::Regular).radius(20.0);
    if apply_liquid_glass(window, options).is_err() {
        let _ = apply_vibrancy(
            window,
            NSVisualEffectMaterial::Popover,
            Some(NSVisualEffectState::Active),
            Some(20.0),
        );
    }
}

fn main() {
    tauri::Builder::default()
        .manage(Panel::default())
        .invoke_handler(tauri::generate_handler![
            cards,
            fit,
            more,
            drinks,
            brew,
            power,
            other_drinks
        ])
        .on_menu_event(|app, event| {
            if event.id() == "quit" {
                app.exit(0);
            } else if let Some(id) = event.id().0.strip_prefix("drink:") {
                let _ = app.emit("drink", id);
            }
        })
        .setup(|app| {
            // A menu-bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let window = app
                .get_webview_window("main")
                .expect("window in tauri.conf.json");
            #[cfg(target_os = "macos")]
            glass(&window);
            let handle = app.handle().clone();
            window.on_window_event(move |event| {
                if let WindowEvent::Focused(false) = event {
                    let panel = handle.state::<Panel>();
                    if !panel.menu_open.load(Ordering::SeqCst) {
                        *panel.hidden_at.lock().unwrap() = Some(Instant::now());
                        let _ = handle.get_webview_window("main").map(|w| w.hide());
                    }
                }
            });
            TrayIconBuilder::with_id("main")
                .icon(Image::from_bytes(include_bytes!("../icons/tray-off.png"))?)
                .icon_as_template(true)
                .tooltip("freeMiele")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        toggle(tray.app_handle(), rect);
                    }
                })
                .build(app)?;
            let handle = app.handle().clone();
            std::thread::spawn(move || watch(handle));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running freeMiele");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn washer() -> Devices {
        serde_json::from_str(include_str!(
            "../../../fixtures/devices.washer-running.sample.json"
        ))
        .unwrap()
    }

    fn card_of(devices: &Devices) -> Card {
        let (id, d) = sorted(devices)[0];
        card(id, d)
    }

    fn coffee() -> Devices {
        serde_json::from_str(include_str!("../../../fixtures/devices.sample.json")).unwrap()
    }

    #[test]
    fn off_machine_offers_turn_on_only() {
        let c = card_of(&coffee());
        assert_eq!((c.tone, c.title.as_str()), ("off", "Off"));
        assert_eq!(
            c.sub.as_deref(),
            Some("Turn the machine on to make a drink.")
        );
        assert!(matches!(c.coffee, Some(Coffee { on: false, .. })));
    }

    #[test]
    fn mobile_start_decides_the_title_and_chip() {
        let mut devices = coffee();
        let d = devices.values_mut().next().unwrap();
        d.state.status.value_raw = 2;
        d.state.remote_enable.mobile_start = false;
        d.state.signal_info = false;
        let c = card_of(&devices);
        assert_eq!(
            (c.tone, c.title.as_str()),
            ("attention", "Turn on Mobile Start")
        );
        assert_eq!(c.chip, "Mobile Start off");
        let d = devices.values_mut().next().unwrap();
        d.state.remote_enable.mobile_start = true;
        let c = card_of(&devices);
        assert_eq!(c.title, "Mobile Start is on");
        assert!(matches!(
            c.coffee,
            Some(Coffee {
                on: true,
                mobile_start: true,
                ..
            })
        ));
    }

    #[test]
    fn brewing_machine_has_no_controls() {
        let mut devices = coffee();
        devices.values_mut().next().unwrap().state.status.value_raw = IN_USE;
        assert_eq!(card_of(&devices).coffee, None);
    }

    #[test]
    fn favourites_match_the_machines_names_and_all_hides_nameless() {
        let programs: Vec<Program> =
            serde_json::from_str(include_str!("../../../fixtures/programs.sample.json")).unwrap();
        let d = drinks_of(&programs);
        let names: Vec<_> = d.favourites.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Espresso",
                "Coffee",
                "Cappuccino",
                "Flat white",
                "Latte macchiato"
            ]
        );
        assert_eq!(d.all[1].name, "Espresso");
        assert_eq!(d.all.len(), programs.len() - 1);
        assert!(d.all.iter().all(|d| d.id != 24050));
    }

    #[test]
    fn running_washer_shows_phase_time_and_progress() {
        let devices = washer();
        let c = card_of(&devices);
        assert_eq!(c.tone, "running");
        assert_eq!(c.title, "Main wash · 23 min left");
        assert_eq!(c.sub.as_deref(), Some("Sportswear"));
        assert_eq!(c.progress, Some(62)); // 38 min done, 23 left
        assert_eq!(tray_title(&devices), "23 min");
    }

    #[test]
    fn finished_washer_says_finished() {
        let mut devices = washer();
        let d = devices.values_mut().next().unwrap();
        d.state.status.value_raw = END;
        d.state.remaining_time = [0, 0];
        let c = card_of(&devices);
        assert_eq!((c.title.as_str(), c.progress), ("Finished", None));
        assert_eq!(tray_title(&devices), "");
    }
}
