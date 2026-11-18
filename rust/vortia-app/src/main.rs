//! Vortia, the desktop app of the Rust engine.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use vortia_app::app::{self, TabName, VortiaApp};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: app::viewport(),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "Vortia",
        options,
        Box::new(|cc| {
            // `vortia-app CASE [--run] [--mesh] [--field NAME] [--results] [--conditions] [--numerics] [--schemes] [--settings] [--fluid] [--turbulence] [--viscosity] [--new-sketch]`: open a case,
            // start it at once, or show its mesh (coloured with a field), conditions or settings.
            let mut args: Vec<_> = std::env::args_os().skip(1).collect();
            let case = if args
                .first()
                .is_some_and(|a| !a.to_string_lossy().starts_with("--"))
            {
                Some(args.remove(0))
            } else {
                None
            };
            let flags = args;
            let has = |flag: &str| flags.iter().any(|a| a == flag);
            let mut app = VortiaApp::new(&cc.egui_ctx, case, has("--run"));
            if has("--new-sketch") {
                app.show_new_sketch();
            } else if let Some(at) = flags.iter().position(|a| a == "--field")
                && let Some(field) = flags.get(at + 1)
            {
                app.show_mesh_field(&field.to_string_lossy());
            } else if has("--mesh") {
                app.show_mesh();
            } else if has("--results") {
                app.show_tab(TabName::Results);
            } else if has("--conditions") {
                app.show_tab(TabName::Conditions);
            } else if has("--numerics") {
                app.show_tab(TabName::Numerics);
            } else if has("--schemes") {
                app.show_tab(TabName::Schemes);
            } else if has("--settings") {
                app.show_tab(TabName::Settings);
            } else if has("--fluid") {
                app.show_tab(TabName::Fluid);
            } else if has("--turbulence") {
                app.show_tab(TabName::Turbulence);
            } else if has("--viscosity") {
                app.show_tab(TabName::Transport);
            }
            Ok(Box::new(app))
        }),
    )
}
