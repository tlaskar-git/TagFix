// Settings: a settings.json next to the exe so the whole install stays
// portable. Missing or corrupt files fall back to defaults.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY: &str = "ctrl+shift+t";
pub const DEFAULT_QUOTE_HOTKEY: &str = "ctrl+shift+q";
pub const DEFAULT_ATTACH_HOTKEY: &str = "ctrl+shift+a";

/// Which product a tag is about. `hosts` are matched against the URL host
/// read from the browser; `export_dir` is where a copy of that target's
/// evidence is written at export time, when it is set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Target {
    pub name: String,
    pub hosts: Vec<String>,
    pub export_dir: Option<String>,
}

impl Target {
    fn new(name: &str, hosts: &[&str]) -> Self {
        Target {
            name: name.to_string(),
            hosts: hosts.iter().map(|h| h.to_string()).collect(),
            export_dir: None,
        }
    }
}

/// The three products this machine sweeps. Editable in the settings window.
pub fn default_targets() -> Vec<Target> {
    vec![
        Target::new("helmsly", &["on.slobal.com", "localhost", "127.0.0.1"]),
        Target::new("slobal.com", &["slobal.com", "www.slobal.com"]),
        Target::new("AgnCred", &["agncred.com", "www.agncred.com"]),
    ]
}

/// Which target a URL host belongs to.
///
/// A host matches a pattern when it is that pattern exactly or a subdomain
/// of it, so a tenant at acme.on.slobal.com maps to helmsly. Both
/// on.slobal.com and slobal.com match that host, so the longest pattern
/// wins: without that rule the tenant would be filed under slobal.com. The
/// dot boundary is what keeps notslobal.com from matching slobal.com.
pub fn target_for_host<'a>(targets: &'a [Target], host: &str) -> Option<&'a str> {
    let host = normalize_host(host);
    if host.is_empty() {
        return None;
    }
    let mut best: Option<(usize, &str)> = None;
    for target in targets {
        for pattern in &target.hosts {
            let pattern = normalize_host(pattern);
            if pattern.is_empty() {
                continue;
            }
            let hit = host == pattern || host.ends_with(&format!(".{}", pattern));
            if !hit {
                continue;
            }
            if best.map(|(len, _)| pattern.len() > len).unwrap_or(true) {
                best = Some((pattern.len(), target.name.as_str()));
            }
        }
    }
    best.map(|(_, name)| name)
}

