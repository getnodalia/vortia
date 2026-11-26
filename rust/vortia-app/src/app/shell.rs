//! The window around a case, built from the engineering suite's shared parts
//! (`nodalia_theme::engineering`, specified in nodalia-design's `docs/suites/engineering.md`):
//! the title bar, a ribbon of workflow steps, the Browser with the case's tree, Properties,
//! the status bar with the run and its residuals, Home, and the command search.

use crate::i18n::{tr, trf};
use eframe::egui::{
    self, Color32, Context, Frame, Id, Key, Margin, Pos2, RichText, Sense, Vec2, ViewportCommand,
};
use nodalia_theme::engineering::{
    self as eng, Document, Look, TitleAction, TitleBar, TitleLabels, TreeHit, TreeRow, layout, menu,
};
use nodalia_theme::office::{self, Entry, Nav};
use nodalia_theme::{Mode, Palette, component, font_size, for_egui, space, window_controls};
use std::path::{Path, PathBuf};

use super::conditions_tab;
use super::newcase;
use super::results::Card;
use super::{
    CaseView, Dialog, Screen, Tab, Tone, VortiaApp, convergence_card, file_tab, grouped, mesh_tab,
    muted, own_controls, palette, results_tab, scientific, settings_tab, status_label, tone_colors,
};
use crate::numerics::File;
use crate::residuals::{Residuals, Trouble};
use crate::summary::Summary;
use crate::viewer::Source;
use vortia_post::BalanceHistory;

/// Something the user asked for, from the File menu, the ribbon or the command search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cmd {
    New,
    Open,
    Run,
    Pause,
    Stop,
    CloseCase,
    Duplicate,
    Quit,
    Go(Tab),
    Show(Dialog),
    CheckUpdates,
}

/// What the command search offers, in the order it lists them.
const COMMANDS: &[(&str, Cmd)] = &[
    ("Run the case", Cmd::Run),
    ("Pause or resume the run", Cmd::Pause),
    ("Stop the run", Cmd::Stop),
    ("New case", Cmd::New),
    ("Open a case", Cmd::Open),
    ("Close the case", Cmd::CloseCase),
    ("Duplicate the case", Cmd::Duplicate),
    ("Go to the run", Cmd::Go(Tab::Run)),
    ("Go to the mesh", Cmd::Go(Tab::Mesh)),
    ("Go to the results", Cmd::Go(Tab::Results)),
    ("Go to the boundary conditions", Cmd::Go(Tab::Conditions)),
    ("Go to the fluid", Cmd::Go(Tab::Fluid)),
    ("Go to the turbulence model", Cmd::Go(Tab::Turbulence)),
    ("Go to the viscosity", Cmd::Go(Tab::Transport)),
    ("Go to the run settings", Cmd::Go(Tab::Settings)),
    ("Go to the solver numbers", Cmd::Go(Tab::Numerics)),
    ("Go to the schemes", Cmd::Go(Tab::Schemes)),
    ("Preferences", Cmd::Show(Dialog::Preferences)),
    ("Keyboard shortcuts", Cmd::Show(Dialog::Shortcuts)),
    ("About Vortia", Cmd::Show(Dialog::About)),
    ("Check for updates…", Cmd::CheckUpdates),
    ("Report a problem", Cmd::Show(Dialog::Report)),
];

/// A workflow step of the ribbon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Case,
    Mesh,
    Setup,
    Results,
}

impl Step {
    const ALL: [Step; 4] = [Step::Case, Step::Mesh, Step::Setup, Step::Results];

    fn name(self) -> &'static str {
        match self {
            Step::Case => tr("Case"),
            Step::Mesh => tr("Mesh"),
            Step::Setup => tr("Setup"),
            Step::Results => tr("Results"),
        }
    }

    /// The view a step opens on.
    fn first(self) -> Tab {
        match self {
            Step::Case => Tab::Run,
            Step::Mesh => Tab::Mesh,
            Step::Setup => Tab::Conditions,
            Step::Results => Tab::Results,
        }
    }

    fn of(tab: Tab) -> Step {
        match tab {
            Tab::Run => Step::Case,
            Tab::Mesh => Step::Mesh,
            Tab::Results => Step::Results,
            Tab::Conditions
            | Tab::Fluid
            | Tab::Turbulence
            | Tab::Transport
            | Tab::Numerics
            | Tab::Schemes
            | Tab::Settings => Step::Setup,
        }
    }
}

/// What is picked in the Browser besides a view.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(super) enum Selection {
    #[default]
    Nothing,
    Patch(String),
}

/// The name a view carries over the main island.
fn view_title(tab: Tab) -> &'static str {
    match tab {
        Tab::Run => tr("Run"),
        Tab::Mesh => tr("Mesh"),
        Tab::Results => tr("Results"),
        Tab::Conditions => tr("Boundary conditions"),
        Tab::Numerics => tr("Solver numbers"),
        Tab::Schemes => tr("Schemes"),
        Tab::Settings => tr("Run settings"),
        Tab::Fluid => tr("Fluid"),
        Tab::Turbulence => tr("Turbulence"),
        Tab::Transport => tr("Viscosity"),
    }
}

fn gap() -> i8 {
    layout::GAP as i8
}

fn look_of(ctx: &Context) -> Look {
    Look::new(palette(ctx))
}

impl VortiaApp {
    /// The whole window under the title bar: Home, or the case.
    pub(super) fn shell(&mut self, ui: &mut egui::Ui, ctx: &Context, maximized: bool) {
        let look = look_of(ctx);
        let p = look.palette;
        egui::Panel::top("vortia_title")
            .exact_size(layout::TITLE_BAR)
            .show_separator_line(false)
            .frame(Frame::NONE)
            .show(ui, |ui| self.title_bar(ui, ctx, &look, maximized));
        if self.home || matches!(self.screen, Screen::Welcome) {
            self.home_page(ui, ctx, &look);
        } else {
            let command = self.case_page(ui, ctx, &look, &p);
            if let Some(command) = command {
                self.perform(ctx, command);
            }
        }
        self.search_dialog(ctx, &look);
    }

    /// Does what was asked for.
    pub(super) fn perform(&mut self, ctx: &Context, command: Cmd) {
        match command {
            Cmd::New => {
                self.home = true;
                self.notice = None;
                self.new_case = Some(newcase::NewCase::new(&newcase::NewCase::default_base()));
            }
            Cmd::Open => {
                self.home = true;
                self.new_case = None;
                self.focus_open = true;
            }
            Cmd::Run => {
                self.start_run(ctx);
                if let Screen::Case(case) = &mut self.screen {
                    case.tab = Tab::Run;
                    self.home = false;
                }
            }
            Cmd::Pause => self.toggle_pause(),
            Cmd::Stop => self.stop_run(),
            Cmd::CloseCase => {
                self.close_case();
                self.home = matches!(self.screen, Screen::Welcome) || self.home;
            }
            Cmd::Duplicate => self.duplicate_case(),
            Cmd::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            Cmd::Go(tab) => {
                if let Screen::Case(case) = &mut self.screen {
                    case.go(tab);
                    self.home = false;
                }
            }
            Cmd::Show(dialog) => self.dialog = Some(dialog),
            Cmd::CheckUpdates => self.updater.check_now(ctx),
        }
    }

    // --- Title bar ---------------------------------------------------------------------------

