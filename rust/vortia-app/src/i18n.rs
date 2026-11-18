//! The language of the interface: English, or Spanish for what the table below covers.
//!
//! The English text is the key, as in gettext: `tr("Run the case")` is that same text in
//! English and its Spanish line in Spanish. Text the table does not know comes back as it
//! was, so a missing line is an English label, never an empty one. The language is one
//! process-wide switch, read by every widget on each frame, so changing it in the
//! Preferences takes effect at once; only the menu bar the desktop owns (the global menu)
//! keeps the language it was built with until the next start.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// What the user chose in the Preferences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Choice {
    /// The language of the system, English when it is not one the app has.
    #[default]
    System,
    English,
    Spanish,
}

impl Choice {
    pub const ALL: [Choice; 3] = [Choice::System, Choice::English, Choice::Spanish];

    pub fn key(self) -> &'static str {
        match self {
            Choice::System => "system",
            Choice::English => "en",
            Choice::Spanish => "es",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }

    /// The language to speak: the choice itself, or the system's for [`Choice::System`].
    pub fn resolve(self, system: Option<&str>) -> Lang {
        match self {
            Choice::English => Lang::En,
            Choice::Spanish => Lang::Es,
            Choice::System => match system {
                Some(tag) if tag.trim().to_ascii_lowercase().starts_with("es") => Lang::Es,
                _ => Lang::En,
            },
        }
    }

    /// The label in the Preferences; each language is named in itself.
    pub fn label(self) -> &'static str {
        match self {
            Choice::System => tr("Follow the system"),
            Choice::English => "English",
            Choice::Spanish => "Español",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The language of the system as the environment names it (`es_ES.UTF-8`).
pub fn system_tag() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty() && value != "C" && value != "POSIX")
}

pub fn set(lang: Lang) {
    CURRENT.store(
        match lang {
            Lang::En => 0,
            Lang::Es => 1,
        },
        Ordering::Relaxed,
    );
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::Es,
        _ => Lang::En,
    }
}

/// Makes the language of a choice the current one.
pub fn apply(choice: Choice) {
    set(choice.resolve(system_tag().as_deref()));
}

