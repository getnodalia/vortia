//! The Preferences window: a column of sections on the left and the page of the chosen one on
//! the right, as in Torno and Forja. Every change applies at once and is kept in the
//! preferences file; nothing here needs a restart.

use eframe::egui::{
    self, Align, Color32, ComboBox, Context, Layout, Pos2, ScrollArea, Sense, Stroke, StrokeKind,
    vec2,
};
use nodalia_theme::office::{self, Look};
use nodalia_theme::{font_size, space};

use super::{SHORTCUTS, VortiaApp, control, palette, theme_label};
use crate::i18n::{Choice, tr, trf};
use crate::prefs::{
    ColourScale, DIGITS_MAX, DIGITS_MIN, Prefs, SCALE_MAX, SCALE_MIN, SCALE_STEP, Theme,
};

/// The sections, in the order of the suite's Preferences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Section {
    #[default]
    General,
    Appearance,
    Units,
    View,
    Shortcuts,
    Performance,
    Updates,
}

impl Section {
    const ALL: [Section; 7] = [
        Section::General,
        Section::Appearance,
        Section::Units,
        Section::View,
        Section::Shortcuts,
        Section::Performance,
        Section::Updates,
    ];

    fn key(self) -> &'static str {
        match self {
            Section::General => "general",
            Section::Appearance => "appearance",
            Section::Units => "units",
            Section::View => "view",
            Section::Shortcuts => "shortcuts",
            Section::Performance => "performance",
            Section::Updates => "updates",
        }
    }

    pub(super) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    fn label(self) -> &'static str {
        match self {
            Section::General => tr("General"),
            Section::Appearance => tr("Appearance"),
            Section::Units => tr("Units and precision"),
            Section::View => tr("View"),
            Section::Shortcuts => tr("Keyboard shortcuts"),
            Section::Performance => tr("Performance"),
            Section::Updates => tr("Updates"),
        }
    }

    /// An NDL icon name; one the kit does not have draws no icon.
    fn icon(self) -> &'static str {
        match self {
            Section::General => "settings",
            Section::Appearance => "appearance",
            Section::Units => "measure",
            Section::View => "monitor",
            Section::Shortcuts => "keyboard",
            Section::Performance => "flash",
            Section::Updates => "cloud",
        }
    }
}

fn colours_label(colours: ColourScale) -> &'static str {
    match colours {
        ColourScale::Rainbow => tr("Rainbow"),
        ColourScale::Viridis => tr("Viridis"),
        ColourScale::Greys => tr("Greys"),
    }
}

/// A strip of the colour scale, for the choice to be seen and not only named.
fn scale_strip(ui: &mut egui::Ui, colours: ColourScale) {
    let (rect, _) = ui.allocate_exact_size(vec2(office::CONTROL_WIDTH, 14.0), Sense::hover());
    let steps = 48;
    let width = rect.width() / steps as f32;
    for n in 0..steps {
        let [r, g, b] = crate::fieldview::scale_of(colours, n as f32 / (steps - 1) as f32);
        let left = rect.left() + n as f32 * width;
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                Pos2::new(left, rect.top()),
                Pos2::new(left + width + 0.5, rect.bottom()),
            ),
            0.0,
            Color32::from_rgb(r, g, b),
        );
    }
    ui.painter().rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        StrokeKind::Inside,
    );
}