    fn title_bar(&mut self, ui: &mut egui::Ui, ctx: &Context, look: &Look, maximized: bool) {
        let name = match &self.screen {
            Screen::Case(case) => Some(case.name()),
            Screen::Welcome => None,
        };
        let documents: Vec<Document> = name
            .iter()
            .map(|name| Document {
                name,
                modified: false,
            })
            .collect();
        // Nodalia's controls lead the bar when the desktop keeps its buttons at the left, and end
        // it otherwise.
        let side = nodalia_theme::titlebar::system_side();
        let controls_left = own_controls() && side == nodalia_theme::titlebar::Side::Left;
        let controls_room = if own_controls() {
            window_controls::WIDTH + 2.0 * space::SM
        } else {
            0.0
        };
        let controls_width = if controls_left { 0.0 } else { controls_room };
        // macOS: the window's content runs under its title bar, so the system's traffic lights sit
        // over the left end of this bar and the bar keeps that room free. They are gone in full
        // screen. The bar's own content starts 8 points in, which is part of the room.
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        let lights_room = if fullscreen {
            0.0
        } else {
            super::lights_room()
        };
        let icon = self.icon(ctx).id();
        let bar = TitleBar {
            app: "Vortia",
            icon: Some(icon),
            documents: &documents,
            active: (!self.home && !documents.is_empty()).then_some(0),
            labels: TitleLabels {
                home: tr("Home"),
                file: tr("File"),
                save: tr("Save"),
                undo: tr("Undo"),
                redo: tr("Redo"),
                new_document: tr("Open a case"),
                close_document: tr("Close the case"),
                search: tr("Search commands"),
                menu: tr("Menu"),
            },
            quick_access: &[],
            controls_width,
            leading_width: if controls_left {
                controls_room
            } else {
                lights_room
            },
        };
        let out = eng::title_bar(ui, look, &bar);
        if own_controls() || super::lights_in_bar() {
            if out.drag.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
            if out.drag.double_clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            }
        }
        if own_controls() {
            let focused = ctx.input(|i| i.focused);
            let mode = if ctx.global_style().visuals.dark_mode {
                Mode::Dark
            } else {
                Mode::Light
            };
            let labels = window_controls::Labels {
                minimize: tr("Minimize"),
                maximize: tr("Maximize"),
                restore: tr("Restore"),
                close: tr("Close"),
            };
            let controls = if controls_left {
                let bar = ui.max_rect();
                nodalia_theme::titlebar::Placement::nodalia(side).controls_rect(
                    egui::Rect::from_min_max(
                        Pos2::new(bar.left(), out.controls.top()),
                        Pos2::new(bar.right(), out.controls.bottom()),
                    ),
                )
            } else {
                out.controls
            };
            let action = window_controls::show_on(
                ui,
                controls,
                side,
                &look.palette,
                mode,
                focused,
                maximized,
                labels,
            );
            if let Some(action) = action {
                ctx.send_viewport_cmd(match action {
                    window_controls::Action::Minimize => ViewportCommand::Minimized(true),
                    window_controls::Action::Maximize => ViewportCommand::Maximized(true),
                    window_controls::Action::Restore => ViewportCommand::Maximized(false),
                    window_controls::Action::Close => ViewportCommand::Close,
                });
            }
        }
        let bottom = ui.min_rect().bottom();
        let at = |x: f32| Pos2::new(x, bottom);
        let pointer_x = ctx.pointer_interact_pos().map(|p| p.x);
        match out.action {
            Some(TitleAction::Home) => self.home = true,
            Some(TitleAction::File) => {
                let x = pointer_x.map_or(0.0, |x| x - 20.0);
                ctx.data_mut(|d| d.insert_temp(file_menu().with("at"), at(x)));
                egui::Popup::toggle_id(ctx, file_menu());
            }
            Some(TitleAction::Menu) => {
                let x = pointer_x.unwrap_or(0.0);
                ctx.data_mut(|d| d.insert_temp(app_menu().with("at"), at(x)));
                egui::Popup::toggle_id(ctx, app_menu());
            }
            // Vortia's editors save themselves (each has its own Save), and there is no
            // document-wide history.
            Some(TitleAction::Save | TitleAction::Undo | TitleAction::Redo) => {}
            Some(TitleAction::Document(_)) => self.home = false,
            Some(TitleAction::CloseDocument(_)) => self.perform(ctx, Cmd::CloseCase),
            Some(TitleAction::NewDocument) => self.perform(ctx, Cmd::Open),
            Some(TitleAction::Search) => self.search = Some(String::new()),
            None => {}
        }
        self.title_menus(ui, ctx, look);
    }

    /// The File menu and the app's menu, opened from the title bar. Both are the suite's menu
    /// (`engineering::menu`), the same surface Torno and Forja open from the same buttons.
    fn title_menus(&mut self, ui: &egui::Ui, ctx: &Context, look: &Look) {
        let mut chosen = None;
        let mut recent: Option<PathBuf> = None;
        let at = |id: Id| {
            ctx.data(|d| d.get_temp::<Pos2>(id.with("at")))
                .unwrap_or_default()
        };
        let case_open = matches!(self.screen, Screen::Case(_));
        let running = matches!(&self.screen, Screen::Case(case) if case.running());
        let popup = |id: Id| {
            menu::popup(
                egui::Popup::new(id, ctx.clone(), at(id), ui.layer_id())
                    .kind(egui::PopupKind::Menu)
                    .open_memory(None),
                look,
            )
        };
        popup(file_menu()).show(|ui| {
            ui.set_min_width(menu::metric::MIN_WIDTH);
            let mut item = |ui: &mut egui::Ui, row: menu::Item, command: Cmd| {
                if menu::item(ui, look, &row).clicked() {
                    chosen = Some(command);
                    ui.close();
                }
            };
            item(ui, menu::Item::new(tr("New case")).icon("file"), Cmd::New);
            item(
                ui,
                menu::Item::new(tr("Open a case")).icon("folder"),
                Cmd::Open,
            );
            menu::submenu(
                ui,
                look,
                &menu::Item::new(tr("Open recent"))
                    .icon("history")
                    .enabled(!self.prefs.recent.is_empty()),
                |ui| {
                    for dir in &self.prefs.recent {
                        let name = dir.file_name().map_or_else(
                            || dir.display().to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                        if menu::item(ui, look, &menu::Item::new(&name))
                            .on_hover_text(dir.display().to_string())
                            .clicked()
                        {
                            recent = Some(dir.clone());
                            ui.close();
                        }
                    }
                },
            );
            menu::separator(ui, look);
            item(
                ui,
                menu::Item::new(tr("Run"))
                    .icon("play")
                    .enabled(case_open && !running),
                Cmd::Run,
            );
            item(
                ui,
                menu::Item::new(tr("Pause or resume"))
                    .icon("pause")
                    .enabled(running),
                Cmd::Pause,
            );
            item(
                ui,
                menu::Item::new(tr("Stop")).icon("stop").enabled(running),
                Cmd::Stop,
            );
            menu::separator(ui, look);
            item(
                ui,
                menu::Item::new(tr("Duplicate the case"))
                    .icon("copy")
                    .enabled(case_open),
                Cmd::Duplicate,
            );
            item(
                ui,
                menu::Item::new(tr("Close the case"))
                    .icon("close")
                    .enabled(case_open && !running),
                Cmd::CloseCase,
            );
            menu::separator(ui, look);
            item(ui, menu::Item::new(tr("Quit")).icon("log-out"), Cmd::Quit);
        });
        popup(app_menu()).show(|ui| {
            ui.set_min_width(menu::metric::MIN_WIDTH);
            for (label, icon, dialog) in [
                (tr("Preferences"), "settings", Dialog::Preferences),
                (tr("Keyboard shortcuts"), "keyboard", Dialog::Shortcuts),
                (tr("Report a problem"), "flag", Dialog::Report),
                (tr("About Vortia"), "info", Dialog::About),
            ] {
                if menu::item(ui, look, &menu::Item::new(label).icon(icon)).clicked() {
                    chosen = Some(Cmd::Show(dialog));
                    ui.close();
                }
            }
            menu::separator(ui, look);
            if menu::item(
                ui,
                look,
                &menu::Item::new(tr("Check for updates…")).icon("refresh"),
            )
            .clicked()
            {
                chosen = Some(Cmd::CheckUpdates);
                ui.close();
            }
        });
        if let Some(command) = chosen {
            self.perform(ctx, command);
        }
        if let Some(dir) = recent
            && let Err(message) = self.open_case(&dir)
        {
            self.notice = Some(message);
        }
    }

    // --- Command search ----------------------------------------------------------------------

    fn search_dialog(&mut self, ctx: &Context, look: &Look) {
        let Some(query) = self.search.as_mut() else {
            return;
        };
        let p = &look.palette;
        let mut chosen = None;
        let mut close = false;
        egui::Modal::new(Id::NULL.with("command-search")).show(ctx, |ui| {
            ui.set_width(nodalia_theme::size::DIALOG);
            let field = ui.add(
                egui::TextEdit::singleline(query)
                    .hint_text(tr("Search commands"))
                    .desired_width(f32::INFINITY),
            );
            field.request_focus();
            ui.add_space(space::SM);
            let needle = query.trim().to_lowercase();
            let hits: Vec<&(&str, Cmd)> = COMMANDS
                .iter()
                .filter(|(label, _)| {
                    needle.is_empty() || tr(label).to_lowercase().contains(&needle)
                })
                .collect();
            for (i, (label, command)) in hits.iter().enumerate() {
                let row = ui.add(
                    egui::Button::new(
                        RichText::new(tr(label)).color(for_egui::color(p.text_primary)),
                    )
                    .frame(false)
                    .min_size(Vec2::new(ui.available_width(), layout::TREE_ROW)),
                );
                if row.clicked() || (i == 0 && ctx.input(|i| i.key_pressed(Key::Enter))) {
                    chosen = Some(*command);
                }
            }
            if hits.is_empty() {
                ui.label(RichText::new(tr("Nothing matches.")).color(muted(p)));
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                close = true;
            }
        });
        if chosen.is_some() || close {
            self.search = None;
        }
        if let Some(command) = chosen {
            self.perform(ctx, command);
        }
    }

    // --- Home --------------------------------------------------------------------------------

    fn home_page(&mut self, ui: &mut egui::Ui, ctx: &Context, look: &Look) {
        let home = eng::home_look(look);
        let p = &look.palette;
        let has_case = matches!(self.screen, Screen::Case(_));
        let nav = Nav {
            back: if has_case { tr("Back to the case") } else { "" },
            top: vec![
                Entry::item("home", tr("Home"), "home"),
                Entry::item("new", tr("New"), "plus"),
                Entry::item("open", tr("Open"), "folder"),
            ],
            bottom: vec![Entry::item("preferences", tr("Preferences"), "settings")],
            selected: Some(if self.new_case.is_some() {
                "new"
            } else {
                "home"
            }),
        };
        let icon = self.icon(ctx).id();
        let version = trf("Version {}", &[&env!("CARGO_PKG_VERSION")]);
        let focus = std::mem::take(&mut self.focus_open);
        let recent: Vec<(String, String)> = self
            .prefs
            .recent
            .iter()
            .map(|dir| {
                let name = dir.file_name().map_or_else(
                    || dir.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                let folder = dir
                    .parent()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default();
                (name, folder)
            })
            .collect();
        let mut open = false;
        let mut picked = None;
        let mut create = false;
        let mut stl_path = None;
        let mut close_stl = false;
        let new_case = &mut self.new_case;
        let stl = &mut self.stl;
        let stl_input = &mut self.stl_input;
        let nav_pick = office::file_view(ui, &home, &nav, |ui| {
            eng::home_header(ui, look, Some(icon), "Vortia", &version);
            if let Some(form) = new_case {
                create = matches!(form.ui(ui, look, p), newcase::Ask::Create);
                if let Some(notice) = &self.notice {
                    ui.add_space(space::SM);
                    ui.colored_label(for_egui::color(p.status_danger_fg), notice);
                }
                return;
            }
            if let Some(view) = stl {
                close_stl = stl_page(ui, look, view);
                return;
            }
            office::section(ui, &home, tr("Open a case"), |ui| {
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 120.0).clamp(160.0, 420.0);
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.path_input)
                            .hint_text(tr("Folder of a case"))
                            .desired_width(width),
                    );
                    if focus {
                        field.request_focus();
                    }
                    open |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    open |= super::accent_button(ui, p, tr("Open case")).clicked();
                });
                if let Some(notice) = &self.notice {
                    ui.add_space(space::SM);
                    ui.colored_label(for_egui::color(p.status_danger_fg), notice);
                }
            });
            office::section(ui, &home, tr("Open a geometry"), |ui| {
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 120.0).clamp(160.0, 420.0);
                    let field = ui.add(
                        egui::TextEdit::singleline(stl_input)
                            .hint_text(tr("An STL, OBJ or STEP file, a Fluent case (.cas.h5, .msh) or a Workbench project (.wbpj)"))
                            .desired_width(width),
                    );
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if (super::accent_button(ui, p, tr("Show geometry")).clicked() || enter)
                        && !stl_input.trim().is_empty()
                    {
                        stl_path = Some(PathBuf::from(stl_input.trim()));
                    }
                });
            });
            office::section(ui, &home, tr("Recent"), |ui| {
                let docs: Vec<eng::RecentDocument> = recent
                    .iter()
                    .map(|(name, folder)| eng::RecentDocument {
                        name,
                        folder,
                        when: "",
                        icon: "nozzle",
                        thumbnail: None,
                    })
                    .collect();
                picked = eng::recent_documents(
                    ui,
                    look,
                    &docs,
                    tr("Nothing here yet. Open a case folder to get started."),
                );
            });
        });
        if let Some(path) = stl_path {
            self.show_stl(ctx, path);
        }
        if close_stl {
            self.stl = None;
        }
        match nav_pick.as_deref() {
            Some(office::BACK) => self.home = false,
            Some("home") => {
                self.new_case = None;
                self.stl = None;
            }
            Some("new") if self.new_case.is_none() => {
                self.notice = None;
                self.new_case = Some(newcase::NewCase::new(&newcase::NewCase::default_base()));
            }
            Some("open") => {
                self.new_case = None;
                self.focus_open = true;
            }
            Some("preferences") => self.dialog = Some(Dialog::Preferences),
            _ => {}
        }
        if create && let Some(form) = &self.new_case {
            match form.create() {
                Ok(dir) => match self.open_case(&dir) {
                    Ok(()) => self.new_case = None,
                    Err(message) => self.notice = Some(message),
                },
                Err(message) => self.notice = Some(message),
            }
        }
        if open {
            let dir = std::path::PathBuf::from(self.path_input.trim());
            match self.open_case(&dir) {
                Ok(()) => self.path_input.clear(),
                Err(message) => self.notice = Some(message),
            }
        }
        if let Some(dir) = picked.and_then(|i| self.prefs.recent.get(i).cloned())
            && let Err(message) = self.open_case(&dir)
        {
            self.notice = Some(message);
        }
    }

    // --- The case ----------------------------------------------------------------------------

    fn case_page(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        look: &Look,
        p: &Palette,
    ) -> Option<Cmd> {
        let g = gap();
        let island = || eng::island(look);
        let mut command = None;
        let Screen::Case(case) = &mut self.screen else {
            return None;
        };

        egui::Panel::bottom("vortia_status")
            .exact_size(layout::STATUS_BAR)
            .show_separator_line(false)
            .frame(Frame::NONE)
            .show(ui, |ui| status_bar(ui, look, case));

        egui::Panel::top("vortia_ribbon")
            .exact_size(layout::RIBBON + 12.0 + layout::GAP)
            .show_separator_line(false)
            .frame(island().outer_margin(Margin {
                left: g,
                right: g,
                top: 0,
                bottom: g,
            }))
            .show(ui, |ui| command = ribbon(ui, look, case));

        egui::Panel::left("vortia_browser")
            .exact_size(layout::BROWSER_WIDTH + 16.0 + layout::GAP)
            .resizable(false)
            .show_separator_line(false)
            .frame(island().outer_margin(Margin {
                left: g,
                right: 0,
                top: 0,
                bottom: g,
            }))
            .show(ui, |ui| browser(ui, look, case));

        egui::Panel::right("vortia_properties")
            .exact_size(layout::PROPERTIES_WIDTH + 16.0 + layout::GAP)
            .resizable(false)
            .show_separator_line(false)
            .frame(island().outer_margin(Margin {
                left: g,
                right: g,
                top: 0,
                bottom: g,
            }))
            .show(ui, |ui| properties(ui, look, case));

        egui::CentralPanel::default()
            .frame(
                island()
                    .inner_margin(Margin::same(12))
                    .outer_margin(Margin {
                        left: g,
                        right: 0,
                        top: 0,
                        bottom: g,
                    }),
            )
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                case_view(ui, ctx, look, p, case);
            });
        command
    }
}

