//! Home's "New": the form that starts a case (`vortia_cases`).

use std::path::{Path, PathBuf};

use eframe::egui::{self, DragValue, Ui};
use nodalia_theme::engineering::Look;
use nodalia_theme::office;
use nodalia_theme::{Palette, space};
use vortia_cases::{Channel, Geometry, Nozzle, NozzleKind};

/// Which of the cases Home offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Channel,
    PlanarNozzle,
    AxisymmetricNozzle,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Channel, Kind::PlanarNozzle, Kind::AxisymmetricNozzle];

    fn title(self) -> &'static str {
        match self {
            Kind::Channel => "Channel",
            Kind::PlanarNozzle => "Planar nozzle",
            Kind::AxisymmetricNozzle => "Axisymmetric nozzle",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Kind::Channel => {
                "A straight 2D channel with the fluid coming in on the left: laminar, incompressible, steady (simpleFoam)."
            }
            Kind::PlanarNozzle => {
                "A converging-diverging slot between two walls: compressible air, k-ω SST, steady (rhoSimpleFoam)."
            }
            Kind::AxisymmetricNozzle => {
                "A converging-diverging nozzle as a 5° wedge of its body of revolution: compressible air, k-ω SST, steady (rhoSimpleFoam)."
            }
        }
    }
}

/// What the form holds.
pub(super) struct NewCase {
    kind: Kind,
    /// The folder the case is written to.
    folder: String,
    channel: Channel,
    planar: Nozzle,
    wedge: Nozzle,
}

/// What the form asks for.
pub(super) enum Ask {
    Nothing,
    Create,
}

impl NewCase {
    /// A form that proposes `base/channel`, or the first name after it that is free.
    pub(super) fn new(base: &Path) -> Self {
        let mut form = Self {
            kind: Kind::Channel,
            folder: String::new(),
            channel: Channel::default(),
            planar: Nozzle::new(NozzleKind::Planar),
            wedge: Nozzle::new(NozzleKind::Axisymmetric),
        };
        form.propose(base);
        form
    }

    /// Where new cases go: the person's `Vortia` folder.
    pub(super) fn default_base() -> PathBuf {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map_or_else(|| PathBuf::from("."), PathBuf::from)
            .join("Vortia")
    }

    fn propose(&mut self, base: &Path) {
        let stem = match self.kind {
            Kind::Channel => "channel",
            Kind::PlanarNozzle => "planar-nozzle",
            Kind::AxisymmetricNozzle => "nozzle",
        };
        let free = (1..1000)
            .map(|n| {
                if n == 1 {
                    stem.to_owned()
                } else {
                    format!("{stem}-{n}")
                }
            })
            .map(|name| base.join(name))
            .find(|path| !path.exists())
            .unwrap_or_else(|| base.join(stem));
        self.folder = free.display().to_string();
    }

    pub(super) fn geometry(&self) -> Geometry {
        match self.kind {
            Kind::Channel => Geometry::Channel(self.channel.clone()),
            Kind::PlanarNozzle => Geometry::Nozzle(self.planar.clone()),
            Kind::AxisymmetricNozzle => Geometry::Nozzle(self.wedge.clone()),
        }
    }

    /// Writes the case; the folder it made, or why not.
    pub(super) fn create(&self) -> Result<PathBuf, String> {
        let dir = PathBuf::from(self.folder.trim());
        if self.folder.trim().is_empty() {
            return Err("Say which folder the case goes in.".into());
        }
        vortia_cases::create(&dir, &self.geometry())
            .map(|_| dir)
            .map_err(|e| e.to_string())
    }

    pub(super) fn ui(&mut self, ui: &mut Ui, look: &Look, p: &Palette) -> Ask {
        let home = nodalia_theme::engineering::home_look(look);
        let mut ask = Ask::Nothing;
        office::section(ui, &home, "New case", |ui| {
            ui.horizontal(|ui| {
                for kind in Kind::ALL {
                    if ui
                        .selectable_label(self.kind == kind, kind.title())
                        .clicked()
                        && self.kind != kind
                    {
                        let base = Path::new(self.folder.trim())
                            .parent()
                            .map_or_else(Self::default_base, Path::to_path_buf);
                        self.kind = kind;
                        self.propose(&base);
                    }
                }
            });
            ui.add_space(space::SM);
            ui.label(self.kind.about());
            ui.add_space(space::SM);
            match self.kind {
                Kind::Channel => channel_fields(ui, &mut self.channel),
                Kind::PlanarNozzle => nozzle_fields(ui, &mut self.planar),
                Kind::AxisymmetricNozzle => nozzle_fields(ui, &mut self.wedge),
            }
            ui.add_space(space::SM);
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 120.0).clamp(160.0, 420.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.folder)
                        .hint_text("Folder for the new case")
                        .desired_width(width),
                );
                if super::accent_button(ui, p, "Create case").clicked() {
                    ask = Ask::Create;
                }
            });
        });
        ask
    }
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(label);
    add(ui);
    ui.end_row();
}

