//! The window: Nodalia islands on the window surface, a header with the hamburger menu, and
//! two screens, the welcome and the case.

use crate::i18n::{Choice, tr, trf};
use std::collections::{HashSet, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui::{
    self, Align, Color32, Context, CornerRadius, Frame, Id, Key, KeyboardShortcut, Layout, Margin,
    Modifiers, RichText, Sense, Stroke, Vec2, ViewportBuilder, ViewportCommand,
};
use nodalia_theme::{
    Accent, Mode, Palette, component, engineering, font_size, for_egui, icons, radius, space,
};

use crate::conditions::{self, Loaded};
use crate::controldict::{self, SettingsForm};
use crate::convergence::Convergence;
#[cfg(target_os = "linux")]
use crate::globalmenu::{Entry, GlobalMenu, Menu};
use crate::numerics::{self, File, Group};
use crate::prefs::{self, Prefs, Theme};
use crate::residuals::Residuals;
use crate::session::{Event, Outcome, Session};
use crate::summary::{self, Summary};
use crate::surface;
use crate::viewer::{Source, Viewer};

mod newcase;
mod results;
mod shell;
use results::{Results, results_tab};
use shell::Selection;

/// The accent of the app.
const ACCENT: Accent = Accent::Vortia;
/// The size of a window control.
const CONTROL: f32 = 28.0;
/// The log lines kept; the solver prints a few per step.
const MAX_LOG: usize = 5_000;
/// The edge of the window that resizes it, with the window's own decorations off.
const GRIP: f32 = 6.0;

/// The viewport the app opens in. Linux draws its own window controls (NDL), so it asks for
/// no decorations; macOS and Windows keep the system's.
pub fn viewport() -> ViewportBuilder {
    let builder = ViewportBuilder::default()
        .with_title("Vortia")
        .with_app_id("dev.danielmiguelt.Vortia")
        .with_inner_size([1100.0, 720.0])
        .with_min_inner_size([760.0, 520.0])
        .with_icon(shell::window_icon());
    if own_controls() {
        builder.with_decorations(false).with_transparent(true)
    } else {
        builder
    }
}

fn own_controls() -> bool {
    cfg!(all(unix, not(target_os = "macos")))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dialog {
    Preferences,
    Shortcuts,
    About,
    Report,
}

/// The run of one case, and what it has reported.
struct CaseView {
    dir: PathBuf,
    application: Option<String>,
    session: Option<Session>,
    log: VecDeque<String>,
    report: Option<Convergence>,
    /// What the log has said about the residuals of this run.
    residuals: Residuals,
    outcome: Option<Outcome>,
    /// A stop was asked for and the engine has not ended yet.
    stopping: bool,
    summary: Option<Summary>,
    summary_rx: Option<std::sync::mpsc::Receiver<Summary>>,
    /// The folder changed (a run ended): read it again.
    summary_stale: bool,
    tab: Tab,
    /// What the Browser has picked besides a view, and the groups of its tree that are closed.
    selection: Selection,
    collapsed: HashSet<&'static str>,
    /// The run settings as typed, and how the last save went (`true` when it worked).
    form: Option<SettingsForm>,
    form_message: Option<(bool, String)>,
    /// The boundary conditions of one field as typed, the field asked for, and how the last
    /// save went.
    conditions: Option<Loaded>,
    conditions_rx: Option<std::sync::mpsc::Receiver<Loaded>>,
    conditions_field: Option<String>,
    conditions_message: Option<(bool, String)>,
    /// The numbers of fvSolution and the schemes of fvSchemes, as typed (each is read when its
    /// tab is first looked at), and how the last save went.
    numerics: FileEdit,
    schemes: FileEdit,
    /// The physics of `constant`, the same way.
    fluid: FileEdit,
    turbulence: FileEdit,
    transport: FileEdit,
    /// What the Results tab has read and calculated; it reads the case when first looked at.
    results: Results,
    /// A field to colour the mesh with as soon as it is read, asked for on the command line.
    first_field: Option<String>,
    /// The mesh is read the first time it is looked at.
    viewer: Option<Result<Viewer, String>>,
    viewer_rx: Option<std::sync::mpsc::Receiver<Result<surface::Surface, String>>>,
}

/// One file of `system` being edited: what was read and typed, and how the last save went.
#[derive(Default)]
struct FileEdit {
    groups: Option<Result<Vec<Group>, String>>,
    message: Option<(bool, String)>,
}

/// A tab that can be asked for from outside, on the command line.
#[derive(Clone, Copy)]
pub enum TabName {
    Results,
    Conditions,
    Numerics,
    Schemes,
    Settings,
    Fluid,
    Turbulence,
    Transport,
}

/// What the main part of the case screen shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Run,
    Mesh,
    Results,
    Conditions,
    Numerics,
    Schemes,
    Settings,
    Fluid,
    Turbulence,
    Transport,
}

impl CaseView {
    fn new(dir: PathBuf, application: Option<String>) -> Self {
        Self {
            dir,
            application,
            session: None,
            log: VecDeque::new(),
            report: None,
            residuals: Residuals::default(),
            outcome: None,
            stopping: false,
            summary: None,
            summary_rx: None,
            summary_stale: true,
            tab: Tab::Run,
            selection: Selection::default(),
            collapsed: HashSet::new(),
            form: None,
            form_message: None,
            conditions: None,
            conditions_rx: None,
            conditions_field: None,
            conditions_message: None,
            numerics: FileEdit::default(),
            schemes: FileEdit::default(),
            fluid: FileEdit::default(),
            turbulence: FileEdit::default(),
            transport: FileEdit::default(),
            results: Results::default(),
            first_field: None,
            viewer: None,
            viewer_rx: None,
        }
    }

    /// The folder the conditions are edited in: where the run starts.
    fn start_time(&self) -> String {
        let start = self.summary.as_ref().and_then(|s| s.start_time);
        vortia_foamfile::write::scalar(start.unwrap_or(0.0))
    }

    /// Reads the field of the Conditions tab on a thread when it is wanted, and takes what it read.
    fn ensure_conditions(&mut self, ctx: &Context) {
        if let Some(rx) = &self.conditions_rx
            && let Ok(loaded) = rx.try_recv()
        {
            self.conditions = Some(loaded);
            self.conditions_rx = None;
        }
        if self.tab == Tab::Conditions
            && self.conditions.is_none()
            && self.conditions_rx.is_none()
            && self.summary.is_some()
        {
            let repaint = ctx.clone();
            self.conditions_rx = Some(conditions::spawn(
                self.dir.clone(),
                self.start_time(),
                self.conditions_field.clone(),
                move || repaint.request_repaint(),
            ));
        }
    }

    /// Reads the mesh on a thread the first time the viewer is wanted, and takes what it read.
    fn ensure_viewer(&mut self, ctx: &Context) {
        if let Some(rx) = &self.viewer_rx
            && let Ok(read) = rx.try_recv()
        {
            let first_field = self.first_field.take();
            self.viewer = Some(read.map(|surface| {
                let mut viewer = Viewer::new(surface);
                viewer.colour_by(first_field.as_deref(), None);
                viewer
            }));
            self.viewer_rx = None;
        }
        if self.tab == Tab::Mesh && self.viewer.is_none() && self.viewer_rx.is_none() {
            let repaint = ctx.clone();
            self.viewer_rx = Some(surface::spawn(self.dir.clone(), move || {
                repaint.request_repaint()
            }));
        }
    }

    /// Reads the folder on a thread when it has not been read, or has changed since.
    fn ensure_summary(&mut self, ctx: &Context) {
        if let Some(rx) = &self.summary_rx
            && let Ok(summary) = rx.try_recv()
        {
            self.summary = Some(summary);
            self.summary_rx = None;
        }
        if self.summary_stale && self.summary_rx.is_none() {
            let repaint = ctx.clone();
            self.summary_rx = Some(summary::spawn(self.dir.clone(), move || {
                repaint.request_repaint()
            }));
            self.summary_stale = false;
        }
    }

    fn name(&self) -> String {
        self.dir.file_name().map_or_else(
            || self.dir.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    fn running(&self) -> bool {
        self.session.is_some()
    }

    fn push_log(&mut self, line: String) {
        self.residuals.push(&line);
        if self.log.len() >= MAX_LOG {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    /// Takes what the engine's thread has said.
    fn poll(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let events = session.poll();
        let done = session.is_finished();
        for event in events {
            match event {
                Event::Log(line) => self.push_log(line),
                Event::Report(report) => self.report = Some(report),
                Event::Finished(result) => self.outcome = Some(result),
            }
        }
        if done {
            self.session = None;
            self.summary_stale = true;
        }
    }
}

enum Screen {
    Welcome,
    Case(Box<CaseView>),
}

pub struct VortiaApp {
    prefs: Prefs,
    prefs_file: Option<PathBuf>,
    applied: Option<Theme>,
    path_input: String,
    notice: Option<String>,
    screen: Screen,
    dialog: Option<Dialog>,
    /// Home is showing over the open case (it always shows when there is none).
    home: bool,
    /// The Open field of Home takes the keyboard on the next frame.
    focus_open: bool,
    /// Home is showing the form that starts a case.
    new_case: Option<newcase::NewCase>,
    /// The command search, with what was typed, while it is open.
    search: Option<String>,
    icon: Option<egui::TextureHandle>,
    /// The geometry file Home is showing, and the path being typed for the next one.
    stl: Option<crate::stlview::StlView>,
    stl_input: String,
    /// The problem report being written: its title and its text.
    report: (String, String),
    /// Looks for a new version, offers it and installs it.
    updater: nodalia_update::egui::Updater,
    /// Whether to look for a new version by itself a while after starting.
    check_updates_at_start: bool,
    /// The menus in the shell's top bar, where there is one.
    #[cfg(target_os = "linux")]
    global_menu: Option<GlobalMenu>,
}

/// What the updater needs to know about this app.
const UPDATE_APP: nodalia_update::App = nodalia_update::App {
    name: "Vortia",
    repo: "getnodalia/vortia",
    bundle: "Vortia.app",
    version: env!("CARGO_PKG_VERSION"),
    edition: None,
};

/// The menus the shell shows for the app, each action named for [`VortiaApp::act`].
#[cfg(target_os = "linux")]
fn global_menus() -> Vec<Menu> {
    vec![
        Menu {
            title: "Vortia".into(),
            entries: vec![
                Entry::action(tr("About Vortia"), "about"),
                Entry::action(tr("Report a problem"), "report"),
                Entry::Separator,
                Entry::action(tr("Preferences"), "preferences"),
                Entry::action(tr("Keyboard shortcuts"), "shortcuts"),
                Entry::action(tr("Check for updates…"), "updates"),
                Entry::Separator,
                Entry::action(tr("Quit"), "quit"),
            ],
        },
        Menu {
            title: tr("Case").into(),
            entries: vec![
                Entry::action(tr("Run"), "run"),
                Entry::action(tr("Pause or resume"), "pause"),
                Entry::action(tr("Stop"), "stop"),
                Entry::Separator,
                Entry::action(tr("Close case"), "close"),
            ],
        },
    ]
}

impl VortiaApp {
    /// `case` is a folder named on the command line, opened at once, and run when `run`.
    pub fn new(ctx: &Context, case: Option<OsString>, run: bool) -> Self {
        let prefs_file = prefs::default_file();
        let prefs = prefs_file
            .as_deref()
            .map(Prefs::load_from)
            .unwrap_or_default();
        // Before the menus are built, and only here: a test must never change the language.
        crate::i18n::apply(prefs.lang);
        let mut app = Self::with_prefs(prefs, prefs_file);
        #[cfg(target_os = "linux")]
        {
            let wake = ctx.clone();
            app.global_menu = GlobalMenu::start(&global_menus(), move || wake.request_repaint());
        }
        // The suite's engineering type (IBM Plex), shared with Forja and Torno.
        engineering::install_fonts(ctx);
        app.apply_theme(ctx);
        if let Some(case) = case {
            let case = PathBuf::from(case);
            match app.open_case(&case) {
                Ok(()) if run => app.start_run(ctx),
                Ok(()) => {}
                Err(message) => app.notice = Some(message),
            }
        }
        app
    }

    pub fn with_prefs(prefs: Prefs, prefs_file: Option<PathBuf>) -> Self {
        Self {
            prefs,
            prefs_file,
            applied: None,
            path_input: String::new(),
            notice: None,
            screen: Screen::Welcome,
            dialog: None,
            home: false,
            focus_open: false,
            new_case: None,
            search: None,
            icon: None,
            updater: nodalia_update::egui::Updater::new(UPDATE_APP)
                .with_style(nodalia_update::egui::Style::Engineering),
            check_updates_at_start: false,
            stl: None,
            stl_input: String::new(),
            report: (String::new(), String::new()),
            #[cfg(target_os = "linux")]
            global_menu: None,
        }
    }

    /// Opens a case folder: it has to hold `system/controlDict`.
    pub fn open_case(&mut self, dir: &Path) -> Result<(), String> {
        if matches!(&self.screen, Screen::Case(case) if case.running()) {
            return Err(tr(tr(
                "Stop the current run and wait for it to finish before opening another case.",
            ))
            .into());
        }
        if !dir.is_dir() {
            return Err(trf("'{}' is not a folder.", &[&dir.display()]));
        }
        if !dir.join("system").join("controlDict").is_file() {
            return Err(format!(
                "'{}' is not a case: it has no system/controlDict.",
                dir.display()
            ));
        }
        // Only the controlDict: a case that is not ready to run still opens, and the run says why.
        let application = vortia_solvers::case::read_dict(&dir.join("system").join("controlDict"))
            .ok()
            .and_then(|control| control.word("application").map(str::to_owned));
        self.prefs.remember(dir);
        self.save_prefs();
        self.notice = None;
        self.home = false;
        self.screen = Screen::Case(Box::new(CaseView::new(dir.to_path_buf(), application)));

        Ok(())
    }

    /// Switches the open case to its mesh.
    pub fn show_mesh(&mut self) {
        if let Screen::Case(case) = &mut self.screen {
            case.tab = Tab::Mesh;
        }
    }

    /// Opens Home's "New case" form on the sketch, for a screenshot of it.
    /// Shows an STL file on Home. The file is read on a thread.
    pub fn show_stl(&mut self, ctx: &Context, path: PathBuf) {
        let wake = ctx.clone();
        self.stl = Some(crate::stlview::StlView::open(path, move || {
            wake.request_repaint();
        }));
        self.home = true;
        self.new_case = None;
        self.notice = None;
    }

    /// Opens the problem report, for a screenshot and for the command line.
    pub fn show_report(&mut self) {
        self.dialog = Some(Dialog::Report);
    }

    pub fn show_new_sketch(&mut self) {
        let mut form = newcase::NewCase::new(&newcase::NewCase::default_base());
        form.choose_sketch();
        self.new_case = Some(form);
    }

    /// Switches the open case to one of its tabs.
    pub fn show_tab(&mut self, tab: TabName) {
        if let Screen::Case(case) = &mut self.screen {
            case.tab = match tab {
                TabName::Results => Tab::Results,
                TabName::Conditions => Tab::Conditions,
                TabName::Numerics => Tab::Numerics,
                TabName::Schemes => Tab::Schemes,
                TabName::Settings => Tab::Settings,
                TabName::Fluid => Tab::Fluid,
                TabName::Turbulence => Tab::Turbulence,
                TabName::Transport => Tab::Transport,
            };
        }
    }

    /// Switches the open case to its mesh, coloured with a field of the latest time.
    pub fn show_mesh_field(&mut self, field: &str) {
        self.show_mesh();
        if let Screen::Case(case) = &mut self.screen {
            case.first_field = Some(field.to_owned());
        }
    }

    fn save_prefs(&self) {
        if let Some(file) = &self.prefs_file {
            // Preferences are not worth interrupting the user for.
            let _ = self.prefs.save_to(file);
        }
    }

    fn start_run(&mut self, ctx: &Context) {
        let Screen::Case(case) = &mut self.screen else {
            return;
        };
        if case.running() {
            return;
        }
        let repaint = ctx.clone();
        match Session::start(&case.dir, move || repaint.request_repaint()) {
            Ok(session) => {
                case.session = Some(session);
                case.log.clear();
                case.report = None;
                case.residuals = Residuals::default();
                case.outcome = None;
                case.stopping = false;
            }
            Err(e) => {
                case.outcome = Some(Outcome::Failed(format!("The engine could not start: {e}")))
            }
        }
    }

    fn stop_run(&mut self) {
        if let Screen::Case(case) = &mut self.screen
            && let Some(session) = &case.session
            && !case.stopping
        {
            session.stop();
            case.stopping = true;
        }
    }

    /// Holds a run at the start of its next step, or lets it go on.
    fn toggle_pause(&mut self) {
        if let Screen::Case(case) = &mut self.screen
            && let Some(session) = &case.session
            && !case.stopping
        {
            session.set_paused(!session.is_paused());
        }
    }

    fn close_case(&mut self) {
        if matches!(&self.screen, Screen::Case(case) if !case.running()) {
            self.screen = Screen::Welcome;
        }
    }

    /// A command that came from the shell's menu.
    #[cfg(target_os = "linux")]
    fn act(&mut self, ctx: &Context, action: &str) {
        match action {
            "about" => self.dialog = Some(Dialog::About),
            "preferences" => self.dialog = Some(Dialog::Preferences),
            "shortcuts" => self.dialog = Some(Dialog::Shortcuts),
            "report" => self.dialog = Some(Dialog::Report),
            "updates" => self.updater.check_now(ctx),
            "quit" => ctx.send_viewport_cmd(ViewportCommand::Close),
            "run" => self.start_run(ctx),
            "pause" => self.toggle_pause(),
            "stop" => self.stop_run(),
            "close" => self.close_case(),
            _ => {}
        }
    }

    /// Takes the shell's clicks, and tells it which of the case's commands apply now.
    #[cfg(target_os = "linux")]
    fn sync_global_menu(&mut self, ctx: &Context) {
        let Some(menu) = &self.global_menu else {
            return;
        };
        let clicks = menu.poll();
        let (open, running, stopping) = match &self.screen {
            Screen::Case(case) => (true, case.running(), case.stopping),
            Screen::Welcome => (false, false, false),
        };
        menu.set_enabled("run", open && !running);
        menu.set_enabled("pause", running && !stopping);
        menu.set_enabled("stop", running && !stopping);
        menu.set_enabled("close", open && !running);
        for action in clicks {
            self.act(ctx, &action);
        }
    }

    fn apply_theme(&mut self, ctx: &Context) {
        if self.applied == Some(self.prefs.theme) {
            return;
        }
        let base = for_egui::Appearance {
            accent: ACCENT,
            ..for_egui::Appearance::default()
        };
        match self.prefs.theme {
            Theme::System => for_egui::apply_system(ctx, &base),
            Theme::Light => for_egui::apply(
                ctx,
                &for_egui::Appearance {
                    mode: Mode::Light,
                    ..base
                },
            ),
            Theme::Dark => for_egui::apply(
                ctx,
                &for_egui::Appearance {
                    mode: Mode::Dark,
                    ..base
                },
            ),
        }
        // Applying the NDL style resets the sizes, so the suite's scale goes on after it.
        engineering::apply_text_styles(ctx);
        self.applied = Some(self.prefs.theme);
    }

    fn shortcuts(&mut self, ctx: &Context) {
        // The update windows are modal: keys do not reach what is behind them.
        if self.updater.is_open() {
            return;
        }
        let command = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let take = |shortcut: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&shortcut));
        if take(command(Key::Comma)) {
            self.dialog = Some(Dialog::Preferences);
        }
        if take(command(Key::Questionmark)) || take(KeyboardShortcut::new(Modifiers::NONE, Key::F1))
        {
            self.dialog = Some(Dialog::Shortcuts);
        }
        if take(command(Key::K)) {
            self.search = Some(String::new());
        }
        if take(command(Key::R)) {
            self.start_run(ctx);
        }
        if take(command(Key::Q)) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if take(KeyboardShortcut::new(Modifiers::NONE, Key::Escape)) {
            self.dialog = None;
            self.search = None;
        }
    }

    /// One frame of the interface.
    pub fn ui(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        self.apply_theme(ctx);
        if let Screen::Case(case) = &mut self.screen {
            case.poll();
            case.ensure_summary(ctx);
            if case.running() {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
        #[cfg(target_os = "linux")]
        self.sync_global_menu(ctx);
        self.shortcuts(ctx);

        let p = palette(ctx);
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let round = if own_controls() && !maximized {
            radius::XXL
        } else {
            0.0
        };
        let frame = Frame::NONE
            .fill(for_egui::color(p.surface_window))
            .corner_radius(corner(round));

        egui::CentralPanel::default()
            .frame(frame)
            .show(root, |ui| self.shell(ui, ctx, maximized));

        self.dialogs(ctx, &p);
        self.updates(ctx);
        if own_controls() && !maximized {
            resize_edges(ctx);
        }
    }

    /// Looks for a new version when asked (and once a day after starting, when the app says so),
    /// and shows the windows of the update.
    fn updates(&mut self, ctx: &Context) {
        if self.check_updates_at_start {
            self.updater.check_at_start(ctx);
        }
        // Closing is safe while no run is going and no dialog or search is open.
        let safe = self.dialog.is_none()
            && self.search.is_none()
            && !matches!(&self.screen, Screen::Case(case) if case.running());
        let language = match crate::i18n::current() {
            crate::i18n::Lang::Es => nodalia_update::Language::Spanish,
            crate::i18n::Lang::En => nodalia_update::Language::English,
        };
        match self.updater.show(ctx, language, safe) {
            nodalia_update::egui::Action::OpenUrl(url) => {
                ctx.open_url(egui::OpenUrl::new_tab(url));
            }
            nodalia_update::egui::Action::Exit => std::process::exit(0),
            nodalia_update::egui::Action::None => {}
        }
    }

    /// Makes the app look for a new version by itself a while after it starts.
    pub fn check_updates_at_start(&mut self) {
        self.check_updates_at_start = true;
    }

    fn dialogs(&mut self, ctx: &Context, p: &Palette) {
        let Some(dialog) = self.dialog else {
            return;
        };
        let title = match dialog {
            Dialog::Preferences => tr("Preferences"),
            Dialog::Shortcuts => tr("Keyboard shortcuts"),
            Dialog::About => tr("About Vortia"),
            Dialog::Report => tr("Report a problem"),
        };
        let mut close = false;
        let mut open_report = false;
        let before = self.prefs.theme;
        let lang_before = self.prefs.lang;
        egui::Modal::new(Id::unique("dialog")).show(ctx, |ui| {
            ui.set_width(component_dialog_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).size(font_size::TITLE).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = control(ui, "close", p.status_danger_fg).clicked();
                });
            });
            ui.add_space(space::MD);
            match dialog {
                Dialog::Preferences => {
                    ui.label(RichText::new(tr("Appearance")).color(muted(p)));
                    for theme in Theme::ALL {
                        ui.selectable_value(&mut self.prefs.theme, theme, theme_label(theme));
                    }
                    ui.add_space(space::MD);
                    ui.label(RichText::new(tr("Language")).color(muted(p)));
                    for lang in Choice::ALL {
                        ui.selectable_value(&mut self.prefs.lang, lang, lang.label());
                    }
                }
                Dialog::Shortcuts => {
                    egui::Grid::new("shortcuts")
                        .num_columns(2)
                        .spacing([space::XL, space::SM])
                        .show(ui, |ui| {
                            for (keys, what) in SHORTCUTS {
                                ui.label(RichText::new(*keys).monospace());
                                ui.label(tr(what));
                                ui.end_row();
                            }
                        });
                }
                Dialog::About => {
                    ui.label(trf("Version {}", &[&env!("CARGO_PKG_VERSION")]));
                    ui.add_space(space::SM);
                    ui.label(tr(
                        "A compressible-flow solver and its desktop app, written in Rust.",
                    ));
                    ui.add_space(space::SM);
                    ui.label(RichText::new("GPL-3.0-or-later").color(muted(p)));
                }
                Dialog::Report => {
                    let application = match &self.screen {
                        Screen::Case(case) => case.application.clone(),
                        Screen::Welcome => None,
                    };
                    ui.label(RichText::new(tr("Title")).color(muted(p)));
                    ui.add(egui::TextEdit::singleline(&mut self.report.0).desired_width(f32::INFINITY));
                    ui.add_space(space::SM);
                    ui.label(RichText::new(tr("What happened")).color(muted(p)));
                    ui.add(
                        egui::TextEdit::multiline(&mut self.report.1)
                            .desired_rows(6)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(space::SM);
                    ui.label(
                        RichText::new(tr(
                            "Nothing is sent from here: GitHub opens with this text for you to review. The version, the system and the solver are added; no path or case name is.",
                        ))
                        .color(muted(p)),
                    );
                    ui.add_space(space::MD);
                    let ready = !self.report.0.trim().is_empty();
                    if ui
                        .add_enabled(ready, egui::Button::new(tr("Open on GitHub")))
                        .clicked()
                    {
                        let text = crate::report::body(&self.report.1, application.as_deref());
                        let draft = crate::report::draft(self.report.0.trim(), &text);
                        if draft.body_on_clipboard {
                            ctx.copy_text(text);
                            self.notice = Some(tr(
                                "The text was too long for the address: it is on the clipboard, paste it into the issue.",
                            )
                            .into());
                        }
                        ctx.open_url(egui::OpenUrl::new_tab(draft.url));
                        open_report = true;
                    }
                }
            }
        });
        if close || open_report {
            self.dialog = None;
        }
        if self.prefs.theme != before {
            self.apply_theme(ctx);
            self.save_prefs();
        }
        if self.prefs.lang != lang_before {
            crate::i18n::apply(self.prefs.lang);
            self.save_prefs();
        }
    }
}

const SHORTCUTS: &[(&str, &str)] = &[
    ("Ctrl+K", "Search commands"),
    ("Ctrl+R", "Run the case"),
    ("Ctrl+,", "Preferences"),
    ("F1", "Keyboard shortcuts"),
    ("Ctrl+Q", "Quit"),
    ("Esc", "Close a dialog"),
];

fn component_dialog_width() -> f32 {
    nodalia_theme::size::DIALOG
}

fn theme_label(theme: Theme) -> &'static str {
    match theme {
        Theme::System => tr("Follow the system"),
        Theme::Light => tr("Light"),
        Theme::Dark => tr("Dark"),
    }
}

/// What a convergence status says to the user, and in which tone.
pub fn status_label(status: &str) -> (&'static str, Tone) {
    match status {
        "warming" => (tr("Warming up"), Tone::Info),
        "checking" => (tr("Checking"), Tone::Info),
        "stabilizing" => (tr("Settling"), Tone::Warning),
        "converged" => (tr("Converged"), Tone::Success),
        "limit" => (tr("Time limit reached"), Tone::Warning),
        "invalid" => (tr("The solution left its physical range"), Tone::Danger),
        _ => (tr("Unknown"), Tone::Info),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Success,
    Warning,
    Danger,
}

/// A number in scientific notation, or a dash when the engine could not give it.
pub fn scientific(value: Option<f64>) -> String {
    value.map_or_else(|| "–".to_owned(), |v| format!("{v:.2e}"))
}

/// A count with a thin space between thousands.
pub fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

fn convergence_card(ui: &mut egui::Ui, p: &Palette, report: Option<&Convergence>) {
    Frame::NONE
        .fill(for_egui::color(p.surface_sunken))
        .corner_radius(corner(component::CONTROL_RADIUS))
        .inner_margin(Margin::same(space::MD as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let Some(r) = report else {
                ui.label(
                    RichText::new(tr(tr(
                        "Convergence appears once the solver has checked the run.",
                    )))
                    .color(muted(p)),
                );
                return;
            };
            let (label, tone) = status_label(&r.status);
            ui.horizontal(|ui| {
                let (fg, bg) = tone_colors(p, tone);
                Frame::NONE
                    .fill(bg)
                    .corner_radius(corner(radius::FULL))
                    .inner_margin(Margin::symmetric(space::MD as i8, space::XS as i8))
                    .show(ui, |ui| ui.label(RichText::new(label).color(fg)));
                if r.automatic {
                    ui.label(
                        RichText::new(format!(
                            "{} of {} checks in a row",
                            r.stable_samples, r.required_samples
                        ))
                        .color(muted(p)),
                    );
                }
            });
            if r.automatic {
                ui.add_space(space::SM);
                ui.add(
                    egui::ProgressBar::new(r.progress()).desired_height(component::PROGRESS_HEIGHT),
                );
            }
            ui.add_space(space::SM);
            egui::Grid::new("convergence")
                .num_columns(3)
                .spacing([space::XL, space::XS])
                .show(ui, |ui| {
                    for (name, value, limit) in [
                        (tr("Mass balance"), r.global_mass, r.mass_tolerance),
                        (tr("Local mass"), r.local_mass, r.mass_tolerance),
                        (tr("Equations"), r.equation_error, r.equation_tolerance),
                        (tr("Field change"), r.field_change, r.field_tolerance),
                    ] {
                        ui.label(RichText::new(name).color(muted(p)));
                        ui.label(RichText::new(scientific(value)).monospace());
                        ui.label(
                            RichText::new(format!("limit {}", scientific(limit)))
                                .monospace()
                                .color(muted(p)),
                        );
                        ui.end_row();
                    }
                });
        });
}

/// One dictionary of `system` (the solver numbers of fvSolution, the schemes of fvSchemes), to
/// edit and save into its file.
fn file_tab(ui: &mut egui::Ui, p: &Palette, case: &mut CaseView, file: File) {
    let running = case.running();
    let edit = match file {
        File::Solution => &mut case.numerics,
        File::Schemes => &mut case.schemes,
        File::Fluid => &mut case.fluid,
        File::Turbulence => &mut case.turbulence,
        File::Transport => &mut case.transport,
    };
    let read = edit
        .groups
        .get_or_insert_with(|| numerics::read(&case.dir, file));
    let (mut save, mut revert) = (false, false);
    match read {
        Err(message) => {
            ui.colored_label(for_egui::color(p.status_danger_fg), message.as_str());
        }
        Ok(groups) => {
            let dirty = groups
                .iter()
                .any(|g| g.items.iter().any(|item| item.changed()));
            egui::ScrollArea::vertical()
                .id_salt(file.name())
                .auto_shrink([false, false])
                .max_height((ui.available_height() - 90.0).max(120.0))
                .show(ui, |ui| {
                    ui.add_enabled_ui(!running, |ui| {
                        for group in groups.iter_mut() {
                            ui.add_space(space::MD);
                            ui.label(RichText::new(group.title()).strong());
                            let id = group.title();
                            for item in &mut group.items {
                                let words = numerics::choices(file, &group.path, &item.key);
                                if item.editable && !words.is_empty() {
                                    choice_row(ui, p, &item.key.clone(), item, words, &id);
                                } else {
                                    condition_row(ui, p, &item.key.clone(), item, &id);
                                }
                            }
                        }
                    });
                });
            ui.add_space(space::MD);
            ui.add_enabled_ui(!running && dirty, |ui| {
                ui.horizontal(|ui| {
                    save = accent_button(ui, p, tr("Save")).clicked();
                    revert = ui.button(tr("Revert")).clicked();
                });
            });
            if running {
                ui.label(
                    RichText::new(format!(
                        "{} cannot change while the case runs.",
                        file.name()
                    ))
                    .color(muted(p)),
                );
            } else if file != File::Solution {
                ui.label(
                    RichText::new(tr("The engine checks these entries when a run starts."))
                        .size(font_size::SMALL)
                        .color(muted(p)),
                );
            }
        }
    }
    if let Some((ok, message)) = &edit.message {
        let tone = if *ok {
            p.status_success_fg
        } else {
            p.status_danger_fg
        };
        ui.colored_label(for_egui::color(tone), message.as_str());
    }
    if revert {
        edit.groups = None;
        edit.message = None;
    } else if save && let Some(Ok(groups)) = &edit.groups {
        edit.message = Some(match numerics::save(&case.dir, file, groups) {
            Ok(()) => (
                true,
                trf("Saved to {}/{}.", &[&file.folder(), &file.name()]),
            ),
            Err(message) => (false, message),
        });
        if matches!(edit.message, Some((true, _))) {
            edit.groups = None;
        }
    }
}

/// The boundary conditions of one field of the case, to edit and save into the field's file.
fn conditions_tab(ui: &mut egui::Ui, p: &Palette, case: &mut CaseView) {
    let running = case.running();
    let time = case.start_time();
    let application = case.application.clone();
    let Some(loaded) = case.conditions.as_mut() else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tr("Reading the field…"));
        });
        return;
    };
    let mut pick: Option<String> = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Field")).color(muted(p)));
        let shown = loaded
            .current
            .as_ref()
            .map(|c| c.field.clone())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("conditions-field")
            .selected_text(shown.as_str())
            .show_ui(ui, |ui| {
                for name in &loaded.fields {
                    if ui.selectable_label(*name == shown, name).clicked() && *name != shown {
                        pick = Some(name.clone());
                    }
                }
            });
        ui.label(
            RichText::new(trf("from the {} folder", &[&time]))
                .size(font_size::SMALL)
                .color(muted(p)),
        );
    });
    ui.add_space(space::SM);
    let (mut save, mut revert) = (false, false);
    match loaded.current.as_mut() {
        Err(message) => {
            ui.colored_label(for_egui::color(p.status_danger_fg), message.as_str());
        }
        Ok(field) => {
            let dirty = field.internal.changed()
                || field
                    .patches
                    .iter()
                    .any(|patch| patch.items.iter().any(|i| i.changed()));
            let types = conditions::patch_types(field, application.as_deref());
            egui::ScrollArea::vertical()
                .id_salt("conditions")
                .auto_shrink([false, false])
                .max_height((ui.available_height() - 90.0).max(120.0))
                .show(ui, |ui| {
                    ui.add_enabled_ui(!running, |ui| {
                        condition_row(ui, p, "internalField", &mut field.internal, tr("Inside"));
                        for patch in &mut field.patches {
                            ui.add_space(space::MD);
                            ui.label(RichText::new(&patch.patch).strong());
                            let retype = conditions::retypable(patch);
                            let retyped =
                                patch.items.iter().any(|i| i.key == "type" && i.changed());
                            for item in &mut patch.items {
                                if retype && item.key == "type" {
                                    choice_row(ui, p, "type", item, &types, &patch.patch);
                                } else if retyped {
                                    // The block is written again for the new type.
                                    ui.horizontal(|ui| {
                                        ui.add_space(space::MD);
                                        ui.label(
                                            RichText::new(format!(
                                                "{} — replaced on save",
                                                item.key
                                            ))
                                            .color(muted(p)),
                                        );
                                    });
                                } else {
                                    condition_row(ui, p, &item.key.clone(), item, &patch.patch);
                                }
                            }
                        }
                    });
                });
            ui.add_space(space::MD);
            ui.add_enabled_ui(!running && dirty, |ui| {
                ui.horizontal(|ui| {
                    save = accent_button(ui, p, tr("Save")).clicked();
                    revert = ui.button(tr("Revert")).clicked();
                });
            });
            if running {
                ui.label(
                    RichText::new(tr("The conditions cannot change while the case runs."))
                        .color(muted(p)),
                );
            }
        }
    }
    if let Some((ok, message)) = &case.conditions_message {
        let tone = if *ok {
            p.status_success_fg
        } else {
            p.status_danger_fg
        };
        ui.colored_label(for_egui::color(tone), message.as_str());
    }
    if let Some(name) = pick {
        case.conditions_field = Some(name);
        case.conditions = None;
        case.conditions_message = None;
    } else if revert {
        case.conditions = None;
        case.conditions_message = None;
    } else if save && let Some(Ok(field)) = case.conditions.as_ref().map(|l| &l.current) {
        let result = conditions::save(&case.dir, &time, field);
        case.conditions_message = Some(match result {
            Ok(()) => (true, trf("Saved to {}/{}.", &[&time, &field.field])),
            Err(message) => (false, message),
        });
        if matches!(case.conditions_message, Some((true, _))) {
            // Read it again, so what shows is what the file says.
            case.conditions_field = Some(field.field.clone());
            case.conditions = None;
        }
    }
}

