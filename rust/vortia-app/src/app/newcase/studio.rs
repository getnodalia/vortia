//! The studio of New: the CAD in the engineering suite's shell, the one Torno and Forja wear.
//!
//! A drawn region and a modelled part are not filled in on a page. They get the whole window: a
//! ribbon of tools with their icons on top, the Browser of what is drawn on the left, the
//! canvas in the middle with the view bar floating over it, Properties on the right and the
//! status bar with the hint of the tool, the freedom of the sketch and the pointer's place.

use eframe::egui::{self, Align2, DragValue, Frame, Margin, RichText, Ui};
use nodalia_theme::engineering::{self as eng, Look, TreeRow, ViewItem, layout};
use nodalia_theme::{Palette, for_egui, space};
use vortia_sketch::Measure;

use super::{Ask, Kind, NewCase, grid, region_canvas_in, row, value};
use crate::i18n::{tr, trf};
use crate::region_editor::{Outline, RegionEditor, Relation, Role, Tool};

/// The width of the Browser and of Properties of a drawn region.
///
/// The canvas is what the studio is for, so the side panels take what their rows need and no
/// more: the drawing gets the rest of the window, and each panel can be put away.
const BROWSER: f32 = 208.0;
const PROPERTIES: f32 = 252.0;

/// The width the part's Browser (its features, each with its numbers) and Properties (the flow)
/// need.
const PART_BROWSER: f32 = 264.0;
const PART_PROPERTIES: f32 = 252.0;

/// The icon of the shared set a tool is drawn with in the ribbon.
fn icon_of(tool: Tool) -> &'static str {
    match tool {
        Tool::Move => "move",
        Tool::Line => "line",
        Tool::Arc => "arc3",
        Tool::Rectangle => "rect",
        Tool::Circle => "circle",
        Tool::Slot => "slot",
        Tool::Polygon => "polygon",
        Tool::Fillet => "sketch-fillet",
        Tool::Chamfer => "sketch-chamfer",
        Tool::Dimension => "dimension",
        Tool::Constrain => "auto-constrain",
        Tool::Delete => "delete",
    }
}