fn value(ui: &mut Ui, v: &mut f64, unit: &str) {
    let speed = 0.01 * v.abs().max(0.001);
    ui.add(
        DragValue::new(v)
            .speed(speed)
            .suffix(format!(" {unit}"))
            .max_decimals(6),
    );
}

fn count(ui: &mut Ui, v: &mut usize) {
    ui.add(DragValue::new(v).speed(0.2).range(1..=10_000));
}

fn channel_fields(ui: &mut Ui, c: &mut Channel) {
    egui::Grid::new("new_channel")
        .num_columns(2)
        .spacing([space::MD, space::SM])
        .show(ui, |ui| {
            row(ui, "Length", |ui| value(ui, &mut c.length, "m"));
            row(ui, "Height", |ui| value(ui, &mut c.height, "m"));
            row(ui, "Cells along", |ui| count(ui, &mut c.cells_x));
            row(ui, "Cells across", |ui| count(ui, &mut c.cells_y));
            row(ui, "Inlet speed", |ui| value(ui, &mut c.speed, "m/s"));
            row(ui, "Viscosity", |ui| value(ui, &mut c.viscosity, "m²/s"));
        });
}

fn nozzle_fields(ui: &mut Ui, n: &mut Nozzle) {
    egui::Grid::new("new_nozzle")
        .num_columns(2)
        .spacing([space::MD, space::SM])
        .show(ui, |ui| {
            row(ui, "Inlet diameter", |ui| {
                value(ui, &mut n.inlet_diameter, "mm")
            });
            row(ui, "Throat diameter", |ui| {
                value(ui, &mut n.throat_diameter, "mm")
            });
            row(ui, "Outlet diameter", |ui| {
                value(ui, &mut n.outlet_diameter, "mm")
            });
            row(ui, "Converging length", |ui| {
                value(ui, &mut n.converging_length, "mm")
            });
            row(ui, "Diverging length", |ui| {
                value(ui, &mut n.diverging_length, "mm")
            });
            row(ui, "Inlet speed", |ui| value(ui, &mut n.inlet_speed, "m/s"));
            row(ui, "Inlet temperature", |ui| {
                value(ui, &mut n.inlet_temperature, "K")
            });
            row(ui, "Outlet pressure", |ui| {
                value(ui, &mut n.outlet_pressure, "Pa")
            });
            row(ui, "Cells across", |ui| count(ui, &mut n.radial_cells));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_makes_a_case_the_app_opens() {
        for kind in Kind::ALL {
            let base = tempfile::tempdir().unwrap();
            let mut form = NewCase::new(base.path());
            form.kind = kind;
            form.propose(base.path());
            let dir = form.create().unwrap();
            assert!(dir.starts_with(base.path()));
            assert!(dir.join("system/controlDict").is_file(), "{kind:?}");
            assert!(dir.join("constant/polyMesh/points").is_file(), "{kind:?}");
        }
    }

    #[test]
    fn the_folder_proposed_is_never_one_that_exists() {
        let base = tempfile::tempdir().unwrap();
        let mut form = NewCase::new(base.path());
        assert!(form.folder.ends_with("channel"));
        let first = form.create().unwrap();
        form.propose(base.path());
        assert_ne!(PathBuf::from(&form.folder), first);
        assert!(form.folder.ends_with("channel-2"));
        // Creating over a case that is there is refused and leaves it alone.
        form.folder = first.display().to_string();
        let err = form.create().unwrap_err();
        assert!(err.contains("not empty"), "{err}");
        assert!(first.join("system/controlDict").is_file());
    }

    #[test]
    fn nothing_is_made_without_a_folder_or_with_numbers_that_cannot_work() {
        let base = tempfile::tempdir().unwrap();
        let mut form = NewCase::new(base.path());
        form.folder = "  ".into();
        assert!(form.create().is_err());
        form.propose(base.path());
        form.channel.length = -1.0;
        let err = form.create().unwrap_err();
        assert!(err.contains("length"), "{err}");
        assert!(!Path::new(form.folder.trim()).exists());
    }
}