/// One entry: its name, and its value in a box when it can be edited.
fn condition_row(ui: &mut egui::Ui, p: &Palette, key: &str, item: &mut conditions::Item, id: &str) {
    ui.horizontal(|ui| {
        ui.add_space(space::MD);
        ui.allocate_ui_with_layout(
            Vec2::new(140.0, 24.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(140.0);
                ui.label(RichText::new(key).color(muted(p)));
            },
        );
        if item.editable {
            ui.add(
                egui::TextEdit::singleline(&mut item.text)
                    .id_salt((id, key))
                    .desired_width(ui.available_width().clamp(200.0, 420.0)),
            );
        } else {
            ui.label(item.text.as_str());
        }
    });
}

/// One entry that is one of a few names: its name, and a pick-list of what the engine has. A word
/// the case already has that is not in the list stays in it, so nothing is lost by looking.
fn choice_row(
    ui: &mut egui::Ui,
    p: &Palette,
    key: &str,
    item: &mut conditions::Item,
    words: &[&str],
    id: &str,
) {
    ui.horizontal(|ui| {
        ui.add_space(space::MD);
        ui.allocate_ui_with_layout(
            Vec2::new(140.0, 24.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(140.0);
                ui.label(RichText::new(key).color(muted(p)));
            },
        );
        let current = item.text.clone();
        egui::ComboBox::from_id_salt((id, key))
            .selected_text(current.as_str())
            .width(220.0)
            .show_ui(ui, |ui| {
                if !words.contains(&current.as_str()) {
                    ui.selectable_value(&mut item.text, current.clone(), current.as_str());
                }
                for word in words {
                    ui.selectable_value(&mut item.text, (*word).to_owned(), *word);
                }
            });
    });
    if let Some(fields) = numerics::model_fields(&item.text) {
        ui.horizontal(|ui| {
            ui.add_space(space::MD + 148.0);
            ui.label(
                RichText::new(format!("Needs {fields} in the time folder."))
                    .size(font_size::SMALL)
                    .color(muted(p)),
            );
        });
    }
}