fn file_menu() -> Id {
    Id::NULL.with("vortia_file_menu")
}

fn app_menu() -> Id {
    Id::NULL.with("vortia_app_menu")
}

// --- Ribbon ----------------------------------------------------------------------------------

/// The workflow steps and their tools. Returns what a tool asked the app to do.
fn ribbon(ui: &mut egui::Ui, look: &Look, case: &mut CaseView) -> Option<Cmd> {
    let mut command = None;
    let step = Step::of(case.tab);
    ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
    ui.horizontal(|ui| {
        for candidate in Step::ALL {
            if eng::ribbon_tab(ui, look, candidate.name(), candidate == step).clicked()
                && candidate != step
            {
                case.go(candidate.first());
            }
        }
    });
    let running = case.running();
    let stopping = case.stopping;
    let paused = case.session.as_ref().is_some_and(|s| s.is_paused());
    let tab = case.tab;
    let mut go = None;
    ui.horizontal(|ui| {
        let tool = |ui: &mut egui::Ui, icon: &str, text: &str, active: bool, enabled: bool| {
            ui.add_enabled_ui(enabled, |ui| match volume(icon) {
                Some(name) => eng::ribbon_tool_3d(ui, look, name, text, active),
                None => eng::ribbon_tool(ui, look, icon, text, active),
            })
            .inner
            .clicked()
        };
        match step {
            Step::Case => {
                eng::ribbon_group(ui, look, tr("Run"), |ui| {
                    if tool(ui, "run", tr("Run"), running, !running) {
                        command = Some(Cmd::Run);
                    }
                    let (icon, text) = if paused {
                        ("play", tr("Resume"))
                    } else {
                        ("pause", tr("Pause"))
                    };
                    if tool(ui, icon, text, paused, running && !stopping) {
                        command = Some(Cmd::Pause);
                    }
                    if tool(ui, "stop", tr("Stop"), false, running && !stopping) {
                        command = Some(Cmd::Stop);
                    }
                });
                eng::ribbon_group(ui, look, tr("Case"), |ui| {
                    if tool(ui, "folder", tr("Open"), false, !running) {
                        command = Some(Cmd::Open);
                    }
                    if tool(ui, "close", tr("Close"), false, !running) {
                        command = Some(Cmd::CloseCase);
                    }
                });
            }
            Step::Mesh => {
                eng::ribbon_group(ui, look, tr("Mesh"), |ui| {
                    if tool(ui, "mesh", tr("Surface"), tab == Tab::Mesh, true) {
                        go = Some(Tab::Mesh);
                    }
                });
            }
            Step::Setup => {
                eng::ribbon_group(ui, look, tr("Physics"), |ui| {
                    if tool(
                        ui,
                        "boundary",
                        tr("Conditions"),
                        tab == Tab::Conditions,
                        true,
                    ) {
                        go = Some(Tab::Conditions);
                    }
                    for (icon, text, target, file) in PHYSICS {
                        let present = has_constant(case, file);
                        if tool(ui, icon, tr(text), tab == target, present) {
                            go = Some(target);
                        }
                    }
                });
                eng::ribbon_group(ui, look, tr("Solver"), |ui| {
                    for (icon, text, target) in [
                        ("settings", tr("Settings"), Tab::Settings),
                        ("solver", tr("Numerics"), Tab::Numerics),
                        ("parameters", tr("Schemes"), Tab::Schemes),
                    ] {
                        if tool(ui, icon, text, tab == target, true) {
                            go = Some(target);
                        }
                    }
                });
            }
            Step::Results => {
                let mut show = None;
                eng::ribbon_group(ui, look, tr("Fields"), |ui| {
                    if tool(ui, "cfd-results", tr("Overview"), false, true) {
                        show = Some(Card::Overview);
                    }
                    if tool(ui, "cfd-wall", tr("Walls"), false, true) {
                        show = Some(Card::Walls);
                    }
                    if tool(ui, "cfd-geometry", tr("Forces"), false, true) {
                        show = Some(Card::Forces);
                    }
                });
                eng::ribbon_group(ui, look, tr("Sampling"), |ui| {
                    if tool(ui, "cfd-probe", tr("Probes"), false, true) {
                        show = Some(Card::Probe);
                    }
                    if tool(ui, "cfd-sample-line", tr("Line"), false, true) {
                        show = Some(Card::Line);
                    }
                    if tool(ui, "cfd-slice", tr("Section"), false, true) {
                        show = Some(Card::Section);
                    }
                });
                eng::ribbon_group(ui, look, tr("Report"), |ui| {
                    if tool(ui, "cfd-residuals", tr("Compare"), false, true) {
                        show = Some(Card::Compare);
                    }
                    if tool(ui, "cfd-report", tr("Save report"), false, true) {
                        case.results.ask_for_report();
                        go = Some(Tab::Results);
                    }
                });
                if let Some(card) = show {
                    case.results.show(card);
                    go = Some(Tab::Results);
                }
            }
        }
    });
    if let Some(tab) = go {
        case.go(tab);
    }
    command
}

