//! What the user chose, kept in a small `key=value` file.
//!
//! A file that is missing, unreadable or half written is never an error: the app starts
//! with the defaults and says nothing, because preferences are not worth a failed launch.

use std::path::{Path, PathBuf};

use crate::i18n::Choice;

/// How many recent cases the welcome screen lists.
pub const MAX_RECENT: usize = 8;

/// The light/dark choice of the Preferences dialog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    /// Follows the system.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];

    fn key(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }
}

/// The colours a field is drawn with in the viewer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColourScale {
    #[default]
    Rainbow,
    Viridis,
    Greys,
}

impl ColourScale {
    pub const ALL: [ColourScale; 3] = [
        ColourScale::Rainbow,
        ColourScale::Viridis,
        ColourScale::Greys,
    ];

    fn key(self) -> &'static str {
        match self {
            ColourScale::Rainbow => "rainbow",
            ColourScale::Viridis => "viridis",
            ColourScale::Greys => "greys",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }

    pub(crate) fn index(self) -> u8 {
        match self {
            ColourScale::Rainbow => 0,
            ColourScale::Viridis => 1,
            ColourScale::Greys => 2,
        }
    }

    pub(crate) fn from_index(index: u8) -> Self {
        Self::ALL
            .into_iter()
            .find(|c| c.index() == index)
            .unwrap_or_default()
    }
}

/// The interface scale, in percent, and the steps the Preferences offer.
pub const SCALE_MIN: u8 = 80;
pub const SCALE_MAX: u8 = 150;
pub const SCALE_STEP: u8 = 10;
/// The range of significant digits on the labels of the scale.
pub const DIGITS_MIN: u8 = 3;
pub const DIGITS_MAX: u8 = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub theme: Theme,
    /// The language of the interface.
    pub lang: Choice,
    /// The cases opened last, the newest first.
    pub recent: Vec<PathBuf>,
    /// Opens the case that was open when the app closed, at start.
    pub reopen_last: bool,
    /// The size of the interface, in percent.
    pub ui_scale: u8,
    /// The colours of a field in the viewer.
    pub colours: ColourScale,
    /// Significant digits on the labels of the colour scale.
    pub digits: u8,
    /// Whether the viewer starts with the cell edges drawn.
    pub cell_edges: bool,
    /// Looks for a new version when the app starts.
    pub check_updates: bool,
    /// The most threads a run uses; 0 is all the machine offers.
    pub threads: u8,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            lang: Choice::default(),
            recent: Vec::new(),
            reopen_last: false,
            ui_scale: 100,
            colours: ColourScale::default(),
            digits: crate::fieldview::DEFAULT_DIGITS,
            cell_edges: true,
            check_updates: true,
            threads: 0,
        }
    }
}

impl Prefs {
    pub fn parse(text: &str) -> Self {
        let mut prefs = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "theme" => {
                    if let Some(theme) = Theme::from_key(value.trim()) {
                        prefs.theme = theme;
                    }
                }
                "lang" => {
                    if let Some(lang) = Choice::from_key(value.trim()) {
                        prefs.lang = lang;
                    }
                }
                "reopen_last" => prefs.reopen_last = value.trim() == "true",
                "cell_edges" => prefs.cell_edges = value.trim() != "false",
                "check_updates" => prefs.check_updates = value.trim() != "false",
                "ui_scale" => {
                    if let Ok(n) = value.trim().parse::<u8>() {
                        prefs.ui_scale = n.clamp(SCALE_MIN, SCALE_MAX);
                    }
                }
                "threads" => {
                    if let Ok(n) = value.trim().parse::<u8>() {
                        prefs.threads = n;
                    }
                }
                "digits" => {
                    if let Ok(n) = value.trim().parse::<u8>() {
                        prefs.digits = n.clamp(DIGITS_MIN, DIGITS_MAX);
                    }
                }
                "colours" => {
                    if let Some(colours) = ColourScale::from_key(value.trim()) {
                        prefs.colours = colours;
                    }
                }
                "recent" if !value.is_empty() => {
                    prefs.remember(Path::new(value));
                }
                _ => {}
            }
        }
        // `remember` puts the newest first and the file lists the newest first.
        prefs.recent.reverse();
        prefs
    }

    pub fn to_text(&self) -> String {
        let mut text = format!(
            "theme={}\nlang={}\nreopen_last={}\nui_scale={}\ncolours={}\ndigits={}\ncell_edges={}\ncheck_updates={}\nthreads={}\n",
            self.theme.key(),
            self.lang.key(),
            self.reopen_last,
            self.ui_scale,
            self.colours.key(),
            self.digits,
            self.cell_edges,
            self.check_updates,
            self.threads,
        );
        for path in &self.recent {
            // A path with a line break could not be read back as one line.
            if let Some(path) = path.to_str().filter(|p| !p.contains(['\n', '\r'])) {
                text.push_str("recent=");
                text.push_str(path);
                text.push('\n');
            }
        }
        text
    }

    /// Puts a case at the top of the recent list.
    pub fn remember(&mut self, case: &Path) {
        self.recent.retain(|p| p != case);
        self.recent.insert(0, case.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    /// Writes the file through a temporary one, so a crash never leaves half of it.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("conf.tmp");
        std::fs::write(&tmp, self.to_text())?;
        std::fs::rename(&tmp, path)
    }
}

