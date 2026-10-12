//! What the user chose in the Settings window, kept as a small JSON file in the app's config folder.

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MAX_FAVOURITES: usize = 5;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    /// Drink names, in the order the panel shows them.
    pub favourites: Vec<String>,
    /// Show minutes left of a running programme in the menu bar.
    pub time_left: bool,
    /// Notify and show a dot on the menu-bar icon when the washer finishes.
    pub washer_alert: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            favourites: [
                "Espresso",
                "Coffee",
                "Cappuccino",
                "Flat white",
                "Latte macchiato",
            ]
            .map(String::from)
            .to_vec(),
            time_left: true,
            washer_alert: true,
        }
    }
}

impl Settings {
    /// The saved settings; the defaults if the file is missing or damaged.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str::<Self>(&json).ok())
            .unwrap_or_default()
            .tidy()
    }

    /// No blank or repeated drinks, at most five.
    pub fn tidy(mut self) -> Self {
        let mut seen = Vec::new();
        self.favourites.retain(|n| {
            let key = n.trim().to_lowercase();
            !key.is_empty() && !seen.contains(&key) && {
                seen.push(key);
                true
            }
        });
        self.favourites.truncate(MAX_FAVOURITES);
        self
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_damaged_file_gives_the_defaults() {
        let dir = std::env::temp_dir().join(format!("freemiele-settings-{}", std::process::id()));
        let file = dir.join("settings.json");
        assert_eq!(Settings::load(&file), Settings::default());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, "{not json").unwrap();
        assert_eq!(Settings::load(&file), Settings::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saved_settings_come_back_and_missing_fields_use_defaults() {
        let dir = std::env::temp_dir().join(format!("freemiele-saved-{}", std::process::id()));
        let file = dir.join("nested/settings.json");
        let mine = Settings {
            favourites: vec!["Latte macchiato".into(), "Espresso".into()],
            time_left: false,
            washer_alert: true,
        };
        mine.save(&file).unwrap();
        assert_eq!(Settings::load(&file), mine);
        std::fs::write(&file, r#"{"time_left": false}"#).unwrap();
        let loaded = Settings::load(&file);
        assert!(!loaded.time_left);
        assert_eq!(loaded.favourites, Settings::default().favourites);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tidy_drops_blanks_and_repeats_and_keeps_five() {
        let s = Settings {
            favourites: ["Espresso", " ", "espresso", "A", "B", "C", "D", "E"]
                .map(String::from)
                .to_vec(),
            ..Settings::default()
        }
        .tidy();
        assert_eq!(s.favourites, ["Espresso", "A", "B", "C", "D"]);
    }
}