// --- Browser ---------------------------------------------------------------------------------

/// What a Browser row opens.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pick {
    View(Tab),
    Patch(String),
    Field(String),
}

struct Row {
    /// The group this row opens and closes, for a row with children.
    group: Option<&'static str>,
    depth: usize,
    icon: &'static str,
    label: String,
    selected: bool,
    muted: bool,
    pick: Pick,
}

/// The volumetric NDL icon a tool is drawn with in the ribbon; the Browser's small rows keep the
/// flat icon of the same concept.
fn volume(flat: &str) -> Option<&str> {
    // Only the objects of a CFD case are drawn in volume. Run, pause, stop, open, close and
    // settings are interface actions and stay flat, like in every other Nodalia app.
    Some(match flat {
        "mesh" => "cfd-mesh",
        "boundary" => "cfd-boundary",
        "material" => "cfd-fluid",
        "waveform" => "cfd-turbulence",
        "droplet" => "cfd-viscosity",
        "solver" => "cfd-numerics",
        "parameters" => "cfd-schemes",
        "probe" => "cfd-probe",
        // The Results step already names its volumetric icons.
        name if name.starts_with("cfd-") => name,
        _ => return None,
    })
}

/// The dictionaries of `constant` that describe the physics, with the view that edits each.
const PHYSICS: [(&str, &str, Tab, File); 3] = [
    ("material", "Fluid", Tab::Fluid, File::Fluid),
    ("waveform", "Turbulence", Tab::Turbulence, File::Turbulence),
    ("droplet", "Viscosity", Tab::Transport, File::Transport),
];

/// Whether the case has the physics dictionary (it is read in with the rest of the folder).
fn has_constant(case: &CaseView, file: File) -> bool {
    case.summary
        .as_ref()
        .is_some_and(|s| s.constant.iter().any(|n| n == file.name()))
}