/// The run settings of the case, to edit and save into its controlDict.
fn settings_tab(ui: &mut egui::Ui, p: &Palette, case: &mut CaseView) {
    if case.form.is_none()
        && let Some(summary) = &case.summary
    {
        case.form = Some(SettingsForm::from_summary(summary));
    }
    let running = case.running();
    let mut save = false;
    let mut revert = false;
    let Some(form) = case.form.as_mut() else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tr("Reading the case…"));
        });
        return;
    };
    ui.add_enabled_ui(!running, |ui| {
        egui::Grid::new("run-settings")
            .num_columns(3)
            .spacing([space::XL, space::MD])
            .show(ui, |ui| {
                for (label, unit, text) in [
                    (tr("Start time"), "s", &mut form.start_time),
                    (tr("End time"), "s", &mut form.end_time),
                    (tr("Time step"), "s", &mut form.delta_t),
                    (
                        tr("Write interval"),
                        tr("steps or s, as the case writes"),
                        &mut form.write_interval,
                    ),
                ] {
                    ui.label(RichText::new(label).color(muted(p)));
                    ui.add(egui::TextEdit::singleline(text).desired_width(140.0));
                    ui.label(RichText::new(unit).size(font_size::SMALL).color(muted(p)));
                    ui.end_row();
                }
            });
        ui.add_space(space::LG);
        ui.horizontal(|ui| {
            save = accent_button(ui, p, tr("Save")).clicked();
            revert = ui.button(tr("Revert")).clicked();
        });
    });
    if running {
        ui.add_space(space::SM);
        ui.label(
            RichText::new(tr("The settings cannot change while the case runs.")).color(muted(p)),
        );
    }
    if let Some((ok, message)) = &case.form_message {
        ui.add_space(space::SM);
        let tone = if *ok {
            p.status_success_fg
        } else {
            p.status_danger_fg
        };
        ui.colored_label(for_egui::color(tone), message.as_str());
    }
    if revert {
        case.form = case.summary.as_ref().map(SettingsForm::from_summary);
        case.form_message = None;
    } else if save {
        let result = case
            .form
            .as_ref()
            .map(SettingsForm::parse)
            .unwrap_or_else(|| Err(tr("Nothing to save yet.").to_owned()))
            .and_then(|settings| controldict::save(&case.dir, &settings));
        case.form_message = Some(match result {
            Ok(()) => {
                case.summary_stale = true;
                (true, tr("Saved to system/controlDict.").to_owned())
            }
            Err(message) => (false, message),
        });
    }
}