impl VortiaApp {
    /// The window. Returns whether it should close.
    pub(super) fn preferences_window(&mut self, ctx: &Context) -> bool {
        let look = Look::new(palette(ctx));
        let p = look.palette;
        let screen = ctx.content_rect().size();
        let width = (screen.x - 80.0).clamp(520.0, 880.0);
        let height = (screen.y - 120.0).clamp(380.0, 580.0);
        let before = self.prefs.clone();
        let mut close = false;
        egui::Modal::new(egui::Id::unique("preferences")).show(ctx, |ui| {
            ui.set_width(width);
            ui.set_height(height);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(tr("Preferences"))
                        .size(font_size::TITLE)
                        .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = control(ui, "close", p.status_danger_fg).clicked();
                });
            });
            ui.add_space(space::MD);
            let body = vec2(width, ui.available_height().max(120.0));
            let page_width = (body.x - office::NAV_WIDTH - space::LG).max(240.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(office::NAV_WIDTH, body.y),
                    Layout::top_down(Align::Min),
                    |ui| {
                        for section in Section::ALL {
                            let selected = section == self.pref_section;
                            if office::nav_row(ui, &look, section.icon(), section.label(), selected)
                                .clicked()
                            {
                                self.pref_section = section;
                            }
                        }
                    },
                );
                ui.add_space(space::LG);
                ui.allocate_ui_with_layout(
                    vec2(page_width, body.y),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ScrollArea::vertical()
                            .id_salt("preferences_page")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_width(page_width - space::LG);
                                let section = self.pref_section;
                                office::page(ui, &look, section.label(), |ui| {
                                    self.preferences_page(ui, ctx, &look, section);
                                });
                            });
                    },
                );
            });
        });
        if self.prefs != before {
            self.prefs_changed(ctx, &before);
        }
        close
    }

    /// Applies what changed in the preferences and keeps them.
    pub(super) fn prefs_changed(&mut self, ctx: &Context, before: &Prefs) {
        if self.prefs.theme != before.theme {
            self.apply_theme(ctx);
        }
        if self.prefs.lang != before.lang {
            crate::i18n::apply(self.prefs.lang);
        }
        self.apply_display(ctx);
        self.save_prefs();
    }

    /// Hands the display and run choices to the parts that draw and solve.
    pub(super) fn apply_display(&self, ctx: &Context) {
        ctx.set_zoom_factor(f32::from(self.prefs.ui_scale) / 100.0);
        crate::fieldview::configure(self.prefs.colours, self.prefs.digits);
        crate::viewer::set_default_cell_edges(self.prefs.cell_edges);
        vortia_fv::set_thread_limit(usize::from(self.prefs.threads));
    }

    fn preferences_page(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        look: &Look,
        section: Section,
    ) {
        match section {
            Section::General => self.general_page(ui, look),
            Section::Appearance => self.appearance_page(ui, look),
            Section::Units => self.units_page(ui, look),
            Section::View => self.view_page(ui, look),
            Section::Shortcuts => shortcuts_page(ui),
            Section::Performance => self.performance_page(ui, look),
            Section::Updates => self.updates_page(ui, ctx, look),
        }
    }

    fn general_page(&mut self, ui: &mut egui::Ui, look: &Look) {
        office::section(ui, look, tr("Language"), |ui| {
            office::settings(ui, "language", |ui| {
                office::setting(ui, look, tr("Interface language"), |ui| {
                    ComboBox::from_id_salt("language")
                        .selected_text(self.prefs.lang.label())
                        .show_ui(ui, |ui| {
                            for lang in Choice::ALL {
                                ui.selectable_value(&mut self.prefs.lang, lang, lang.label());
                            }
                        });
                });
            });
            office::hint(
                ui,
                look,
                tr("The menu bar the desktop owns changes language at the next start."),
            );
        });
        office::section(ui, look, tr("Cases"), |ui| {
            ui.checkbox(
                &mut self.prefs.reopen_last,
                tr("Open the last case when Vortia starts"),
            );
            ui.add_space(space::SM);
            let recent = self.prefs.recent.len();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(recent > 0, egui::Button::new(tr("Clear the recent cases")))
                    .clicked()
                {
                    self.prefs.recent.clear();
                }
                office::hint(ui, look, &trf("{} on the list", &[&recent]));
            });
        });
    }

    fn appearance_page(&mut self, ui: &mut egui::Ui, look: &Look) {
        office::section(ui, look, tr("Theme"), |ui| {
            office::settings(ui, "theme", |ui| {
                office::setting(ui, look, tr("Colours"), |ui| {
                    ComboBox::from_id_salt("theme")
                        .selected_text(theme_label(self.prefs.theme))
                        .show_ui(ui, |ui| {
                            for theme in Theme::ALL {
                                ui.selectable_value(
                                    &mut self.prefs.theme,
                                    theme,
                                    theme_label(theme),
                                );
                            }
                        });
                });
            });
        });
        office::section(ui, look, tr("Size"), |ui| {
            office::settings(ui, "size", |ui| {
                office::setting(ui, look, tr("Interface size"), |ui| {
                    ComboBox::from_id_salt("ui_scale")
                        .selected_text(format!("{} %", self.prefs.ui_scale))
                        .show_ui(ui, |ui| {
                            for percent in (SCALE_MIN..=SCALE_MAX).step_by(usize::from(SCALE_STEP))
                            {
                                ui.selectable_value(
                                    &mut self.prefs.ui_scale,
                                    percent,
                                    format!("{percent} %"),
                                );
                            }
                        });
                });
            });
            office::hint(
                ui,
                look,
                tr("Makes text and controls larger or smaller, for a screen that is far or dense."),
            );
        });
    }

    fn units_page(&mut self, ui: &mut egui::Ui, look: &Look) {
        office::section(ui, look, tr("Numbers"), |ui| {
            office::settings(ui, "digits", |ui| {
                office::setting(ui, look, tr("Significant digits"), |ui| {
                    ComboBox::from_id_salt("digits")
                        .selected_text(self.prefs.digits.to_string())
                        .show_ui(ui, |ui| {
                            for digits in DIGITS_MIN..=DIGITS_MAX {
                                ui.selectable_value(
                                    &mut self.prefs.digits,
                                    digits,
                                    digits.to_string(),
                                );
                            }
                        });
                });
            });
            let sample = |v: f32| crate::fieldview::label_with(self.prefs.digits, v);
            office::hint(
                ui,
                look,
                &trf(
                    "On the labels of the colour scale: {}, {}, {}.",
                    &[&sample(300.567_9), &sample(101_325.0), &sample(0.000_012_5)],
                ),
            );
        });
        office::section(ui, look, tr("Units"), |ui| {
            office::hint(
                ui,
                look,
                tr(
                    "A case is kept in SI units (metres, kilograms, seconds, kelvin, pascals) and every value is shown in them.",
                ),
            );
        });
    }

    fn view_page(&mut self, ui: &mut egui::Ui, look: &Look) {
        office::section(ui, look, tr("Fields"), |ui| {
            office::settings(ui, "colours", |ui| {
                office::setting(ui, look, tr("Colour scale"), |ui| {
                    ComboBox::from_id_salt("colours")
                        .selected_text(colours_label(self.prefs.colours))
                        .show_ui(ui, |ui| {
                            for colours in ColourScale::ALL {
                                ui.selectable_value(
                                    &mut self.prefs.colours,
                                    colours,
                                    colours_label(colours),
                                );
                            }
                        });
                });
                office::setting(ui, look, tr("Preview"), |ui| {
                    scale_strip(ui, self.prefs.colours);
                });
            });
            office::hint(
                ui,
                look,
                tr(
                    "Viridis and Greys stay readable in black and white and for colour-vision deficiencies.",
                ),
            );
        });
        office::section(ui, look, tr("Mesh"), |ui| {
            ui.checkbox(
                &mut self.prefs.cell_edges,
                tr("Draw the cell edges when a mesh opens"),
            );
        });
    }

    fn performance_page(&mut self, ui: &mut egui::Ui, look: &Look) {
        let available = vortia_fv::available_threads();
        office::section(ui, look, tr("Run"), |ui| {
            office::settings(ui, "threads", |ui| {
                office::setting(ui, look, tr("Threads for a run"), |ui| {
                    let text = match self.prefs.threads {
                        0 => trf("All ({})", &[&available]),
                        n => n.to_string(),
                    };
                    ComboBox::from_id_salt("threads")
                        .selected_text(text)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.prefs.threads,
                                0,
                                trf("All ({})", &[&available]),
                            );
                            for n in (1..=available.min(64)).map(|n| n as u8) {
                                ui.selectable_value(&mut self.prefs.threads, n, n.to_string());
                            }
                        });
                });
            });
            office::hint(
                ui,
                look,
                tr(
                    "The answer of a run is the same with any number of threads; only the time changes. It applies to the runs that start from now on.",
                ),
            );
        });
    }

    fn updates_page(&mut self, ui: &mut egui::Ui, ctx: &Context, look: &Look) {
        office::section(ui, look, tr("Updates"), |ui| {
            ui.checkbox(
                &mut self.prefs.check_updates,
                tr("Look for a new version when Vortia starts"),
            );
            ui.add_space(space::SM);
            ui.horizontal(|ui| {
                if ui.button(tr("Check for updates…")).clicked() {
                    self.updater.check_now(ctx);
                }
                office::hint(ui, look, &trf("Version {}", &[&env!("CARGO_PKG_VERSION")]));
            });
        });
    }
}

fn shortcuts_page(ui: &mut egui::Ui) {
    egui::Grid::new("preferences_shortcuts")
        .num_columns(2)
        .spacing([space::XL, space::SM])
        .show(ui, |ui| {
            for (keys, what) in SHORTCUTS {
                ui.label(egui::RichText::new(*keys).monospace());
                ui.label(tr(what));
                ui.end_row();
            }
        });
}