/// The rows of the case's tree: the case, its mesh and patches, the physics and its fields, the
/// solver's files, the run and the results.
fn tree(case: &CaseView) -> Vec<Row> {
    let open = |group: &str| !case.collapsed.contains(group);
    let nothing = case.selection == Selection::Nothing;
    let view = |tab: Tab| case.tab == tab && nothing;
    let mut rows = vec![Row {
        group: Some("case"),
        depth: 0,
        icon: "folder",
        label: case.name(),
        selected: false,
        muted: false,
        pick: Pick::View(Tab::Run),
    }];
    if !open("case") {
        return rows;
    }
    let summary = case.summary.as_ref();
    rows.push(Row {
        group: Some("mesh"),
        depth: 1,
        icon: "mesh",
        label: tr("Mesh").into(),
        selected: view(Tab::Mesh),
        muted: false,
        pick: Pick::View(Tab::Mesh),
    });
    if open("mesh")
        && let Some(Ok(mesh)) = summary.and_then(|s| s.mesh.as_ref())
    {
        for patch in &mesh.patches {
            rows.push(Row {
                group: None,
                depth: 2,
                icon: "boundary",
                label: patch.name.clone(),
                selected: case.selection == Selection::Patch(patch.name.clone()),
                muted: matches!(patch.kind.as_str(), "empty" | "wedge"),
                pick: Pick::Patch(patch.name.clone()),
            });
        }
    }
    rows.push(Row {
        group: Some("physics"),
        depth: 1,
        icon: "boundary",
        label: tr("Boundary conditions").into(),
        selected: view(Tab::Conditions) && case.conditions_field.is_none(),
        muted: false,
        pick: Pick::View(Tab::Conditions),
    });
    if open("physics") {
        for (icon, label, tab, file) in PHYSICS {
            if has_constant(case, file) {
                rows.push(Row {
                    group: None,
                    depth: 2,
                    icon,
                    label: tr(label).into(),
                    selected: view(tab),
                    muted: false,
                    pick: Pick::View(tab),
                });
            }
        }
        for field in summary.map(|s| s.fields.as_slice()).unwrap_or_default() {
            rows.push(Row {
                group: None,
                depth: 2,
                icon: "contour",
                label: field.clone(),
                selected: view(Tab::Conditions) && case.conditions_field.as_ref() == Some(field),
                muted: false,
                pick: Pick::Field(field.clone()),
            });
        }
    }
    rows.push(Row {
        group: Some("solver"),
        depth: 1,
        icon: "solver",
        label: tr("Solver").into(),
        selected: false,
        muted: false,
        pick: Pick::View(Tab::Settings),
    });
    if open("solver") {
        for (icon, label, tab) in [
            ("settings", tr("Run settings"), Tab::Settings),
            ("parameters", tr("Numerics"), Tab::Numerics),
            ("parameters", tr("Schemes"), Tab::Schemes),
        ] {
            rows.push(Row {
                group: None,
                depth: 2,
                icon,
                label: label.into(),
                selected: view(tab),
                muted: false,
                pick: Pick::View(tab),
            });
        }
    }
    rows.push(Row {
        group: None,
        depth: 1,
        icon: "run",
        label: tr("Run").into(),
        selected: view(Tab::Run),
        muted: false,
        pick: Pick::View(Tab::Run),
    });
    rows.push(Row {
        group: None,
        depth: 1,
        icon: "chart",
        label: tr("Results").into(),
        selected: view(Tab::Results),
        muted: false,
        pick: Pick::View(Tab::Results),
    });
    rows
}

fn browser(ui: &mut egui::Ui, look: &Look, case: &mut CaseView) {
    eng::panel_header(ui, look, tr("Browser"), |ui| {
        if let Some(Ok(mesh)) = case.summary.as_ref().and_then(|s| s.mesh.as_ref()) {
            ui.label(
                RichText::new(trf("{} cells", &[&grouped(mesh.cells)]))
                    .font(eng::caption())
                    .color(for_egui::color(look.palette.text_secondary)),
            );
        }
    });
    let rows = tree(case);
    let mut pick = None;
    let mut toggle = None;
    egui::ScrollArea::vertical()
        .id_salt("vortia-browser")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for row in &rows {
                let expanded = row.group.map(|g| !case.collapsed.contains(g));
                let shown = TreeRow {
                    depth: row.depth,
                    expanded,
                    icon: row.icon,
                    label: &row.label,
                    selected: row.selected,
                    muted: row.muted,
                };
                let (_, hit) = eng::tree_row(ui, look, &shown);
                match hit {
                    TreeHit::Row => pick = Some(row.pick.clone()),
                    TreeHit::Disclosure => toggle = row.group,
                    TreeHit::None => {}
                }
            }
        });
    if let Some(group) = toggle
        && !case.collapsed.remove(group)
    {
        case.collapsed.insert(group);
    }
    match pick {
        Some(Pick::View(tab)) => case.go(tab),
        Some(Pick::Patch(name)) => {
            case.go(Tab::Mesh);
            case.selection = Selection::Patch(name);
        }
        Some(Pick::Field(field)) => {
            case.go(Tab::Conditions);
            if case.conditions_field.as_ref() != Some(&field) {
                case.conditions_field = Some(field);
                case.conditions = None;
                case.conditions_message = None;
            }
        }
        None => {}
    }
}

// --- Properties ------------------------------------------------------------------------------

fn properties(ui: &mut egui::Ui, look: &Look, case: &CaseView) {
    let title = match &case.selection {
        Selection::Patch(_) => tr("Patch"),
        Selection::Nothing => tr("Case"),
    };
    eng::panel_header(ui, look, title, |_| {});
    egui::ScrollArea::vertical()
        .id_salt("vortia-properties")
        .auto_shrink([false, false])
        .show(ui, |ui| match (&case.selection, case.summary.as_ref()) {
            (_, None) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(tr("Reading the case…")).color(muted(&look.palette)));
                });
            }
            (Selection::Patch(name), Some(summary)) => patch_properties(ui, look, summary, name),
            (Selection::Nothing, Some(summary)) => case_properties(ui, look, summary),
        });
}

fn case_properties(ui: &mut egui::Ui, look: &Look, s: &Summary) {
    let p = &look.palette;
    eng::properties(ui, "case", |ui| {
        let row = |ui: &mut egui::Ui, name: &str, text: String| {
            eng::property(ui, look, name, |ui| eng::value(ui, look, &text));
        };
        if let Some(app) = &s.application {
            row(ui, tr("Solver"), app.clone());
        }
        if let (Some(start), Some(end)) = (s.start_time, s.end_time) {
            row(ui, tr("Time"), trf("{} to {} s", &[&start, &end]));
        }
        if let Some(dt) = s.delta_t {
            row(ui, tr("Time step"), format!("{dt} s"));
        }
        if let Some(every) = s.write_interval {
            row(ui, tr("Writes every"), every.to_string());
        }
        if let Some(Ok(mesh)) = &s.mesh {
            row(ui, tr("Cells"), grouped(mesh.cells));
            row(ui, tr("Faces"), grouped(mesh.faces));
            row(ui, tr("Points"), grouped(mesh.points));
            row(ui, tr("Patches"), mesh.patches.len().to_string());
        }
        if let Some(last) = s.times.last() {
            row(ui, tr("Written up to"), format!("{last} s"));
        }
    });
    match &s.mesh {
        None => {
            ui.add_space(space::SM);
            ui.label(RichText::new(tr("The case has no mesh yet.")).color(muted(p)));
        }
        Some(Err(message)) => {
            ui.add_space(space::SM);
            ui.colored_label(for_egui::color(p.status_danger_fg), message);
        }
        Some(Ok(_)) => {}
    }
}

fn patch_properties(ui: &mut egui::Ui, look: &Look, s: &Summary, name: &str) {
    let patch = s
        .mesh
        .as_ref()
        .and_then(|m| m.as_ref().ok())
        .and_then(|m| m.patches.iter().find(|p| p.name == name));
    let Some(patch) = patch else {
        ui.label(RichText::new(tr("This patch is not in the mesh.")).color(muted(&look.palette)));
        return;
    };
    eng::properties(ui, "patch", |ui| {
        eng::property(ui, look, tr("Name"), |ui| eng::value(ui, look, &patch.name));
        eng::property(ui, look, tr("Type"), |ui| eng::value(ui, look, &patch.kind));
        eng::property(ui, look, tr("Faces"), |ui| {
            eng::value(ui, look, &grouped(patch.faces))
        });
    });
    if patch.kind == "wall" {
        ui.add_space(space::SM);
        ui.label(
            RichText::new(tr(tr(
                "Shear, y+, forces and heat flux of this wall are under Results.",
            )))
            .font(eng::caption())
            .color(muted(&look.palette)),
        );
    }
}

// --- The main island -------------------------------------------------------------------------