/// The mesh of the case, or why there is none to show.
fn mesh_tab(
    ui: &mut egui::Ui,
    p: &Palette,
    viewer: Option<&mut Result<Viewer, String>>,
    source: &Source,
) {
    match viewer {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr("Reading the mesh…"));
            });
        }
        Some(Err(message)) => {
            ui.colored_label(for_egui::color(p.status_danger_fg), message.as_str());
        }
        Some(Ok(viewer)) => viewer.show(
            ui,
            for_egui::color(p.surface_sunken),
            muted(p),
            component::CONTROL_RADIUS.round().clamp(0.0, 255.0) as u8,
            source,
        ),
    }
}

fn tone_colors(p: &Palette, tone: Tone) -> (Color32, Color32) {
    let (fg, bg) = match tone {
        Tone::Info => (p.status_info_fg, p.status_info_bg),
        Tone::Success => (p.status_success_fg, p.status_success_bg),
        Tone::Warning => (p.status_warning_fg, p.status_warning_bg),
        Tone::Danger => (p.status_danger_fg, p.status_danger_bg),
    };
    (for_egui::color(fg), for_egui::color(bg))
}

fn palette(ctx: &Context) -> Palette {
    let mode = if ctx.global_style().visuals.dark_mode {
        Mode::Dark
    } else {
        Mode::Light
    };
    mode.palette_with(ACCENT)
}