/// Where the file lives: the user's configuration folder of the platform.
pub fn default_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home?.join("Library/Application Support")
    } else {
        match std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
            Some(dir) if dir.is_absolute() => dir,
            _ => home?.join(".config"),
        }
    };
    Some(base.join("Vortia").join("rust-app.conf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_and_keep_the_order_of_the_recent_cases() {
        let mut prefs = Prefs {
            theme: Theme::Dark,
            lang: Choice::Spanish,
            ..Prefs::default()
        };
        prefs.remember(Path::new("/cases/a"));
        prefs.remember(Path::new("/cases/b"));
        prefs.remember(Path::new("/cases/a"));
        assert_eq!(
            prefs.recent,
            [PathBuf::from("/cases/a"), PathBuf::from("/cases/b")]
        );
        assert_eq!(Prefs::parse(&prefs.to_text()), prefs);
    }

    #[test]
    fn every_choice_survives_the_file() {
        let prefs = Prefs {
            reopen_last: true,
            ui_scale: 120,
            colours: ColourScale::Viridis,
            digits: 6,
            cell_edges: false,
            check_updates: false,
            threads: 4,
            ..Prefs::default()
        };
        assert_eq!(Prefs::parse(&prefs.to_text()), prefs);
    }

    #[test]
    fn numbers_out_of_range_are_brought_back_into_it() {
        let prefs = Prefs::parse("ui_scale=255\ndigits=0\n");
        assert_eq!((prefs.ui_scale, prefs.digits), (SCALE_MAX, DIGITS_MIN));
        let prefs = Prefs::parse("ui_scale=1\ndigits=99\n");
        assert_eq!((prefs.ui_scale, prefs.digits), (SCALE_MIN, DIGITS_MAX));
        // What is not a number leaves the default.
        assert_eq!(Prefs::parse("ui_scale=big\n").ui_scale, 100);
    }

    #[test]
    fn the_list_of_recent_cases_is_bounded() {
        let mut prefs = Prefs::default();
        for n in 0..(MAX_RECENT + 5) {
            prefs.remember(Path::new(&format!("/cases/{n}")));
        }
        assert_eq!(prefs.recent.len(), MAX_RECENT);
        assert_eq!(Prefs::parse(&prefs.to_text()).recent.len(), MAX_RECENT);
    }

    #[test]
    fn nonsense_in_the_file_falls_back_to_the_defaults() {
        let prefs = Prefs::parse("theme=neon\nno equals sign\nrecent=\n=\u{0}\n");
        assert_eq!(prefs, Prefs::default());
    }

    #[test]
    fn a_missing_file_is_the_defaults_and_saving_creates_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("deep/er/rust-app.conf");
        assert_eq!(Prefs::load_from(&file), Prefs::default());
        let mut prefs = Prefs {
            theme: Theme::Light,
            ..Prefs::default()
        };
        prefs.remember(Path::new("/cases/x"));
        prefs.save_to(&file).unwrap();
        assert_eq!(Prefs::load_from(&file), prefs);
    }

    #[test]
    fn a_path_with_a_line_break_is_not_written() {
        let mut prefs = Prefs::default();
        prefs.remember(Path::new("/cases/odd\nname"));
        assert!(!prefs.to_text().contains("odd"));
    }
}