impl NewCase {
    /// Draws the studio over the whole window under the title bar. `notice` is what the last
    /// attempt to make the case said was wrong.
    pub(in crate::app) fn studio(
        &mut self,
        ui: &mut Ui,
        look: &Look,
        p: &Palette,
        notice: Option<&str>,
    ) -> Ask {
        let g = layout::GAP as i8;
        let island = || eng::island(look);
        let part = self.kind == Kind::Modelled;
        let mut ask = Ask::Nothing;
        if part {
            self.modelled.begin(ui.ctx());
        }

        egui::Panel::bottom("new_studio_status")
            .exact_size(layout::STATUS_BAR)
            .show_separator_line(false)
            .frame(Frame::NONE)
            .show(ui, |ui| self.status(ui, look, notice));

        egui::Panel::top("new_studio_ribbon")
            .exact_size(layout::RIBBON + 12.0 + layout::GAP)
            .show_separator_line(false)
            .frame(island().outer_margin(Margin {
                left: g,
                right: g,
                top: 0,
                bottom: g,
            }))
            .show(ui, |ui| ask = self.ribbon(ui, look, p));

        if !self.browser_shut {
            egui::Panel::left("new_studio_browser")
                .exact_size(if part { PART_BROWSER } else { BROWSER } + 16.0 + layout::GAP)
                .resizable(false)
                .show_separator_line(false)
                .frame(island().outer_margin(Margin {
                    left: g,
                    right: 0,
                    top: 0,
                    bottom: g,
                }))
                .show(ui, |ui| {
                    if part {
                        self.modelled.studio_browser(ui, look);
                    } else {
                        self.region_browser(ui, look);
                    }
                });
        }

        if !self.properties_shut {
            egui::Panel::right("new_studio_properties")
                .exact_size(if part { PART_PROPERTIES } else { PROPERTIES } + 16.0 + layout::GAP)
                .resizable(false)
                .show_separator_line(false)
                .frame(island().outer_margin(Margin {
                    left: g,
                    right: g,
                    top: 0,
                    bottom: g,
                }))
                .show(ui, |ui| {
                    // The folder and the button at the foot, the rest in the room that is left.
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        self.save_row(ui, look, p, &mut ask);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            if part {
                                self.modelled.studio_properties(ui, look);
                            } else {
                                self.region_properties(ui, look);
                            }
                        });
                    });
                });
        }

        egui::CentralPanel::default()
            .frame(Frame::NONE.outer_margin(Margin {
                left: g,
                right: if self.properties_shut { g } else { 0 },
                top: 0,
                bottom: g,
            }))
            .show(ui, |ui| {
                if part {
                    self.modelled.studio_canvas(ui, p);
                } else {
                    self.region_canvas(ui, look);
                }
            });
        ask
    }

    // --- Ribbon ------------------------------------------------------------------------------

    fn ribbon(&mut self, ui: &mut Ui, look: &Look, p: &Palette) -> Ask {
        let mut ask = Ask::Nothing;
        let part = self.kind == Kind::Modelled;
        ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
        ui.horizontal(|ui| {
            eng::ribbon_tab(ui, look, if part { tr("Part") } else { tr("Sketch") }, true);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if super::super::accent_button(ui, p, tr("Create case")).clicked() {
                    ask = Ask::Create;
                }
                ui.add_space(space::SM);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("‹  {}", tr("All cases")))
                                .color(for_egui::color(look.palette.accent_text)),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    self.leave();
                }
            });
        });
        egui::ScrollArea::horizontal()
            .id_salt("new_studio_tools")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if part {
                        self.modelled.studio_ribbon(ui, look);
                    } else {
                        region_tools(ui, look, &mut self.region);
                    }
                });
            });
        ask
    }

    // --- Browser -----------------------------------------------------------------------------

    /// The sketch as a tree, and the names of its edges, which say what each one does.
    fn region_browser(&mut self, ui: &mut Ui, look: &Look) {
        let palette = &look.palette;
        eng::panel_header(ui, look, tr("Browser"), |_| {});
        let edges = self.region.drawing().len();
        let points = self.region.points().len();
        let held = self.region.markers().len();
        let sketch = tr("Sketch");
        let edges_label = trf("{} edges", &[&edges]);
        let points_label = trf("{} points", &[&points]);
        let held_label = trf("{} constraints", &[&held]);
        let rows = [
            (0, Some(true), "sketch", sketch),
            (1, None, "line", edges_label.as_str()),
            (1, None, "point", points_label.as_str()),
            (1, None, "constraint", held_label.as_str()),
        ];
        for (depth, expanded, icon, label) in rows {
            let _ = eng::tree_row(
                ui,
                look,
                &TreeRow {
                    depth,
                    expanded,
                    icon,
                    label,
                    selected: false,
                    muted: depth > 0,
                },
            );
        }
        ui.add_space(space::MD);
        eng::panel_header(ui, look, tr("Edge names"), |_| {});
        ui.label(
            RichText::new(tr("inlet, outlet or symmetry; any other name is a wall."))
                .font(eng::caption())
                .color(for_egui::color(palette.text_secondary)),
        );
        ui.add_space(space::SM);
        let mut rename = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (entity, mut name) in self.region.names() {
                    ui.horizontal(|ui| {
                        let (dot, _) =
                            ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        let colour = match Role::of(&name) {
                            Role::Inlet => for_egui::color(palette.accent_default),
                            Role::Outlet => ui.visuals().warn_fg_color,
                            Role::Symmetry => for_egui::color(palette.text_secondary),
                            Role::Wall => for_egui::color(palette.text_primary),
                        };
                        ui.painter().circle_filled(dot.center(), 4.0, colour);
                        let hint = trf("Edge {}", &[&entity]);
                        let answer = ui.add(
                            egui::TextEdit::singleline(&mut name)
                                .id_salt(("studio_edge", entity))
                                .hint_text(hint)
                                .desired_width(ui.available_width()),
                        );
                        if answer.changed() {
                            rename = Some((entity, name.clone()));
                        }
                    });
                }
            });
        // A refused name keeps the sketch as it was and says why in the status bar.
        if let Some((entity, name)) = rename {
            let _ = self.region.rename(entity, &name);
        }
    }

    // --- Properties --------------------------------------------------------------------------

    fn region_properties(&mut self, ui: &mut Ui, look: &Look) {
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                eng::panel_header(ui, look, self.region.tool.title(), |_| {});
                self.tool_options(ui);
                ui.add_space(space::MD);

                eng::panel_header(ui, look, tr("Dimensions"), |_| {});
                self.dimensions(ui, look);
                ui.add_space(space::MD);

                eng::panel_header(ui, look, tr("Sketch"), |_| {});
                self.sketch_file(ui);
                ui.add_space(space::MD);

                eng::panel_header(ui, look, tr("The flow"), |_| {});
                grid(ui, "studio_flow", |ui| {
                    row(ui, tr("Size of the triangles"), |ui| {
                        value(ui, &mut self.drawn.cell_size, "mm")
                    });
                    row(ui, tr("Inlet speed"), |ui| {
                        value(ui, &mut self.drawn.speed, "m/s")
                    });
                    row(ui, tr("Viscosity"), |ui| {
                        value(ui, &mut self.drawn.viscosity, "m²/s")
                    });
                });
            });
    }

    /// What the tool in use needs beside the clicks.
    fn tool_options(&mut self, ui: &mut Ui) {
        let editor = &mut self.region;
        grid(ui, "studio_tool", |ui| {
            match editor.tool {
                Tool::Polygon => row(ui, tr("Sides"), |ui| {
                    ui.add(DragValue::new(&mut editor.sides).range(3..=vortia_sketch::MAX_SIDES));
                }),
                Tool::Fillet => row(ui, tr("Radius"), |ui| {
                    ui.add(
                        DragValue::new(&mut editor.radius)
                            .speed(0.5)
                            .range(0.1..=10000.0)
                            .suffix(" mm"),
                    );
                }),
                Tool::Chamfer => row(ui, tr("Distance"), |ui| {
                    ui.add(
                        DragValue::new(&mut editor.distance)
                            .speed(0.5)
                            .range(0.1..=10000.0)
                            .suffix(" mm"),
                    );
                }),
                Tool::Dimension => row(ui, tr("Measure"), |ui| {
                    for (measure, label) in [
                        (Measure::Distance, tr("Distance")),
                        (Measure::Along, tr("Along x")),
                        (Measure::Height, tr("Height")),
                    ] {
                        ui.selectable_value(&mut editor.measure, measure, label);
                    }
                }),
                _ => {}
            }
            row(ui, tr("Grid"), |ui| {
                ui.add(
                    DragValue::new(&mut editor.grid)
                        .speed(0.5)
                        .range(0.0..=1000.0)
                        .suffix(" mm"),
                );
            });
        });
        ui.label(RichText::new(editor.tool.hint(editor.progress(), editor.relation)).weak());
    }

    /// The dimensions that hold the sketch, each editable and removable.
    fn dimensions(&mut self, ui: &mut Ui, look: &Look) {
        let mut edit = None;
        let mut remove = None;
        let dimensions = self.region.dimensions();
        if dimensions.is_empty() {
            ui.label(
                RichText::new(tr(
                    "None yet: pick the Dimension tool and click two points.",
                ))
                .weak(),
            );
        }
        // One under the other, the name on top: they read "Length, point 0 to 1", which is wider
        // than a column of a grid in Properties, and a panel's contents never overflow it.
        for mut d in dimensions {
            let speed = 0.01 * d.value.abs().max(1.0);
            ui.label(RichText::new(&d.label).weak());
            ui.horizontal(|ui| {
                if ui
                    .add(
                        DragValue::new(&mut d.value)
                            .speed(speed)
                            .suffix(format!(" {}", d.unit))
                            .max_decimals(4),
                    )
                    .changed()
                {
                    edit = Some((d.index, d.value));
                }
                if eng::status_toggle(ui, look, "close", tr("Remove"), false)
                    .on_hover_text(tr("Remove"))
                    .clicked()
                {
                    remove = Some(d.index);
                }
            });
            ui.add_space(space::XS);
        }
        // A refused edit keeps the sketch as it was and says why in the status bar.
        if let Some((index, v)) = edit {
            let _ = self.region.set_dimension(index, v);
        }
        if let Some(index) = remove {
            let _ = self.region.remove_dimension(index);
        }
    }

    /// Where the drawing starts from, and the saved sketch to open.
    fn sketch_file(&mut self, ui: &mut Ui) {
        grid(ui, "studio_start", |ui| {
            row(ui, tr("Start from"), |ui| {
                egui::ComboBox::from_id_salt("studio_outline")
                    .selected_text(self.region.outline.title())
                    .show_ui(ui, |ui| {
                        for outline in Outline::ALL {
                            if ui
                                .selectable_label(self.region.outline == outline, outline.title())
                                .clicked()
                                && self.region.outline != outline
                            {
                                self.region.restart(outline);
                            }
                        }
                    });
            });
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.region_file)
                    .hint_text(tr("A saved sketch"))
                    .desired_width((ui.available_width() - 116.0).max(80.0)),
            );
            if ui.button(tr("Open sketch")).clicked() {
                // A refusal stays in the editor's `problem`, which the status bar shows.
                let _ = self
                    .region
                    .open(std::path::Path::new(self.region_file.trim()));
            }
        });
    }

    /// The folder the case goes in, at the foot of Properties, with the button that makes it.
    fn save_row(&mut self, ui: &mut Ui, look: &Look, p: &Palette, ask: &mut Ask) {
        if super::super::accent_button(ui, p, tr("Create case")).clicked() {
            *ask = Ask::Create;
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.folder)
                .hint_text(tr("Folder for the new case"))
                .desired_width(ui.available_width()),
        );
        ui.label(
            RichText::new(tr("Where it is saved"))
                .font(eng::caption())
                .color(for_egui::color(look.palette.text_secondary)),
        );
    }

    // --- Canvas ------------------------------------------------------------------------------

    /// The sketch on all the room there is, with the view bar floating at the foot of it.
    fn region_canvas(&mut self, ui: &mut Ui, look: &Look) {
        let size = ui.available_size();
        let rect = egui::Rect::from_min_size(ui.cursor().min, size);
        self.cursor = region_canvas_in(ui, &mut self.region, size);
        let editor = &mut self.region;
        let items = [
            ViewItem::Tool {
                icon: "fit",
                name: tr("Fit view"),
                active: false,
            },
            ViewItem::Tool {
                icon: "grid",
                name: tr("Grid"),
                active: editor.grid > 0.0,
            },
            ViewItem::Separator,
            ViewItem::Tool {
                icon: "undo",
                name: tr("Undo"),
                active: false,
            },
            ViewItem::Tool {
                icon: "redo",
                name: tr("Redo"),
                active: false,
            },
        ];
        let clicked = egui::Area::new(egui::Id::NULL.with("new_studio_view_bar"))
            .order(egui::Order::Foreground)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(egui::pos2(rect.center().x, rect.bottom() - 14.0))
            .show(ui.ctx(), |ui| eng::view_bar(ui, look, &items))
            .inner;
        match clicked {
            Some(0) => editor.fit(),
            Some(1) => editor.grid = if editor.grid > 0.0 { 0.0 } else { 5.0 },
            Some(3) => {
                editor.undo();
            }
            Some(4) => {
                editor.redo();
            }
            _ => {}
        }
    }

    // --- Status ------------------------------------------------------------------------------

    fn status(&mut self, ui: &mut Ui, look: &Look, notice: Option<&str>) {
        let p = &look.palette;
        let part = self.kind == Kind::Modelled;
        let danger = for_egui::color(p.status_danger_fg);
        let cursor = self.cursor;
        let features = self.modelled.feature_count();
        let (browser_shut, properties_shut) = (&mut self.browser_shut, &mut self.properties_shut);
        let editor = &mut self.region;
        let warn = ui.visuals().warn_fg_color;
        let secondary = for_egui::color(p.text_secondary);
        // What the bar says on the left: a refusal first, then what is missing, then the hint.
        let (text, colour) = if let Some(notice) = notice {
            (notice.to_owned(), danger)
        } else if part {
            (trf("{} features", &[&features]), secondary)
        } else if let Some(problem) = &editor.problem {
            (problem.clone(), danger)
        } else if let Some(why) = editor.open_because() {
            (why, warn)
        } else if let Some(missing) = editor.missing() {
            (missing.to_owned(), warn)
        } else {
            (
                editor
                    .tool
                    .hint(editor.progress(), editor.relation)
                    .to_owned(),
                secondary,
            )
        };
        eng::status_bar(
            ui,
            |ui| {
                ui.label(RichText::new(text).color(colour));
            },
            |ui| {
                // Drawn from the right edge: the first thing is the last on the bar, so the
                // panels' switches stand where the panels do.
                for (switch, icon, name) in [
                    (&mut *properties_shut, "panel-right", tr("Properties")),
                    (&mut *browser_shut, "sidebar", tr("Browser")),
                ] {
                    if eng::status_toggle(ui, look, icon, name, !*switch)
                        .on_hover_text(name)
                        .clicked()
                    {
                        *switch = !*switch;
                    }
                }
                ui.add_space(space::SM);
                if part {
                    return;
                }
                if let Some((x, y)) = cursor {
                    eng::status_value(ui, look, &format!("x {x:.2}   y {y:.2} mm"));
                }
                if eng::status_toggle(ui, look, "grid", tr("Grid"), editor.grid > 0.0).clicked() {
                    editor.grid = if editor.grid > 0.0 { 0.0 } else { 5.0 };
                }
                if let Some(free) = editor.freedom() {
                    let text = if free == 0 {
                        tr("Fully defined").to_owned()
                    } else {
                        trf("{} degrees of freedom", &[&free])
                    };
                    ui.label(RichText::new(text).color(for_egui::color(p.text_secondary)));
                }
            },
        );
    }
}

