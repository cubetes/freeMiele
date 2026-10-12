//! The freeMiele menu-bar app: a tray icon and a panel with one card per appliance.
//! Read-only for now. All Miele logic lives in the `freemiele` core.

use freemiele::{COFFEE_MACHINE, Device, Devices, END, Session, activity};
use serde::Serialize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
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

fn card(d: &Device) -> Card {
    let s = &d.state;
    let status = s.status.value_raw;
    let (program, phase) = activity(d);
    let [h, m] = s.remaining_time;
    let left = (h + m > 0 && status != END).then(|| time_left(s.remaining_time));
    let mobile_off = s.remote_enable.full_remote_control && !s.remote_enable.mobile_start;
    let tone = match status {
        _ if s.signal_failure => "fault",
        1 | NOT_CONNECTED => "off",
        IN_USE => "running",
        _ if s.signal_info || mobile_off => "attention",
        _ => "ready",
    };
    let chip = match status {
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
    let title = match &left {
        Some(left) => format!("{base} · {left} left"),
        None => base,
    };
    let sub = if phase.is_some() || status == END {
        program
    } else if s.signal_failure {
        Some("Look at the appliance.".into())
    } else if s.signal_info {
        Some("Needs attention: water, beans, tray or care.".into())
    } else if mobile_off && status != 1 {
        Some("Mobile start is off. Turn it on at the machine.".into())
    } else {
        None
    };
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
    }
}

fn sorted(devices: &Devices) -> Vec<&Device> {
    let mut ids: Vec<_> = devices.keys().collect();
    ids.sort();
    ids.into_iter().map(|id| &devices[id]).collect()
}

/// Menu-bar text: minutes left of a running laundry programme, or the drink being made.
fn tray_title(devices: &Devices) -> String {
    sorted(devices)
        .into_iter()
        .filter(|d| d.state.status.value_raw == IN_USE)
        .find_map(|d| {
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
    }
}

fn watch(app: AppHandle) {
    match Session::load() {
        Ok(mut session) => freemiele::watch(&mut session, |devices, _| {
            let offline = devices
                .values()
                .all(|d| d.state.status.value_raw == NOT_CONNECTED);
            let cards = sorted(devices).into_iter().map(card).collect();
            show(&app, cards, tray_title(devices), offline);
        }),
        Err(e) => show(&app, vec![offline_card(e.to_string())], String::new(), true),
    }
}

#[tauri::command]
fn cards(panel: State<Panel>) -> Vec<Card> {
    panel.cards.lock().unwrap().clone()
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
        .invoke_handler(tauri::generate_handler![cards, fit, more])
        .on_menu_event(|app, event| {
            if event.id() == "quit" {
                app.exit(0);
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

    #[test]
    fn running_washer_shows_phase_time_and_progress() {
        let devices = washer();
        let c = card(sorted(&devices)[0]);
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
        let c = card(sorted(&devices)[0]);
        assert_eq!((c.title.as_str(), c.progress), ("Finished", None));
        assert_eq!(tray_title(&devices), "");
    }
}