/// The view the Browser and the ribbon picked, under its title.
fn case_view(ui: &mut egui::Ui, ctx: &Context, look: &Look, p: &Palette, case: &mut CaseView) {
    let running = case.running();
    eng::panel_header(ui, look, view_title(case.tab), |ui| {
        if running {
            ui.label(RichText::new(tr("Running…")).color(muted(p)));
            ui.spinner();
        }
    });
    if let Some(outcome) = &case.outcome {
        use crate::session::Outcome;
        let (tone, text) = match outcome {
            Outcome::Failed(message) => (p.status_danger_fg, message.as_str()),
            Outcome::Stopped => (
                p.status_warning_fg,
                tr("The run was stopped; what was written before stays in the case."),
            ),
            Outcome::Done => (p.status_success_fg, tr("The run finished.")),
        };
        ui.colored_label(for_egui::color(tone), text);
        ui.add_space(space::SM);
    }
    case.ensure_viewer(ctx);
    case.ensure_conditions(ctx);
    match case.tab {
        Tab::Results => results_tab(ui, p, &case.dir.clone(), &mut case.results),
        Tab::Conditions => conditions_tab(ui, p, case),
        Tab::Numerics => file_tab(ui, p, case, File::Solution),
        Tab::Schemes => file_tab(ui, p, case, File::Schemes),
        Tab::Fluid => file_tab(ui, p, case, File::Fluid),
        Tab::Turbulence => file_tab(ui, p, case, File::Turbulence),
        Tab::Transport => file_tab(ui, p, case, File::Transport),
        Tab::Settings => settings_tab(ui, p, case),
        Tab::Mesh => {
            let summary = case.summary.as_ref();
            let source = Source {
                dir: case.dir.clone(),
                times: summary.map(|s| s.times.clone()).unwrap_or_default(),
                fields: summary.map(|s| s.fields.clone()).unwrap_or_default(),
            };
            mesh_tab(ui, p, case.viewer.as_mut(), &source);
        }
        Tab::Run => run_view(ui, p, case),
    }
}

/// The colours of the residual lines: eight that stay apart on both themes.
const SERIES: [(u8, u8, u8); 8] = [
    (0, 114, 178),
    (230, 159, 0),
    (0, 158, 115),
    (213, 94, 0),
    (204, 121, 167),
    (86, 180, 233),
    (240, 228, 66),
    (153, 153, 153),
];

fn series_colour(i: usize) -> egui::Color32 {
    let (r, g, b) = SERIES
        .get(i % SERIES.len())
        .copied()
        .unwrap_or((153, 153, 153));
    egui::Color32::from_rgb(r, g, b)
}

/// The warning for a run whose residuals are going the wrong way, and what to try.
fn trouble_message(trouble: &Trouble) -> String {
    match trouble {
        Trouble::NotANumber { field, step } => format!(
            "The residual of {field} is not a number at step {step}: the fields are no longer \
             valid. Stop the run and check the boundary conditions, the initial values and the \
             mesh; lower the relaxation factors in Numerics before running again."
        ),
        Trouble::Growing {
            field,
            step,
            residual,
            lowest,
        } => format!(
            "The residual of {field} has risen to {residual:.3e} at step {step} from a lowest \
             value of {lowest:.1e}: the run seems to diverge. Stop it and check the boundary \
             conditions and the mesh; lower the relaxation factors in Numerics before running \
             again."
        ),
    }
}

/// How the residuals have fallen, one line per equation, on a logarithmic axis.
fn residual_card(ui: &mut egui::Ui, p: &Palette, residuals: &Residuals, case: &Path) {
    Frame::NONE
        .fill(for_egui::color(p.surface_sunken))
        .corner_radius(super::corner(nodalia_theme::component::CONTROL_RADIUS))
        .inner_margin(Margin::same(space::MD as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Residuals")).strong());
                let note_id = egui::Id::unique(("residuals-saved", case));
                if !residuals.is_empty() && ui.small_button(tr("Save CSV")).clicked() {
                    let relative = Path::new("postProcessing")
                        .join("residuals")
                        .join("residuals.csv");
                    let note = match vortia_foamfile::write::atomic(
                        &case.join(&relative),
                        residuals.csv().as_bytes(),
                    ) {
                        Ok(()) => (true, trf("Saved to {}.", &[&relative.display()])),
                        Err(e) => (false, e.to_string()),
                    };
                    ui.data_mut(|d| d.insert_temp(note_id, note));
                }
                if let Some((ok, message)) = ui.data(|d| d.get_temp::<(bool, String)>(note_id)) {
                    let tone = if ok {
                        p.status_success_fg
                    } else {
                        p.status_danger_fg
                    };
                    ui.colored_label(for_egui::color(tone), message);
                }
            });
            if let Some(trouble) = residuals.trouble() {
                ui.colored_label(
                    for_egui::color(p.status_danger_fg),
                    trouble_message(&trouble),
                );
            }
            let Some(([t0, t1], [lo, hi])) = residuals.bounds() else {
                ui.label(
                    RichText::new(tr("They appear as the solver reports each step."))
                        .color(muted(p)),
                );
                return;
            };
            ui.horizontal_wrapped(|ui| {
                for (i, series) in residuals.series.iter().enumerate() {
                    let (swatch, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(swatch, 2.0, series_colour(i));
                    ui.label(RichText::new(&series.field).color(muted(p)));
                }
            });
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 150.0),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(rect);
            let left = rect.left() + 44.0;
            let area = egui::Rect::from_min_max(
                egui::pos2(left, rect.top() + 4.0),
                egui::pos2(rect.right() - 6.0, rect.bottom() - 16.0),
            );
            // Whole decades: the axis runs from the one under the smallest residual to the
            // one over the largest, so a flat run still has a visible range.
            let top = hi.log10().ceil().max(lo.log10().floor() + 1.0);
            let bottom = lo.log10().floor();
            let span = (top - bottom).max(1.0);
            let x_of = |t: f64| {
                let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.5 };
                area.left() + f as f32 * area.width()
            };
            let y_of = |r: f64| {
                let f = ((r.log10() - bottom) / span).clamp(0.0, 1.0);
                area.bottom() - f as f32 * area.height()
            };
            let grid = for_egui::color(p.border_subtle);
            let ink = for_egui::color(p.text_secondary);
            let mut decade = bottom;
            while decade <= top {
                let y = y_of(10f64.powf(decade));
                painter.line_segment(
                    [egui::pos2(area.left(), y), egui::pos2(area.right(), y)],
                    egui::Stroke::new(1.0, grid),
                );
                painter.text(
                    egui::pos2(area.left() - 4.0, y),
                    egui::Align2::RIGHT_CENTER,
                    format!("1e{decade:.0}"),
                    eng::caption(),
                    ink,
                );
                decade += 1.0;
            }
            painter.text(
                egui::pos2(area.left(), rect.bottom()),
                egui::Align2::LEFT_BOTTOM,
                format!("{t0}"),
                eng::caption(),
                ink,
            );
            painter.text(
                egui::pos2(area.right(), rect.bottom()),
                egui::Align2::RIGHT_BOTTOM,
                format!("{t1}"),
                eng::caption(),
                ink,
            );
            for (i, series) in residuals.series.iter().enumerate() {
                let line: Vec<egui::Pos2> = series
                    .points
                    .iter()
                    .map(|&(t, v)| egui::pos2(x_of(t), y_of(v)))
                    .collect();
                painter.add(egui::Shape::line(
                    line,
                    egui::Stroke::new(1.5, series_colour(i)),
                ));
            }
        });
}

