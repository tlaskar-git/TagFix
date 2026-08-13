// Settings: a settings.json next to the exe so the whole install stays
// portable. Missing or corrupt files fall back to defaults.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY: &str = "ctrl+shift+t";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub hotkey: String,
    pub output_dir: Option<String>,
    pub launch_at_login: bool,
    pub help_shown: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            hotkey: DEFAULT_HOTKEY.to_string(),
            output_dir: None,
            launch_at_login: false,
            help_shown: false,
        }
    }
}

pub fn settings_path(exe_dir: &Path) -> PathBuf {
    exe_dir.join("settings.json")
}

pub fn load(exe_dir: &Path) -> Settings {
    let path = settings_path(exe_dir);
    match std::fs::read_to_string(&path) {
        // Tolerate a UTF-8 BOM: hand-edited files often carry one.
        Ok(raw) => serde_json::from_str(raw.trim_start_matches('\u{feff}')).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save(exe_dir: &Path, settings: &Settings) -> io::Result<()> {
    crate::store::write_json_atomic(&settings_path(exe_dir), settings)
}

/// Where sweeps live: the configured output directory, or sweeps/ next to
/// the exe.
pub fn resolve_sweeps_dir(exe_dir: &Path, settings: &Settings) -> PathBuf {
    match &settings.output_dir {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => exe_dir.join("sweeps"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tagfix-set-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tmp_dir("missing");
        let s = load(&dir);
        assert_eq!(s, Settings::default());
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_gives_defaults() {
        let dir = tmp_dir("corrupt");
        std::fs::write(settings_path(&dir), "{not json").unwrap();
        assert_eq!(load(&dir), Settings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_round_trip() {
        let dir = tmp_dir("round");
        let s = Settings {
            hotkey: "ctrl+alt+f9".into(),
            output_dir: Some("D:\\sweeps".into()),
            launch_at_login: true,
            help_shown: true,
        };
        save(&dir, &s).unwrap();
        assert_eq!(load(&dir), s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_json_fills_defaults() {
        let dir = tmp_dir("partial");
        std::fs::write(settings_path(&dir), r#"{"launchAtLogin":true}"#).unwrap();
        let s = load(&dir);
        assert!(s.launch_at_login);
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
        assert_eq!(s.output_dir, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_tolerates_utf8_bom() {
        let dir = tmp_dir("bom");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"hotkey":"ctrl+alt+f9"}"#);
        std::fs::write(settings_path(&dir), bytes).unwrap();
        assert_eq!(load(&dir).hotkey, "ctrl+alt+f9");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweeps_dir_defaults_next_to_exe() {
        let dir = tmp_dir("swdir");
        let s = Settings::default();
        assert_eq!(resolve_sweeps_dir(&dir, &s), dir.join("sweeps"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweeps_dir_respects_override_and_ignores_blank() {
        let dir = tmp_dir("swdir2");
        let mut s = Settings::default();
        s.output_dir = Some("D:\\custom\\out".into());
        assert_eq!(
            resolve_sweeps_dir(&dir, &s),
            PathBuf::from("D:\\custom\\out")
        );
        s.output_dir = Some("   ".into());
        assert_eq!(resolve_sweeps_dir(&dir, &s), dir.join("sweeps"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