fn muted(p: &Palette) -> Color32 {
    for_egui::color(p.text_secondary)
}

fn corner(value: f32) -> CornerRadius {
    CornerRadius::same(value.round().clamp(0.0, 255.0) as u8)
}

/// An icon of the NDL set painted into a square; nothing when the set lacks the name.
fn paint_icon(painter: &egui::Painter, rect: egui::Rect, name: &str, ink: Color32) {
    if let Some(icon) = icons::get(name) {
        icons::paint(painter, rect, icon, icons::Style::bold(ink));
    }
}

/// A round window control, tinted: the fill is the tint at low strength, the icon the tint.
fn control(ui: &mut egui::Ui, name: &str, tint: nodalia_theme::Rgba) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(CONTROL), Sense::click());
    let ink = for_egui::color(tint);
    let strength = if response.hovered() { 0.34 } else { 0.18 };
    ui.painter()
        .circle_filled(rect.center(), CONTROL / 2.0, ink.gamma_multiply(strength));
    paint_icon(ui.painter(), rect.shrink(8.0), name, ink);
    response
}

fn accent_button(ui: &mut egui::Ui, p: &Palette, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(for_egui::color(p.text_on_accent)))
            .fill(for_egui::color(p.accent_default))
            .stroke(Stroke::NONE),
    )
}