/// What entered against what left, from the flux the run wrote: a case that conserves mass
/// closes this to a small fraction of a percent once it has converged.
fn balance_card(ui: &mut egui::Ui, p: &Palette, balance: Option<&BalanceHistory>) {
    Frame::NONE
        .fill(for_egui::color(p.surface_sunken))
        .corner_radius(super::corner(nodalia_theme::component::CONTROL_RADIUS))
        .inner_margin(Margin::same(space::MD as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tr("Mass balance")).strong());
            let Some(b) = balance else {
                ui.label(
                    RichText::new(tr("It appears once a run has written the flux (phi)."))
                        .color(muted(p)),
                );
                return;
            };
            ui.label(
                RichText::new(trf(
                    "Times {} to {} ({} written): mean {} %, worst {} %",
                    &[
                        &b.first,
                        &b.last,
                        &b.samples,
                        &format!("{:.3}", b.mean_abs_percent),
                        &format!("{:.3}", b.max_abs_percent),
                    ],
                ))
                .color(muted(p)),
            );
            ui.add_space(space::SM);
            egui::Grid::new("balance")
                .num_columns(2)
                .spacing([space::XL, space::XS])
                .show(ui, |ui| {
                    for (patch, flux) in &b.latest.patches {
                        ui.label(RichText::new(patch).color(muted(p)));
                        ui.label(RichText::new(format!("{flux:+.4e}")).monospace());
                        ui.end_row();
                    }
                    ui.label(RichText::new(tr("Net")).strong());
                    ui.label(
                        RichText::new(format!(
                            "{:+.4e} ({:+.3} %)",
                            b.latest.net(),
                            b.latest.percent()
                        ))
                        .monospace(),
                    );
                    ui.end_row();
                });
        });
}

fn run_view(ui: &mut egui::Ui, p: &Palette, case: &CaseView) {
    egui::ScrollArea::vertical()
        .id_salt("run-view")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            convergence_card(ui, p, case.report.as_ref());
            ui.add_space(space::MD);
            residual_card(ui, p, &case.residuals, &case.dir);
            ui.add_space(space::MD);
            balance_card(
                ui,
                p,
                case.summary.as_ref().and_then(|s| s.balance.as_ref()),
            );
            ui.add_space(space::MD);
            ui.label(RichText::new(tr("Log")).color(muted(p)));
            Frame::NONE
                .fill(for_egui::color(p.surface_sunken))
                .corner_radius(super::corner(nodalia_theme::component::CONTROL_RADIUS))
                .inner_margin(Margin::same(space::MD as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(260.0);
                    egui::ScrollArea::vertical()
                        .id_salt("run-log")
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if case.log.is_empty() {
                                ui.label(
                                    RichText::new(tr("The solver's output appears here."))
                                        .color(muted(p)),
                                );
                            }
                            for line in &case.log {
                                ui.label(RichText::new(line).font(eng::value_font()));
                            }
                        });
                });
        });
}

// --- Status bar ------------------------------------------------------------------------------

/// What the run is doing, in a word and a tone.
fn run_state(case: &CaseView) -> (String, Tone) {
    use crate::session::Outcome;
    if case.running() {
        let paused = case.session.as_ref().is_some_and(|s| s.is_paused());
        let text = match (&case.report, case.stopping) {
            (_, true) => tr("Stopping").to_owned(),
            _ if paused => tr("Paused").to_owned(),
            (Some(report), false) => trf("Running · {}", &[&status_label(&report.status).0]),
            (None, false) => tr("Running").to_owned(),
        };
        return (text, Tone::Info);
    }
    match &case.outcome {
        Some(Outcome::Done) => (tr("Finished").into(), Tone::Success),
        Some(Outcome::Stopped) => (tr("Stopped").into(), Tone::Warning),
        Some(Outcome::Failed(_)) => (tr("Failed").into(), Tone::Danger),
        None => (tr("Ready").into(), Tone::Info),
    }
}

fn status_bar(ui: &mut egui::Ui, look: &Look, case: &CaseView) {
    let p = &look.palette;
    let (state, tone) = run_state(case);
    let dot: Color32 = tone_colors(p, tone).0;
    eng::status_bar(
        ui,
        |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, dot);
            ui.label(RichText::new(state).color(for_egui::color(p.text_primary)));
            if let Some(app) = &case.application {
                ui.label(RichText::new(format!("·  {app}")).color(muted(p)));
            }
        },
        |ui| {
            let Some(report) = &case.report else {
                return;
            };
            // Drawn from the right edge, so the last value is the first one drawn.
            eng::status_value(
                ui,
                look,
                &format!("change {}", scientific(report.field_change)),
            );
            eng::status_value(
                ui,
                look,
                &format!("equations {}", scientific(report.equation_error)),
            );
            eng::status_value(
                ui,
                look,
                &format!("mass {}", scientific(report.global_mass)),
            );
            if let Some(time) = report.time {
                eng::status_value(ui, look, &format!("t {time}"));
            }
        },
    );
}

impl CaseView {
    /// Opens a view, and forgets a patch picked in the Browser unless the view is the mesh.
    pub(super) fn go(&mut self, tab: Tab) {
        if tab != Tab::Mesh {
            self.selection = Selection::Nothing;
        }
        self.tab = tab;
    }
}

/// The app's icon, as the title bar and Home draw it.
const ICON: &[u8] = include_bytes!("../../../../resources/icons/vortia.png");

/// The side the icon is kept at: it is drawn at 24 and 52 points, so 128 serves both at 2x.
const ICON_SIDE: u32 = 128;

/// The icon the window manager gets. eframe puts its own black egui logo there when a viewport
/// has none, and on macOS that replaces the bundle's icon in the Dock. macOS therefore passes an
/// empty icon, which eframe leaves alone, so the Dock keeps the `Vortia.icns` of the bundle; the
/// other systems get the portrait.
pub(super) fn window_icon() -> egui::IconData {
    if cfg!(target_os = "macos") {
        return egui::IconData::default();
    }
    image::load_from_memory_with_format(ICON, image::ImageFormat::Png)
        .map(|img| {
            let rgba = img.to_rgba8();
            egui::IconData {
                width: rgba.width(),
                height: rgba.height(),
                rgba: rgba.into_raw(),
            }
        })
        // A broken icon is the same as none: the window still opens.
        .unwrap_or_default()
}

fn load_icon(ctx: &Context) -> egui::TextureHandle {
    let image = image::load_from_memory_with_format(ICON, image::ImageFormat::Png)
        .map(|img| {
            let small = image::imageops::resize(
                &img.to_rgba8(),
                ICON_SIDE,
                ICON_SIDE,
                image::imageops::FilterType::Lanczos3,
            );
            egui::ColorImage::from_rgba_unmultiplied(
                [ICON_SIDE as usize, ICON_SIDE as usize],
                small.as_raw(),
            )
        })
        // A broken icon leaves the space empty rather than stopping the window.
        .unwrap_or_else(|_| egui::ColorImage::filled([1, 1], Color32::TRANSPARENT));
    ctx.load_texture("vortia-icon", image, egui::TextureOptions::LINEAR)
}

impl VortiaApp {
    pub(super) fn icon(&mut self, ctx: &Context) -> &egui::TextureHandle {
        self.icon.get_or_insert_with(|| load_icon(ctx))
    }
}

/// An STL file on Home: what it is, then the surface. True when the user closes it.
fn stl_page(ui: &mut egui::Ui, look: &Look, view: &mut crate::stlview::StlView) -> bool {
    let p = &look.palette;
    view.poll();
    let mut close = false;
    ui.horizontal(|ui| {
        let name = view
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        ui.label(RichText::new(name).size(font_size::TITLE).strong());
        if ui.button(tr("Close geometry")).clicked() {
            close = true;
        }
    });
    ui.add_space(space::SM);
    if view.reading() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tr("Reading the geometry…"));
        });
        return close;
    }
    if let Some(message) = view.failure() {
        ui.colored_label(for_egui::color(p.status_danger_fg), message);
        return close;
    }
    if let Some((about, viewer, source)) = view.loaded() {
        let lines = match about {
            crate::stlview::About::Stl(summary) => stl_lines(summary),
            crate::stlview::About::Case(lines) => lines.clone(),
        };
        for line in lines {
            ui.label(line);
        }
        ui.add_space(space::SM);
        viewer.show(
            ui,
            for_egui::color(p.surface_sunken),
            muted(p),
            component::CONTROL_RADIUS.round().clamp(0.0, 255.0) as u8,
            source,
        );
    }
    close
}