/// The text in the current language.
pub fn tr(english: &'static str) -> &'static str {
    translate(current(), english)
}

/// A line with `{}` holes, translated and then filled in order: `trf("Saved to {}.", &[&path])`.
pub fn trf(english: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut args = args.iter();
    let mut rest = tr(english);
    while let Some(at) = rest.find("{}") {
        out.push_str(rest.get(..at).unwrap_or_default());
        if let Some(arg) = args.next() {
            out.push_str(&arg.to_string());
        }
        rest = rest.get(at + 2..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// The text in a language, without touching the process-wide one.
pub fn translate(lang: Lang, english: &'static str) -> &'static str {
    match lang {
        Lang::En => english,
        Lang::Es => spanish().get(english).copied().unwrap_or(english),
    }
}

fn spanish() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| ES.iter().copied().collect())
}

/// English → Spanish. The wording follows the other Nodalia apps ("Caso", "Ejecutar").
pub const ES: &[(&str, &str)] = &[
    ("About Vortia", "Acerca de Vortia"),
    ("Analyse", "Analizar"),
    ("Appearance", "Apariencia"),
    ("Area", "Área"),
    ("Arrows", "Flechas"),
    ("Average", "Media"),
    ("Back to the case", "Volver al caso"),
    ("Body", "Cuerpo"),
    ("Boundary conditions", "Condiciones de contorno"),
    ("Calculate", "Calcular"),
    ("Case", "Caso"),
    ("Cavity", "Cavidad"),
    ("Cell edges", "Aristas de las celdas"),
    ("Cells", "Celdas"),
    ("Cells across", "Celdas a lo ancho"),
    ("Cells ahead", "Celdas por delante"),
    ("Cells along", "Celdas a lo largo"),
    ("Cells along a side", "Celdas por lado"),
    ("Cells along the plate", "Celdas a lo largo de la placa"),
    ("Channel", "Canal"),
    ("Checking", "Comprobando"),
    ("Choose at least one wall.", "Elige al menos una pared."),
    ("Close", "Cerrar"),
    ("Close a dialog", "Cerrar un diálogo"),
    ("Close case", "Cerrar el caso"),
    ("Close the case", "Cerrar el caso"),
    ("Colour by", "Colorear por"),
    ("Conditions", "Condiciones"),
    ("Conical nozzle", "Tobera cónica"),
    ("Converged", "Convergido"),
    (
        "Convergence appears once the solver has checked the run.",
        "La convergencia aparece cuando el solver ha comprobado la ejecución.",
    ),
    ("Converging length", "Longitud convergente"),
    ("Create case", "Crear el caso"),
    ("Cutting the cells…", "Cortando las celdas…"),
    ("Cutting…", "Cortando…"),
    ("Dark", "Oscuro"),
    ("Density", "Densidad"),
    ("Diverging length", "Longitud divergente"),
    ("End", "Fin"),
    ("End time", "Tiempo final"),
    ("Equations", "Ecuaciones"),
    ("Event", "Evento"),
    (
        "Every setting has to be a number.",
        "Todos los ajustes tienen que ser números.",
    ),
    ("Faces", "Caras"),
    ("Failed", "Fallido"),
    ("Field", "Campo"),
    ("Field change", "Cambio del campo"),
    ("Fields", "Campos"),
    ("File", "Archivo"),
    ("Finished", "Terminado"),
    ("Flat plate", "Placa plana"),
    ("Fluid", "Fluido"),
    ("Flux out", "Flujo de salida"),
    ("Folder for the new case", "Carpeta del caso nuevo"),
    ("Folder of a case", "Carpeta de un caso"),
    ("Follow the system", "Seguir el sistema"),
    ("Following…", "Siguiendo…"),
    (
        "Force and moment on the walls, with their coefficients.",
        "Fuerza y momento sobre las paredes, con sus coeficientes.",
    ),
    ("Forces", "Fuerzas"),
    ("Forces on plate", "Fuerzas sobre la placa"),
    ("From", "Desde"),
    ("From sketch", "Desde un croquis"),
    (
        "Go to the boundary conditions",
        "Ir a las condiciones de contorno",
    ),
    ("Go to the fluid", "Ir al fluido"),
    ("Go to the mesh", "Ir a la malla"),
    ("Go to the results", "Ir a los resultados"),
    ("Go to the run", "Ir a la ejecución"),
    ("Go to the run settings", "Ir a los ajustes de ejecución"),
    ("Go to the schemes", "Ir a los esquemas"),
    ("Go to the solver numbers", "Ir a los números del solver"),
    ("Go to the turbulence model", "Ir al modelo de turbulencia"),
    ("Go to the viscosity", "Ir a la viscosidad"),
    ("Heat in", "Calor entrante"),
    ("Height", "Altura"),
    ("Home", "Inicio"),
    ("Language", "Idioma"),
    ("Inlet diameter", "Diámetro de entrada"),
    ("Inlet speed", "Velocidad de entrada"),
    ("Inlet temperature", "Temperatura de entrada"),
    ("Inside", "Interior"),
    ("Integral", "Integral"),
    (
        "It appears once a run has written the flux (phi).",
        "Aparece cuando una ejecución ha escrito el flujo (phi).",
    ),
    ("Keyboard shortcuts", "Atajos de teclado"),
    ("Length", "Longitud"),
    ("Lid speed", "Velocidad de la tapa"),
    ("Light", "Claro"),
    ("Line", "Línea"),
    ("Local mass", "Masa local"),
    ("Log", "Registro"),
    ("Look again", "Mirar de nuevo"),
    ("Mass balance", "Balance de masa"),
    ("Maximize", "Maximizar"),
    ("Maximum", "Máximo"),
    ("Mean shear", "Cortante medio"),
    ("Menu", "Menú"),
    ("Mesh", "Malla"),
    ("Minimize", "Minimizar"),
    ("Minimum", "Mínimo"),
    ("Mixing length", "Longitud de mezcla"),
    ("Moments about", "Momentos respecto a"),
    ("Name", "Nombre"),
    ("Net", "Neto"),
    ("New", "Nuevo"),
    ("New case", "Caso nuevo"),
    ("No finite values.", "No hay valores finitos."),
    (
        "Nothing here yet. Open a case folder to get started.",
        "Aún no hay nada. Abre la carpeta de un caso para empezar.",
    ),
    ("Nothing matches.", "Nada coincide."),
    ("Nothing to save yet.", "Todavía no hay nada que guardar."),
    ("Numerics", "Numérica"),
    ("Open", "Abrir"),
    ("Open a case", "Abrir un caso"),
    ("Open case", "Abrir un caso"),
    ("Outlet diameter", "Diámetro de salida"),
    ("Outlet pressure", "Presión de salida"),
    ("Patch", "Parche"),
    ("Patches", "Parches"),
    ("Pause", "Pausar"),
    ("Pause or resume", "Pausar o reanudar"),
    ("Pause or resume the run", "Pausar o reanudar la ejecución"),
    ("Paused", "En pausa"),
    (
        "Per unit density (m²/s², m³/s²): the solver is incompressible.",
        "Por unidad de densidad (m²/s², m³/s²): el solver es incompresible.",
    ),
    ("Perpendicular to", "Perpendicular a"),
    ("Physics", "Física"),
    ("Planar nozzle", "Tobera plana"),
    ("Planar slot", "Ranura plana"),
    ("Plate length", "Longitud de la placa"),
    ("Points", "Puntos"),
    ("Preferences", "Preferencias"),
    ("Pressure", "Presión"),
    ("Probe", "Sonda"),
    ("Probes", "Sondas"),
    ("Quit", "Salir"),
    ("Read", "Leer"),
    ("Reading the case…", "Leyendo el caso…"),
    ("Reading the field…", "Leyendo el campo…"),
    ("Reading the mesh…", "Leyendo la malla…"),
    ("Reading…", "Leyendo…"),
    ("Ready", "Listo"),
    ("Recent", "Recientes"),
    ("Redo", "Rehacer"),
    ("Reference values", "Valores de referencia"),
    ("Reload", "Recargar"),
    ("Residuals", "Residuos"),
    ("Restore", "Restaurar"),
    ("Results", "Resultados"),
    ("Resume", "Reanudar"),
    ("Revert", "Deshacer los cambios"),
    ("Revolved (5° wedge)", "De revolución (cuña de 5°)"),
    ("Rounded throat", "Garganta redondeada"),
    ("Run", "Ejecutar"),
    ("Run settings", "Ajustes de ejecución"),
    ("Run the case", "Ejecutar el caso"),
    ("Running", "En ejecución"),
    ("Running…", "Ejecutando…"),
    ("Sample", "Muestra"),
    ("Sampling…", "Muestreando…"),
    ("Save", "Guardar"),
    ("Save CSV", "Guardar CSV"),
    (
        "Say which folder the case goes in.",
        "Indica en qué carpeta va el caso.",
    ),
    ("Schemes", "Esquemas"),
    ("Search commands", "Buscar órdenes"),
    ("Section", "Sección"),
    ("Settings", "Ajustes"),
    ("Settling", "Asentándose"),
    ("Setup", "Preparación"),
    (
        "Shear, y+, forces and heat flux of this wall are under Results.",
        "El cortante, y+, las fuerzas y el flujo de calor de esta pared están en Resultados.",
    ),
    ("Side", "Lado"),
    ("Slice", "Corte"),
    ("Slip wall ahead", "Pared deslizante por delante"),
    ("Solver", "Solver"),
    ("Solver numbers", "Números del solver"),
    ("Speed", "Velocidad"),
    ("Start from", "Empezar desde"),
    ("Start time", "Tiempo inicial"),
    ("Stop", "Detener"),
    ("Stop the current run", "Detener la ejecución actual"),
    (
        "Stop the current run and wait for it to finish before opening another case.",
        "Detén la ejecución actual y espera a que termine antes de abrir otro caso.",
    ),
    ("Stop the run", "Detener la ejecución"),
    ("Stopped", "Detenido"),
    ("Stopping", "Deteniendo"),
    ("Straight duct", "Conducto recto"),
    ("Stream along", "Corriente a lo largo de"),
    ("Stream speed", "Velocidad de la corriente"),
    ("Streamlines", "Líneas de corriente"),
    (
        "Stress in Pa, force in N. Shear is the wall's pull on the fluid, so it is negative along the flow.",
        "Tensión en Pa, fuerza en N. El cortante es el tirón de la pared sobre el fluido, así que es negativo a lo largo del flujo.",
    ),
    ("Surface", "Superficie"),
    (
        "That is not a field of the case.",
        "Eso no es un campo del caso.",
    ),
    (
        "The calculation stopped unexpectedly.",
        "El cálculo se detuvo de forma inesperada.",
    ),
    ("The case could not be read.", "No se pudo leer el caso."),
    ("The case has no mesh yet.", "El caso aún no tiene malla."),
    (
        "The case has no results yet. Run it first.",
        "El caso aún no tiene resultados. Ejecútalo primero.",
    ),
    (
        "The case has no wall with results.",
        "El caso no tiene ninguna pared con resultados.",
    ),
    ("The centre of rotation", "El centro de giro"),
    (
        "The conditions cannot change while the case runs.",
        "Las condiciones no pueden cambiar mientras el caso se ejecuta.",
    ),
    ("The density", "La densidad"),
    (
        "The end time has to be after the start time.",
        "El tiempo final tiene que ser posterior al inicial.",
    ),
    (
        "The engine checks these entries when a run starts.",
        "El motor comprueba estas entradas al empezar una ejecución.",
    ),
    ("The field could not be read.", "No se pudo leer el campo."),
    (
        "The flow could not be computed.",
        "No se pudo calcular el flujo.",
    ),
    (
        "The line is outside the mesh",
        "La línea queda fuera de la malla",
    ),
    ("The mesh could not be read.", "No se pudo leer la malla."),
    (
        "The mesh has no boundary patches.",
        "La malla no tiene parches de contorno.",
    ),
    ("The position", "La posición"),
    (
        "The position needs a number.",
        "La posición necesita un número.",
    ),
    ("The reference area", "El área de referencia"),
    ("The reference density", "La densidad de referencia"),
    ("The reference length", "La longitud de referencia"),
    ("The reference pressure", "La presión de referencia"),
    ("The reference speed", "La velocidad de referencia"),
    (
        "The reference speed needs a number.",
        "La velocidad de referencia necesita un número.",
    ),
    ("The run finished.", "La ejecución terminó."),
    (
        "The run was stopped; what was written before stays in the case.",
        "La ejecución se detuvo; lo escrito antes se queda en el caso.",
    ),
    (
        "The section could not be computed.",
        "No se pudo calcular la sección.",
    ),
    (
        "The settings cannot change while the case runs.",
        "Los ajustes no pueden cambiar mientras el caso se ejecuta.",
    ),
    (
        "The solution left its physical range",
        "La solución salió de su rango físico",
    ),
    (
        "The solver's output appears here.",
        "La salida del solver aparece aquí.",
    ),
    (
        "The stream and the lift cannot be along the same axis.",
        "La corriente y la sustentación no pueden ir por el mismo eje.",
    ),
    (
        "The time is not a folder name.",
        "El tiempo no es el nombre de una carpeta.",
    ),
    (
        "The time step has to be greater than zero.",
        "El paso de tiempo tiene que ser mayor que cero.",
    ),
    ("The wedge angle", "El ángulo de la cuña"),
    (
        "The write interval has to be greater than zero.",
        "El intervalo de escritura tiene que ser mayor que cero.",
    ),
    (
        "They appear as the solver reports each step.",
        "Aparecen a medida que el solver informa de cada paso.",
    ),
    (
        "This patch is not in the mesh.",
        "Este parche no está en la malla.",
    ),
    ("Throat", "Garganta"),
    ("Throat diameter", "Diámetro de la garganta"),
    ("Time", "Tiempo"),
    ("Time limit reached", "Límite de tiempo alcanzado"),
    ("Time step", "Paso de tiempo"),
    ("Turbulence", "Turbulencia"),
    ("Turbulence intensity", "Intensidad de turbulencia"),
    (
        "Turbulent (k-ω SST, wall functions)",
        "Turbulento (k-ω SST, funciones de pared)",
    ),
    ("Type", "Tipo"),
    ("Undo", "Deshacer"),
    ("Unknown", "Desconocido"),
    ("Viscosity", "Viscosidad"),
    ("Viscous", "Viscoso"),
    ("Volume", "Volumen"),
    ("Walls", "Paredes"),
    (
        "A compressible-flow solver and its desktop app, written in Rust.",
        "Un solver de flujo compresible y su aplicación de escritorio, escritos en Rust.",
    ),
    ("Axisymmetric nozzle", "Tobera axisimétrica"),
    ("Adding up the field…", "Sumando el campo…"),
    ("Adding up the faces…", "Sumando las caras…"),
    ("At", "En"),
    ("To", "Hasta"),
    ("Blocks along the wall", "Bloques a lo largo de la pared"),
    ("Browser", "Explorador"),
    ("line's start", "inicio de la línea"),
    ("line's end", "fin de la línea"),
    ("lift along", "sustentación según"),
    ("positive against the axis", "positivo contra el eje"),
    ("from rho", "de rho"),
    ("y+ (min, mean, max)", "y+ (mín., media, máx.)"),
    (
        "Saved to system/controlDict.",
        "Guardado en system/controlDict.",
    ),
    (
        "steps or s, as the case writes",
        "pasos o s, según escriba el caso",
    ),
    (
        "nonuniform (a value for every cell)",
        "no uniforme (un valor por cada celda)",
    ),
    (
        "Times {} to {} ({} written): mean {} %, worst {} %",
        "Tiempos {} a {} ({} escritos): media {} %, peor {} %",
    ),
    ("{} cells", "{} celdas"),
    ("{} to {} s", "{} a {} s"),
    ("Version {}", "Versión {}"),
    ("Saved to {}.", "Guardado en {}."),
    ("Saved to {}/{}.", "Guardado en {}/{}."),
    ("Running · {}", "En ejecución · {}"),
    ("Forces on {}", "Fuerzas sobre {}"),
    ("Save {}", "Guardar {}"),
    ("Time {}", "Tiempo {}"),
    ("from the {} folder", "de la carpeta {}"),
    ("'{}' is not a folder.", "«{}» no es una carpeta."),
    ("Distance, point {} to {}", "Distancia, punto {} a {}"),
    (
        "Length along, point {} to {}",
        "Longitud a lo largo, punto {} a {}",
    ),
    ("Height, point {} to {}", "Altura, punto {} a {}"),
    ("Radius of arc {}", "Radio del arco {}"),
    ("Angle, point {} to {}", "Ángulo, punto {} a {}"),
    ("Warming up", "Calentando"),
    ("Wedge angle", "Ángulo de la cuña"),
    (
        "What goes through a plane across the domain.",
        "Lo que atraviesa un plano a través del dominio.",
    ),
    ("Write interval", "Intervalo de escritura"),
    ("Writes every", "Escribe cada"),
    ("Written up to", "Escrito hasta"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_no_repeated_english_line() {
        let mut keys: Vec<_> = ES.iter().map(|(en, _)| *en).collect();
        let n = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), n);
        assert!(ES.iter().all(|(en, es)| !en.is_empty() && !es.is_empty()));
    }

    /// A line whose English text no longer appears in the source translates nothing.
    #[test]
    fn every_line_of_the_table_is_used_by_the_app() {
        fn sources(dir: &std::path::Path, text: &mut String) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    sources(&path, text);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("i18n.rs")
                {
                    text.push_str(&std::fs::read_to_string(&path).unwrap_or_default());
                }
            }
        }
        let mut text = String::new();
        sources(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut text,
        );
        let unused: Vec<_> = ES
            .iter()
            .filter(|(en, _)| !text.contains(&format!("\"{en}\"")))
            .map(|(en, _)| *en)
            .collect();
        assert!(unused.is_empty(), "not in the source: {unused:?}");
    }

    #[test]
    fn spanish_translates_what_it_knows_and_leaves_the_rest() {
        // The switch is process-wide, so the tests leave it alone.
        assert_eq!(translate(Lang::Es, "Run the case"), "Ejecutar el caso");
        assert_eq!(translate(Lang::Es, "Not in the table"), "Not in the table");
        assert_eq!(translate(Lang::En, "Run the case"), "Run the case");
    }

    #[test]
    fn holes_are_filled_in_order_whatever_the_language() {
        assert_eq!(trf("Saved to {}/{}.", &[&"a", &"b"]), "Saved to a/b.");
        assert_eq!(trf("Not in the table {}", &[&1]), "Not in the table 1");
        assert_eq!(trf("Saved to {}.", &[]), "Saved to .");
    }

    #[test]
    fn the_system_language_picks_spanish_only_for_spanish() {
        assert_eq!(Choice::System.resolve(Some("es_ES.UTF-8")), Lang::Es);
        assert_eq!(Choice::System.resolve(Some("en_US.UTF-8")), Lang::En);
        assert_eq!(Choice::System.resolve(None), Lang::En);
        assert_eq!(Choice::English.resolve(Some("es")), Lang::En);
        assert_eq!(Choice::Spanish.resolve(None), Lang::Es);
    }
}