/// The tools of a sketch in the groups a CAD has: draw, modify and hold, with the relations
/// while the Constrain tool is the one in use, and the history and the view.
fn region_tools(ui: &mut Ui, look: &Look, editor: &mut RegionEditor) {
    for (title, tools) in [
        (tr("Draw"), &Tool::DRAW[..]),
        (tr("Modify"), &Tool::MODIFY[..]),
        (tr("Hold"), &Tool::HOLD[..]),
    ] {
        eng::ribbon_group(ui, look, title, |ui| {
            for &tool in tools {
                if eng::ribbon_tool_3d(ui, look, icon_of(tool), tool.title(), editor.tool == tool)
                    .clicked()
                    && editor.tool != tool
                {
                    editor.set_tool(tool);
                }
            }
        });
    }
    if editor.tool == Tool::Constrain {
        eng::ribbon_group(ui, look, tr("Relations"), |ui| {
            for relation in Relation::ALL {
                if eng::ribbon_tool(
                    ui,
                    look,
                    relation.icon(),
                    relation.title(),
                    editor.relation == relation,
                )
                .clicked()
                    && editor.relation != relation
                {
                    editor.relation = relation;
                    editor.cancel();
                }
            }
        });
    }
    eng::ribbon_group(ui, look, tr("View"), |ui| {
        ui.add_enabled_ui(editor.can_undo(), |ui| {
            if eng::ribbon_tool(ui, look, "undo", tr("Undo"), false).clicked() {
                editor.undo();
            }
        });
        ui.add_enabled_ui(editor.can_redo(), |ui| {
            if eng::ribbon_tool(ui, look, "redo", tr("Redo"), false).clicked() {
                editor.redo();
            }
        });
        if eng::ribbon_tool(ui, look, "fit", tr("Fit view"), false).clicked() {
            editor.fit();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use nodalia_theme::{Accent, Mode};

    #[test]
    fn the_studio_draws_in_both_themes_with_either_side_panel_put_away() {
        let base = tempfile::tempdir().unwrap();
        for mode in [Mode::Light, Mode::Dark] {
            let p = mode.palette_with(Accent::Vortia);
            let look = Look::new(p);
            for key in ["region", "model"] {
                for (browser_shut, properties_shut) in
                    [(false, false), (true, false), (false, true), (true, true)]
                {
                    let mut form = NewCase::new(base.path());
                    assert!(form.choose_named(key));
                    form.browser_shut = browser_shut;
                    form.properties_shut = properties_shut;
                    let ctx = egui::Context::default();
                    for _ in 0..2 {
                        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                            let _ = form.studio(ui, &look, &p, None);
                        });
                        out.textures_delta.clear();
                    }
                    assert_eq!(form.browser_shut, browser_shut);
                    assert_eq!(form.properties_shut, properties_shut);
                }
            }
        }
    }

    #[test]
    fn the_side_panels_leave_the_canvas_most_of_a_laptop_screen() {
        // A 1280 point window: what is not a side panel, a margin or a gap is the drawing's.
        let sides = BROWSER + PROPERTIES + 2.0 * (16.0 + layout::GAP);
        assert!(1280.0 - sides > 0.55 * 1280.0, "{sides}");
        let part = PART_BROWSER + PART_PROPERTIES + 2.0 * (16.0 + layout::GAP);
        assert!(1280.0 - part > 0.5 * 1280.0, "{part}");
    }
}