/// With no window decorations the edges of the window are ours to resize by.
fn resize_edges(ctx: &Context) {
    use egui::ResizeDirection as D;
    let screen = ctx.content_rect();
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let left = pos.x - screen.min.x < GRIP;
    let right = screen.max.x - pos.x < GRIP;
    let top = pos.y - screen.min.y < GRIP;
    let bottom = screen.max.y - pos.y < GRIP;
    let direction = match (left, right, top, bottom) {
        (true, _, true, _) => D::NorthWest,
        (_, true, true, _) => D::NorthEast,
        (true, _, _, true) => D::SouthWest,
        (_, true, _, true) => D::SouthEast,
        (true, ..) => D::West,
        (_, true, ..) => D::East,
        (_, _, true, _) => D::North,
        (.., true) => D::South,
        _ => return,
    };
    let icon = match direction {
        D::North | D::South => egui::CursorIcon::ResizeVertical,
        D::East | D::West => egui::CursorIcon::ResizeHorizontal,
        D::NorthEast | D::SouthWest => egui::CursorIcon::ResizeNeSw,
        D::NorthWest | D::SouthEast => egui::CursorIcon::ResizeNwSe,
    };
    ctx.set_cursor_icon(icon);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}

impl eframe::App for VortiaApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // The window is transparent where its corners are rounded.
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        VortiaApp::ui(self, ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_never_gets_eframes_default_icon() {
        // A viewport without an icon makes eframe use its own egui logo, which on macOS hides
        // the bundle's icon in the Dock. macOS asks for none; the others carry the portrait.
        let icon = viewport()
            .icon
            .expect("the viewport sets an icon explicitly");
        if cfg!(target_os = "macos") {
            assert_eq!(*icon, egui::IconData::default());
        } else {
            assert!(icon.width > 0 && icon.height > 0);
            assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
        }
    }

    fn frame(app: &mut VortiaApp, ctx: &Context) {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.ui(ui));
        // Nobody paints these frames.
        output.textures_delta.clear();
    }

    fn case_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("system")).unwrap();
        std::fs::write(
            dir.path().join("system/controlDict"),
            "FoamFile { version 2.0; format ascii; class dictionary; object controlDict; }\n\
             application vortiaCentralFoam;\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn only_a_folder_with_a_control_dict_opens() {
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        let empty = tempfile::tempdir().unwrap();
        assert!(app.open_case(&empty.path().join("missing")).is_err());
        assert!(
            app.open_case(empty.path())
                .unwrap_err()
                .contains("controlDict")
        );
        assert!(matches!(app.screen, Screen::Welcome));

        let case = case_dir();
        app.open_case(case.path()).unwrap();
        let Screen::Case(view) = &app.screen else {
            panic!("the case did not open");
        };
        assert_eq!(view.application.as_deref(), Some("vortiaCentralFoam"));
        assert_eq!(app.prefs.recent, [case.path().to_path_buf()]);
    }

    #[test]
    fn opening_another_case_does_not_abandon_a_running_session() {
        let first = case_dir();
        let second = case_dir();
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(first.path()).unwrap();
        let Screen::Case(case) = &mut app.screen else {
            panic!("the case did not open");
        };
        // The session remains active in the UI until its completion event is polled.
        case.session = Some(Session::start(first.path(), || {}).unwrap());
        assert!(
            app.open_case(second.path())
                .unwrap_err()
                .contains("Stop the current run")
        );
        let Screen::Case(case) = &app.screen else {
            panic!("the active case was lost");
        };
        assert_eq!(case.dir, first.path());
        assert!(case.running());
        assert_eq!(app.prefs.recent, [first.path().to_path_buf()]);
    }

    #[test]
    fn the_run_settings_of_a_case_are_edited_and_saved() {
        let case = case_dir();
        std::fs::write(
            case.path().join("system/controlDict"),
            "application vortiaCentralFoam;\nstartTime 0;\nendTime 10;\ndeltaT 0.5;\n\
             writeInterval 2;\n",
        )
        .unwrap();
        let ctx = Context::default();
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(case.path()).unwrap();
        if let Screen::Case(view) = &mut app.screen {
            view.tab = Tab::Settings;
        }
        let mut ready = false;
        for _ in 0..200 {
            frame(&mut app, &ctx);
            if let Screen::Case(view) = &app.screen
                && view.form.is_some()
            {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready, "the form never appeared");
        let Screen::Case(view) = &mut app.screen else {
            panic!("the case closed");
        };
        let form = view.form.as_mut().unwrap();
        assert_eq!(form.end_time, "10");
        form.end_time = "20".into();
        // What the Save button does, without a pointer.
        let settings = view.form.as_ref().unwrap().parse().unwrap();
        controldict::save(&view.dir, &settings).unwrap();
        let text = std::fs::read_to_string(case.path().join("system/controlDict")).unwrap();
        assert!(text.contains("endTime 20;"), "{text}");
        assert!(text.contains("deltaT 0.5;"), "{text}");
    }

    #[test]
    fn the_boundary_conditions_of_a_field_are_shown_and_saved() {
        let case = case_dir();
        std::fs::create_dir_all(case.path().join("0")).unwrap();
        std::fs::write(
            case.path().join("0/T"),
            "FoamFile { version 2.0; format ascii; class volScalarField; object T; }\n\
             dimensions [0 0 0 1 0 0 0];\ninternalField uniform 300;\n\
             boundaryField\n{\n    inlet\n    {\n        type fixedValue;\n\
             \x20       value uniform 300;\n    }\n}\n",
        )
        .unwrap();
        let ctx = Context::default();
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(case.path()).unwrap();
        app.show_tab(TabName::Conditions);
        let mut ready = false;
        for _ in 0..300 {
            frame(&mut app, &ctx);
            if let Screen::Case(view) = &app.screen
                && view.conditions.is_some()
            {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready, "the field was never read");
        let Screen::Case(view) = &mut app.screen else {
            panic!("the case closed");
        };
        let loaded = view.conditions.as_mut().unwrap();
        assert_eq!(loaded.fields, ["T"]);
        let field = loaded.current.as_mut().unwrap();
        field.patches[0].items[1].text = "uniform 350".into();
        conditions::save(&view.dir, "0", field).unwrap();
        let text = std::fs::read_to_string(case.path().join("0/T")).unwrap();
        assert!(text.contains("value uniform 350;"), "{text}");
        assert!(text.contains("internalField uniform 300;"), "{text}");
    }

    #[test]
    fn the_solver_numbers_of_a_case_are_shown_and_saved() {
        let case = case_dir();
        std::fs::write(
            case.path().join("system/fvSolution"),
            "solvers\n{\n    p\n    {\n        solver PCG;\n        relTol 0.01;\n    }\n}\n",
        )
        .unwrap();
        let ctx = Context::default();
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(case.path()).unwrap();
        app.show_tab(TabName::Numerics);
        frame(&mut app, &ctx);
        let Screen::Case(view) = &mut app.screen else {
            panic!("the case closed");
        };
        let groups = view.numerics.groups.as_mut().unwrap().as_mut().unwrap();
        assert_eq!(groups[0].title(), "solvers / p");
        groups[0].items[1].text = "0.1".into();
        numerics::save(&view.dir, File::Solution, groups).unwrap();
        let text = std::fs::read_to_string(case.path().join("system/fvSolution")).unwrap();
        assert!(
            text.contains("relTol 0.1;") && text.contains("solver PCG;"),
            "{text}"
        );
    }

    #[test]
    fn the_physics_of_a_case_is_shown_and_saved_in_constant() {
        let case = case_dir();
        std::fs::create_dir_all(case.path().join("constant")).unwrap();
        std::fs::write(
            case.path().join("constant/turbulenceProperties"),
            "simulationType RAS;\nRAS\n{\n    model           kOmegaSST;\n    turbulence      on;\n}\n",
        )
        .unwrap();
        std::fs::write(
            case.path().join("constant/thermophysicalProperties"),
            "mixture\n{\n    transport\n    {\n        mu              1.8e-05;\n        Pr              0.71;\n    }\n}\n",
        )
        .unwrap();
        let ctx = Context::default();
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(case.path()).unwrap();
        app.show_tab(TabName::Turbulence);
        frame(&mut app, &ctx);
        app.show_tab(TabName::Fluid);
        frame(&mut app, &ctx);
        let Screen::Case(view) = &mut app.screen else {
            panic!("the case closed");
        };
        let groups = view.turbulence.groups.as_mut().unwrap().as_mut().unwrap();
        assert_eq!(groups[1].title(), "RAS");
        groups[1].items[0].text = "kEpsilon".into();
        numerics::save(&view.dir, File::Turbulence, groups).unwrap();
        let groups = view.fluid.groups.as_mut().unwrap().as_mut().unwrap();
        assert_eq!(groups[0].title(), "mixture / transport");
        groups[0].items[0].text = "2e-05".into();
        numerics::save(&view.dir, File::Fluid, groups).unwrap();
        let read = |name| std::fs::read_to_string(case.path().join("constant").join(name)).unwrap();
        assert!(read("turbulenceProperties").contains("model           kEpsilon;"));
        assert!(read("thermophysicalProperties").contains("mu              2e-05;"));
        assert!(!read("thermophysicalProperties").contains("1.8e-05"));
    }

    #[test]
    fn the_schemes_of_a_case_are_shown_and_saved() {
        let case = case_dir();
        std::fs::write(
            case.path().join("system/fvSchemes"),
            "divSchemes\n{\n    div(phi,U)      bounded Gauss linearUpwind grad(U);\n}\n",
        )
        .unwrap();
        let ctx = Context::default();
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(case.path()).unwrap();
        app.show_tab(TabName::Schemes);
        frame(&mut app, &ctx);
        let Screen::Case(view) = &mut app.screen else {
            panic!("the case closed");
        };
        let groups = view.schemes.groups.as_mut().unwrap().as_mut().unwrap();
        assert_eq!(groups[0].title(), "divSchemes");
        groups[0].items[0].text = "bounded Gauss upwind".into();
        numerics::save(&view.dir, File::Schemes, groups).unwrap();
        let text = std::fs::read_to_string(case.path().join("system/fvSchemes")).unwrap();
        assert!(
            text.contains("div(phi,U)      bounded Gauss upwind;"),
            "{text}"
        );
    }

    #[test]
    fn the_mesh_of_a_case_is_read_and_drawn() {
        let case = case_dir();
        let from = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vortia-foamfile/tests/data/cavity-ascii/constant/polyMesh");
        let to = case.path().join("constant/polyMesh");
        std::fs::create_dir_all(&to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
        let ctx = Context::default();
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(case.path()).unwrap();
        app.show_mesh();
        let mut ready = false;
        for _ in 0..200 {
            frame(&mut app, &ctx);
            if let Screen::Case(view) = &app.screen
                && let Some(viewer) = &view.viewer
            {
                let viewer = viewer.as_ref().unwrap();
                assert!(viewer.surface().triangle_count() > 0);
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready, "the mesh was not read");
        // And it draws once it is there.
        frame(&mut app, &ctx);
    }

    #[test]
    fn the_mesh_is_coloured_with_a_field_of_the_case() {
        let case = case_dir();
        let data = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vortia-foamfile/tests/data/cavity-ascii");
        let to = case.path().join("constant/polyMesh");
        std::fs::create_dir_all(&to).unwrap();
        for entry in std::fs::read_dir(data.join("constant/polyMesh")).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
        std::fs::create_dir_all(case.path().join("0.01")).unwrap();
        for name in ["p", "U", "phi"] {
            std::fs::copy(
                data.join("0.01").join(name),
                case.path().join("0.01").join(name),
            )
            .unwrap();
        }
        let ctx = Context::default();
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(case.path()).unwrap();
        app.show_mesh_field("p");
        let mut seen = None;
        for _ in 0..300 {
            frame(&mut app, &ctx);
            if let Screen::Case(view) = &app.screen
                && let Some(Ok(viewer)) = &view.viewer
                && let Some(values) = viewer.values()
            {
                seen = Some(values.clone());
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let values = seen.expect("the field was not read").unwrap();
        assert_eq!((values.name.as_str(), values.time.as_str()), ("p", "0.01"));
        assert_eq!(values.kind, "scalar");
        assert_eq!(values.values.len(), 12);
        frame(&mut app, &ctx);
    }

    #[test]
    fn a_case_without_a_mesh_says_so_in_its_mesh_tab() {
        let case = case_dir();
        let ctx = Context::default();
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(case.path()).unwrap();
        app.show_mesh();
        for _ in 0..200 {
            frame(&mut app, &ctx);
            if let Screen::Case(view) = &app.screen
                && let Some(viewer) = &view.viewer
            {
                assert!(viewer.is_err());
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("no answer about the mesh");
    }

    #[test]
    fn every_screen_and_dialog_draws_in_both_themes() {
        let case = case_dir();
        for theme in [Theme::Light, Theme::Dark, Theme::System] {
            let ctx = Context::default();
            let mut app = VortiaApp::with_prefs(
                Prefs {
                    theme,
                    ..Prefs::default()
                },
                None,
            );
            frame(&mut app, &ctx);
            for dialog in [
                Dialog::Preferences,
                Dialog::Shortcuts,
                Dialog::About,
                Dialog::Report,
            ] {
                app.dialog = Some(dialog);
                frame(&mut app, &ctx);
            }
            app.dialog = None;
            app.open_case(case.path()).unwrap();
            frame(&mut app, &ctx);
            if let Screen::Case(view) = &mut app.screen {
                view.report = Convergence::parse(
                    r#"{"status":"stabilizing","automatic":true,"stableSamples":1,"requiredSamples":5,"globalMass":1e-4}"#,
                );
                view.log.push_back("Time = 1".into());
                view.outcome = Some(Outcome::Failed("boom".into()));
            }
            frame(&mut app, &ctx);
        }
    }

    #[test]
    fn a_run_that_cannot_start_the_solver_says_so_in_the_case() {
        let case = case_dir();
        let ctx = Context::default();
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(case.path()).unwrap();
        app.start_run(&ctx);
        let start = std::time::Instant::now();
        loop {
            frame(&mut app, &ctx);
            let Screen::Case(view) = &app.screen else {
                panic!("left the case");
            };
            if let Some(outcome) = &view.outcome {
                assert!(
                    matches!(outcome, Outcome::Failed(_)),
                    "the solver is not set up: {outcome:?}"
                );
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(60), "no outcome");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_case_folder_is_read_for_the_screen_and_again_after_a_run() {
        let case = case_dir();
        let ctx = Context::default();
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(case.path()).unwrap();
        let start = std::time::Instant::now();
        let wait_for_summary = |app: &mut VortiaApp| loop {
            frame(app, &ctx);
            if let Screen::Case(view) = &app.screen
                && view.summary.is_some()
            {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "no summary");
            std::thread::sleep(Duration::from_millis(10));
        };
        wait_for_summary(&mut app);
        let Screen::Case(view) = &mut app.screen else {
            panic!("left the case");
        };
        assert_eq!(
            view.summary.as_ref().and_then(|s| s.application.as_deref()),
            Some("vortiaCentralFoam")
        );
        // A run that ends marks the folder as changed; the next frame reads it again.
        view.summary = None;
        view.summary_stale = true;
        wait_for_summary(&mut app);
    }

    #[test]
    fn counts_are_grouped_by_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(7500), "7\u{202f}500");
        assert_eq!(grouped(1_234_567), "1\u{202f}234\u{202f}567");
    }

    #[test]
    fn statuses_and_numbers_read_well() {
        assert_eq!(status_label("converged"), ("Converged", Tone::Success));
        assert_eq!(status_label("invalid").1, Tone::Danger);
        assert_eq!(status_label("something new").1, Tone::Info);
        assert_eq!(scientific(None), "–");
        assert_eq!(scientific(Some(0.00123)), "1.23e-3");
    }

    #[test]
    fn the_log_keeps_only_its_last_lines() {
        let mut view = CaseView::new(PathBuf::from("/c"), None);

        for n in 0..MAX_LOG + 10 {
            view.push_log(n.to_string());
        }
        assert_eq!(view.log.len(), MAX_LOG);
        assert_eq!(view.log.front().map(String::as_str), Some("10"));
        assert_eq!(view.name(), "c");
    }
}