/// What the geometry panel says about a surface, as sentences.
pub(super) fn stl_lines(s: &vortia_mesh::stl::Summary) -> Vec<String> {
    let mut lines = vec![
        trf(
            "{} triangles, {} vertices",
            &[&grouped(s.triangles), &grouped(s.vertices)],
        ),
        trf(
            "Size {} × {} × {}",
            &[
                &g4(s.high[0] - s.low[0]),
                &g4(s.high[1] - s.low[1]),
                &g4(s.high[2] - s.low[2]),
            ],
        ),
        trf("Area {}", &[&g4(s.area)]),
    ];
    lines.push(match s.volume {
        Some(v) => trf("Closed solid, volume {}", &[&g4(v)]),
        None => trf(
            "Not closed: {} open edges, {} edges shared by more than two triangles",
            &[&grouped(s.open_edges), &grouped(s.non_manifold_edges)],
        ),
    });
    if s.degenerate > 0 {
        lines.push(trf("{} triangles have no area", &[&grouped(s.degenerate)]));
    }
    lines
}

/// Four significant figures, in the units of the file (an STL has none).
fn g4(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".to_owned();
    }
    let digits = 3 - v.abs().log10().floor() as i32;
    let decimals = digits.clamp(0, 9) as usize;
    if v.abs() >= 1e6 || v.abs() < 1e-3 {
        format!("{v:.3e}")
    } else {
        format!("{v:.decimals$}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convergence::Convergence;
    use crate::prefs::Prefs;

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

    fn app_with(case: &std::path::Path) -> VortiaApp {
        let mut app = VortiaApp::with_prefs(Prefs::default(), None);
        app.open_case(case).unwrap();
        app
    }

    fn frame(app: &mut VortiaApp, ctx: &Context) {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.ui(ui));
        output.textures_delta.clear();
    }

    fn view(app: &mut VortiaApp) -> &mut CaseView {
        match &mut app.screen {
            Screen::Case(case) => case,
            Screen::Welcome => panic!("the test opened a case"),
        }
    }

    #[test]
    fn every_view_belongs_to_one_step_that_opens_on_a_view_of_its_own() {
        for tab in [
            Tab::Run,
            Tab::Mesh,
            Tab::Results,
            Tab::Conditions,
            Tab::Numerics,
            Tab::Schemes,
            Tab::Settings,
            Tab::Fluid,
            Tab::Turbulence,
            Tab::Transport,
        ] {
            assert_eq!(Step::of(Step::of(tab).first()), Step::of(tab));
        }
        for step in Step::ALL {
            assert_eq!(Step::of(step.first()), step);
        }
    }

    #[test]
    fn the_tree_lists_the_mesh_the_fields_and_the_solver_files_and_folds() {
        let dir = case_dir();
        let mut app = app_with(dir.path());
        let case = view(&mut app);
        let labels = |case: &CaseView| tree(case).into_iter().map(|r| r.label).collect::<Vec<_>>();
        assert_eq!(
            labels(case),
            [
                case.name().as_str(),
                "Mesh",
                "Boundary conditions",
                "Solver",
                "Run settings",
                "Numerics",
                "Schemes",
                "Run",
                "Results"
            ]
        );
        case.collapsed.insert("solver");
        assert!(!labels(case).contains(&"Schemes".to_owned()));
        case.collapsed.insert("case");
        assert_eq!(labels(case).len(), 1);
    }

    #[test]
    fn a_view_is_marked_in_the_tree_and_a_picked_patch_is_forgotten_when_leaving_the_mesh() {
        let dir = case_dir();
        let mut app = app_with(dir.path());
        let case = view(&mut app);
        case.go(Tab::Numerics);
        let selected: Vec<_> = tree(case)
            .into_iter()
            .filter(|r| r.selected)
            .map(|r| r.label)
            .collect();
        assert_eq!(selected, ["Numerics"]);
        case.go(Tab::Mesh);
        case.selection = Selection::Patch("inlet".into());
        case.go(Tab::Mesh);
        assert_eq!(case.selection, Selection::Patch("inlet".into()));
        case.go(Tab::Schemes);
        assert_eq!(case.selection, Selection::Nothing);
    }

    #[test]
    fn commands_move_between_home_the_views_and_the_dialogs() {
        let dir = case_dir();
        let ctx = Context::default();
        let mut app = app_with(dir.path());
        assert!(!app.home);
        app.perform(&ctx, Cmd::Open);
        assert!(app.home && app.focus_open);
        app.perform(&ctx, Cmd::Go(Tab::Schemes));
        assert!(!app.home);
        assert_eq!(view(&mut app).tab, Tab::Schemes);
        app.perform(&ctx, Cmd::Show(Dialog::About));
        assert_eq!(app.dialog, Some(Dialog::About));
        app.perform(&ctx, Cmd::CloseCase);
        assert!(matches!(app.screen, Screen::Welcome));
        // With no case open, a view cannot be gone to.
        app.perform(&ctx, Cmd::Go(Tab::Mesh));
        assert!(matches!(app.screen, Screen::Welcome));
    }

    #[test]
    fn updates_are_offered_by_the_menus_and_never_checked_by_a_test_run() {
        assert!(COMMANDS.iter().any(|(_, c)| *c == Cmd::CheckUpdates));
        let app = VortiaApp::with_prefs(Prefs::default(), None);
        // A new app has not asked GitHub anything, and no update window holds the keys.
        assert!(!app.check_updates_at_start);
        assert!(!app.updater.is_open());
        assert_eq!(app.updater.app().name, "Vortia");
    }

    #[test]
    fn the_command_search_offers_what_the_menus_do() {
        assert!(COMMANDS.iter().any(|(_, c)| *c == Cmd::Run));
        assert!(COMMANDS.iter().any(|(_, c)| *c == Cmd::Go(Tab::Conditions)));
        let labels: Vec<_> = COMMANDS.iter().map(|(l, _)| *l).collect();
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(labels.len(), unique.len());
    }

    #[test]
    fn the_status_bar_tells_the_state_of_the_run() {
        let dir = case_dir();
        let mut app = app_with(dir.path());
        let case = view(&mut app);
        assert_eq!(run_state(case), ("Ready".to_owned(), Tone::Info));
        case.outcome = Some(crate::session::Outcome::Failed("boom".into()));
        assert_eq!(run_state(case).1, Tone::Danger);
        case.outcome = Some(crate::session::Outcome::Stopped);
        assert_eq!(run_state(case).1, Tone::Warning);
        case.outcome = Some(crate::session::Outcome::Done);
        assert_eq!(run_state(case), ("Finished".to_owned(), Tone::Success));
    }

    #[test]
    fn home_the_case_and_the_search_draw_in_both_themes_with_everything_open() {
        let dir = case_dir();
        for theme in [crate::prefs::Theme::Light, crate::prefs::Theme::Dark] {
            let ctx = Context::default();
            let mut app = VortiaApp::with_prefs(
                Prefs {
                    theme,
                    recent: vec![dir.path().to_path_buf()],
                    ..Prefs::default()
                },
                None,
            );
            frame(&mut app, &ctx);
            app.open_case(dir.path()).unwrap();
            frame(&mut app, &ctx);
            view(&mut app).report = Convergence::parse(
                r#"{"status":"stabilizing","automatic":true,"stableSamples":1,"requiredSamples":5,"globalMass":1e-4,"iteration":12}"#,
            );
            for tab in [
                Tab::Run,
                Tab::Mesh,
                Tab::Results,
                Tab::Conditions,
                Tab::Numerics,
                Tab::Schemes,
                Tab::Settings,
                Tab::Fluid,
                Tab::Turbulence,
                Tab::Transport,
            ] {
                view(&mut app).go(tab);
                frame(&mut app, &ctx);
            }
            view(&mut app).selection = Selection::Patch("nowhere".into());
            frame(&mut app, &ctx);
            app.home = true;
            frame(&mut app, &ctx);
            app.perform(&ctx, Cmd::New);
            assert!(app.new_case.is_some());
            frame(&mut app, &ctx);
            app.new_case = None;
            app.search = Some("run".into());
            frame(&mut app, &ctx);
            assert!(app.search.is_some());
        }
    }
}