/// Lowercase, drop any port and any trailing root dot. Callers hand us
/// whatever the address bar held.
fn normalize_host(host: &str) -> String {
    let host = host.trim().to_lowercase();
    let host = host.split('/').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    host.trim_end_matches('.').to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub hotkey: String,
    pub output_dir: Option<String>,
    pub launch_at_login: bool,
    pub help_shown: bool,
    // Round 02 additions. The serde container default keeps a settings.json
    // written by 0.2.x loading with these filled in.
    pub show_pen_chip: bool,
    pub quote_screenshot: bool,
    pub quote_hotkey: String,
    pub attach_hotkey: String,
    pub context_frame: bool,
    pub new_sweep_each_day: bool,
    pub targets: Vec<Target>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            hotkey: DEFAULT_HOTKEY.to_string(),
            output_dir: None,
            launch_at_login: false,
            help_shown: false,
            show_pen_chip: true,
            quote_screenshot: false,
            quote_hotkey: DEFAULT_QUOTE_HOTKEY.to_string(),
            attach_hotkey: DEFAULT_ATTACH_HOTKEY.to_string(),
            context_frame: true,
            new_sweep_each_day: false,
            targets: default_targets(),
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
        let mut targets = default_targets();
        targets[0].export_dir = Some("D:\\AI\\Helmsly".into());
        let s = Settings {
            hotkey: "ctrl+alt+f9".into(),
            output_dir: Some("D:\\sweeps".into()),
            launch_at_login: true,
            help_shown: true,
            show_pen_chip: false,
            quote_screenshot: true,
            quote_hotkey: "ctrl+alt+q".into(),
            attach_hotkey: "ctrl+alt+a".into(),
            context_frame: false,
            new_sweep_each_day: true,
            targets,
        };
        save(&dir, &s).unwrap();
        assert_eq!(load(&dir), s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_02_defaults_match_the_brief() {
        let s = Settings::default();
        assert!(s.show_pen_chip);
        assert!(!s.quote_screenshot);
        assert_eq!(s.quote_hotkey, "ctrl+shift+q");
        assert_eq!(s.attach_hotkey, "ctrl+shift+a");
        assert!(s.context_frame);
        assert!(!s.new_sweep_each_day);
        let names: Vec<&str> = s.targets.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["helmsly", "slobal.com", "AgnCred"]);
        assert!(s.targets.iter().all(|t| t.export_dir.is_none()));
        assert_eq!(
            s.targets[0].hosts,
            vec!["on.slobal.com", "localhost", "127.0.0.1"]
        );
    }

    #[test]
    fn partial_json_fills_defaults() {
        let dir = tmp_dir("partial");
        std::fs::write(settings_path(&dir), r#"{"launchAtLogin":true}"#).unwrap();
        let s = load(&dir);
        assert!(s.launch_at_login);
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
        assert_eq!(s.output_dir, None);
        // A 0.2.x settings.json knows nothing about round 02.
        assert!(s.show_pen_chip);
        assert!(s.context_frame);
        assert_eq!(s.quote_hotkey, DEFAULT_QUOTE_HOTKEY);
        assert_eq!(s.attach_hotkey, DEFAULT_ATTACH_HOTKEY);
        assert_eq!(s.targets, default_targets());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_json_keeps_an_explicit_empty_target_list() {
        let dir = tmp_dir("partial-targets");
        std::fs::write(settings_path(&dir), r#"{"targets":[]}"#).unwrap();
        // An operator who cleared every target row means it.
        assert!(load(&dir).targets.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn host_matches_exactly_and_by_subdomain() {
        let t = default_targets();
        assert_eq!(target_for_host(&t, "on.slobal.com"), Some("helmsly"));
        assert_eq!(target_for_host(&t, "acme.on.slobal.com"), Some("helmsly"));
        assert_eq!(target_for_host(&t, "localhost"), Some("helmsly"));
        assert_eq!(target_for_host(&t, "127.0.0.1"), Some("helmsly"));
        assert_eq!(target_for_host(&t, "slobal.com"), Some("slobal.com"));
        assert_eq!(target_for_host(&t, "www.slobal.com"), Some("slobal.com"));
        assert_eq!(target_for_host(&t, "agncred.com"), Some("AgnCred"));
        assert_eq!(target_for_host(&t, "www.agncred.com"), Some("AgnCred"));
    }

    #[test]
    fn the_longest_matching_host_wins() {
        // A tenant subdomain matches both on.slobal.com and slobal.com; it
        // must land on helmsly, not on the site.
        let t = default_targets();
        assert_eq!(target_for_host(&t, "tenant.on.slobal.com"), Some("helmsly"));
        assert_eq!(
            target_for_host(&t, "deep.tenant.on.slobal.com"),
            Some("helmsly")
        );
    }

    #[test]
    fn host_matching_respects_the_dot_boundary() {
        let t = default_targets();
        assert_eq!(target_for_host(&t, "notslobal.com"), None);
        assert_eq!(target_for_host(&t, "myagncred.com"), None);
        assert_eq!(target_for_host(&t, "slobal.com.evil.test"), None);
    }

    #[test]
    fn host_matching_normalizes_case_and_port() {
        let t = default_targets();
        assert_eq!(target_for_host(&t, "WWW.Slobal.COM"), Some("slobal.com"));
        assert_eq!(target_for_host(&t, "localhost:5173"), Some("helmsly"));
        assert_eq!(target_for_host(&t, "slobal.com."), Some("slobal.com"));
        assert_eq!(target_for_host(&t, "  slobal.com  "), Some("slobal.com"));
    }

    #[test]
    fn unknown_and_empty_hosts_have_no_target() {
        let t = default_targets();
        assert_eq!(target_for_host(&t, "example.com"), None);
        assert_eq!(target_for_host(&t, ""), None);
        assert_eq!(target_for_host(&t, "   "), None);
        assert_eq!(target_for_host(&[], "slobal.com"), None);
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
