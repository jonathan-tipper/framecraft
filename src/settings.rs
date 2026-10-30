//! Local shortcut preferences. Validation is independent of native registration.
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShortcutSettings {
    pub capture: String,
    pub pause_resume: String,
    pub stop: String,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            capture: "Super+Alt+KeyS".into(),
            pause_resume: "Super+Alt+KeyP".into(),
            stop: "Super+Alt+KeyX".into(),
        }
    }
}

impl ShortcutSettings {
    pub fn validate(&self) -> Result<(), String> {
        self.canonical_keys().map(|_| ())
    }

    pub fn canonical_keys(&self) -> Result<[String; 3], String> {
        let keys = [
            canonical_key(&self.capture)?,
            canonical_key(&self.pause_resume)?,
            canonical_key(&self.stop)?,
        ];
        if keys[0] == keys[1] || keys[0] == keys[2] || keys[1] == keys[2] {
            return Err("Capture, Pause/Resume and Stop must use different shortcuts.".into());
        }
        for key in &keys {
            let reserved = match key.as_str() {
                "Super+KeyQ" => Some("Quit"),
                "Super+KeyO" => Some("Open image"),
                "Super+KeyS" => Some("Save PNG"),
                "Super+KeyZ" => Some("Undo"),
                "Super+Shift+KeyZ" => Some("Redo"),
                "Super+Shift+KeyC" => Some("Copy image"),
                "Super+Alt+Digit3" | "Super+Alt+Digit4" | "Super+Alt+Digit5" => {
                    Some("Capture once")
                }
                "Super+KeyH" | "Super+Alt+KeyH" => Some("the macOS Hide menu"),
                _ => None,
            };
            if let Some(action) = reserved {
                return Err(format!("{key} is reserved for {action}. Choose another shortcut, for example Super+Alt+Shift+KeyS."));
            }
        }
        Ok(keys)
    }

    pub fn path() -> Result<PathBuf, String> {
        let home = std::env::var_os("HOME").ok_or("Could not locate your settings folder.")?;
        Ok(PathBuf::from(home).join("Library/Application Support/Framecraft/shortcuts.json"))
    }

    pub fn load() -> Result<Self, String> {
        Self::load_from(&Self::path()?)
    }

    pub fn load_from(path: &Path) -> Result<Self, String> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(error) => return Err(format!("Could not read shortcut settings: {error}")),
        };
        if bytes.len() > 4096 {
            return Err("Shortcut settings are too large. Reset to defaults.".into());
        }
        let result: Self = serde_json::from_slice(&bytes).map_err(|error| {
            format!("Could not read shortcut settings: {error}. Reset to defaults.")
        })?;
        result.validate()?;
        Ok(result)
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::path()?)
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let parent = path.parent().ok_or("Settings path has no parent folder.")?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create settings folder: {error}"))?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
        temporary
            .write_all(&bytes)
            .map_err(|error| error.to_string())?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        temporary
            .persist(path)
            .map_err(|error| format!("Could not save shortcut settings: {error}"))?;
        Ok(())
    }
}

fn canonical_key(value: &str) -> Result<String, String> {
    let mut modifiers = [false; 4];
    let tokens: Vec<_> = value.split('+').map(str::trim).collect();
    if tokens.len() < 2 {
        return Err("Use at least one modifier and a key, for example Super+Alt+KeyS.".into());
    }
    for token in &tokens[..tokens.len() - 1] {
        let index = match token.to_ascii_lowercase().as_str() {
            "super" | "cmd" | "command" => 0,
            "alt" | "option" => 1,
            "control" | "ctrl" => 2,
            "shift" => 3,
            _ => {
                return Err(format!(
                    "Unknown shortcut modifier: {token}. Use Super, Alt, Control or Shift."
                ))
            }
        };
        if modifiers[index] {
            return Err(format!("Shortcut repeats modifier {token}."));
        }
        modifiers[index] = true;
    }
    let key = tokens.last().unwrap().to_ascii_uppercase();
    let key = if key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic() {
        format!("Key{key}")
    } else if key.len() == 4 && key.starts_with("KEY") && key.as_bytes()[3].is_ascii_alphabetic() {
        format!("Key{}", &key[3..])
    } else if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() {
        format!("Digit{key}")
    } else if key.len() == 6 && key.starts_with("DIGIT") && key.as_bytes()[5].is_ascii_digit() {
        format!("Digit{}", &key[5..])
    } else if key
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n))
    {
        key
    } else {
        return Err("Choose a letter, digit or function key (F1–F24).".into());
    };
    let mut result = ["Super", "Alt", "Control", "Shift"]
        .into_iter()
        .zip(modifiers)
        .filter_map(|(name, enabled)| enabled.then_some(name))
        .collect::<Vec<_>>()
        .join("+");
    result.push('+');
    result.push_str(&key);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn app_shortcuts_cannot_be_remapped_even_through_aliases() {
        for reserved in [
            "Command+q",
            "cmd+o",
            "Super+KeyS",
            "command+z",
            "Shift+Command+z",
            "command+shift+c",
            "option+command+3",
            "Super+Alt+Digit4",
            "COMMAND+OPTION+5",
        ] {
            let preferences = ShortcutSettings {
                capture: reserved.into(),
                ..Default::default()
            };
            assert!(
                preferences.validate().unwrap_err().contains("reserved"),
                "{reserved}"
            );
        }
        ShortcutSettings::default().validate().unwrap();
    }
    #[test]
    fn shortcut_persistence_reset_and_corruption_are_explicit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prefs.json");
        assert_eq!(
            ShortcutSettings::load_from(&path).unwrap(),
            ShortcutSettings::default()
        );
        let changed = ShortcutSettings {
            capture: "Control+Shift+F9".into(),
            ..Default::default()
        };
        changed.save_to(&path).unwrap();
        assert_eq!(ShortcutSettings::load_from(&path).unwrap(), changed);
        ShortcutSettings::default().save_to(&path).unwrap();
        assert_eq!(
            ShortcutSettings::load_from(&path).unwrap(),
            ShortcutSettings::default()
        );
        std::fs::write(&path, b"bad data").unwrap();
        assert!(ShortcutSettings::load_from(&path).is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
    #[test]
    fn equivalent_modifier_order_and_aliases_cannot_bypass_duplicates() {
        let settings = ShortcutSettings {
            capture: "alt+Command+s".into(),
            pause_resume: "Super+Alt+KeyS".into(),
            ..Default::default()
        };
        assert!(settings.validate().unwrap_err().contains("different"));
        assert!(canonical_key("KeyS").is_err());
        assert!(canonical_key("Super+Super+KeyS").is_err());
        assert!(canonical_key("Super+").is_err());
        assert!(canonical_key("Super+F25").is_err());
        assert_eq!(
            canonical_key("option+command+9").unwrap(),
            "Super+Alt+Digit9"
        );
    }
}
