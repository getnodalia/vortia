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
    ("Duplicate case", "Duplicar caso"),
    (
        "'{}' is not a case: it has no system/controlDict.",
        "'{}' no es un caso: no tiene system/controlDict.",
    ),
    ("Copy of the case opened: {}", "Copia del caso abierta: {}"),
    ("Open a case first.", "Abre primero un caso."),
    (
        "The case has no folder name.",
        "El caso no tiene nombre de carpeta.",
    ),
    (
        "There are too many copies of this case.",
        "Hay demasiadas copias de este caso.",
    ),
    (
        "The case could not be copied: {}",
        "No se pudo copiar el caso: {}",
    ),
    ("Save picture", "Guardar imagen"),
    (
        "The picture could not be taken.",
        "No se pudo tomar la imagen.",
    ),
    ("General", "General"),
    ("Units and precision", "Unidades y precisión"),
    ("View", "Vista"),
    ("Performance", "Rendimiento"),
    ("Updates", "Actualizaciones"),
    ("Rainbow", "Arcoíris"),
    ("Viridis", "Viridis"),
    ("Greys", "Grises"),
    ("Interface language", "Idioma de la interfaz"),
    (
        "The menu bar the desktop owns changes language at the next start.",
        "La barra de menús que gestiona el escritorio cambia de idioma en el próximo arranque.",
    ),
    ("Cases", "Casos"),
    (
        "Open the last case when Vortia starts",
        "Abrir el último caso al iniciar Vortia",
    ),
    ("Clear the recent cases", "Vaciar los casos recientes"),
    ("Theme", "Tema"),
    ("Colours", "Colores"),
    ("Size", "Tamaño"),
    ("Interface size", "Tamaño de la interfaz"),
    (
        "Makes text and controls larger or smaller, for a screen that is far or dense.",
        "Agranda o reduce el texto y los controles, para una pantalla lejana o muy densa.",
    ),
    ("Numbers", "Números"),
    ("Significant digits", "Cifras significativas"),
    ("Units", "Unidades"),
    (
        "A case is kept in SI units (metres, kilograms, seconds, kelvin, pascals) and every value is shown in them.",
        "Un caso se guarda en unidades SI (metros, kilogramos, segundos, kelvin, pascales) y todos los valores se muestran en ellas.",
    ),
    ("Colour scale", "Escala de color"),
    ("Preview", "Vista previa"),
    (
        "Viridis and Greys stay readable in black and white and for colour-vision deficiencies.",
        "Viridis y Grises se leen en blanco y negro y con deficiencias de visión del color.",
    ),
    (
        "Draw the cell edges when a mesh opens",
        "Dibujar las aristas de las celdas al abrir una malla",
    ),
    ("Threads for a run", "Hilos de una ejecución"),
    (
        "The answer of a run is the same with any number of threads; only the time changes. It applies to the runs that start from now on.",
        "El resultado de una ejecución es el mismo con cualquier número de hilos; solo cambia el tiempo. Se aplica a las ejecuciones que empiecen a partir de ahora.",
    ),
    (
        "Look for a new version when Vortia starts",
        "Buscar una versión nueva al iniciar Vortia",
    ),
    ("{} on the list", "{} en la lista"),
    (
        "On the labels of the colour scale: {}, {}, {}.",
        "En las etiquetas de la escala de color: {}, {}, {}.",
    ),
    ("All ({})", "Todos ({})"),
    ("About Vortia", "Acerca de Vortia"),
    ("Report a problem", "Informar de un problema"),
    ("Airfoil", "Perfil aerodinámico"),
    (
        "A NACA 4-digit airfoil in a stream at any angle of attack, on a C-grid Vortia meshes itself: turbulent, incompressible, steady (simpleFoam) with Spalart–Allmaras or k-ω SST. Lift, drag and moment coefficients and the Cp curve come out of `vortia airfoil`. The NACA 0012 at Reynolds 6·10⁶ by default.",
        "Un perfil NACA de 4 cifras en una corriente con cualquier ángulo de ataque, en una malla en C que Vortia genera por sí mismo: turbulento, incompresible, estacionario (simpleFoam) con Spalart–Allmaras o k-ω SST. Los coeficientes de sustentación, resistencia y momento y la curva de Cp salen de `vortia airfoil`. El NACA 0012 a Reynolds 6·10⁶ por defecto.",
    ),
    ("Turbulence model", "Modelo de turbulencia"),
    ("NACA profile", "Perfil NACA"),
    ("Angle of attack", "Ángulo de ataque"),
    ("Chord", "Cuerda"),
    ("Reynolds number", "Número de Reynolds"),
    ("Distance to the far field", "Distancia al campo lejano"),
    ("Wake length", "Longitud de la estela"),
    (
        "Cells along each surface",
        "Celdas a lo largo de cada superficie",
    ),
    ("Cells to the far field", "Celdas hasta el campo lejano"),
    ("Cells along the wake", "Celdas a lo largo de la estela"),
    ("First cell y+", "y+ de la primera celda"),
    ("Jet into the atmosphere", "Chorro a la atmósfera"),
    ("Orifice radius", "Radio del orificio"),
    ("Atmosphere length", "Longitud de la atmósfera"),
    ("Atmosphere radius", "Radio de la atmósfera"),
    ("Total pressure", "Presión total"),
    ("Total temperature", "Temperatura total"),
    ("Ambient pressure", "Presión ambiente"),
    ("Cells along the axis", "Celdas a lo largo del eje"),
    ("Cells across the orifice", "Celdas a través del orificio"),
    ("Cells outside the orifice", "Celdas fuera del orificio"),
    (
        "A straight 2D channel with the fluid coming in on the left: laminar, incompressible, steady (simpleFoam).",
        "Un canal 2D recto con el fluido entrando por la izquierda: laminar, incompresible, estacionario (simpleFoam).",
    ),
    (
        "A square box closed on every side whose top wall slides: laminar, incompressible, steady (simpleFoam). Reynolds number 100 by default, the case of Ghia et al. (1982).",
        "Una caja cuadrada cerrada por todos lados cuya tapa se desliza: laminar, incompresible, estacionario (simpleFoam). Reynolds 100 por defecto, el caso de Ghia et al. (1982).",
    ),
    (
        "A flat plate in a uniform stream with a slip wall ahead of it: laminar, incompressible, steady (simpleFoam). Reynolds number 10⁵ on the plate by default, the boundary layer Blasius solved.",
        "Una placa plana en una corriente uniforme con una pared deslizante delante: laminar, incompresible, estacionario (simpleFoam). Reynolds 10⁵ sobre la placa por defecto, la capa límite que resolvió Blasius.",
    ),
    (
        "A converging-diverging slot between two walls: compressible air, k-ω SST, steady (rhoSimpleFoam).",
        "Una ranura convergente-divergente entre dos paredes: aire compresible, k-ω SST, estacionario (rhoSimpleFoam).",
    ),
    (
        "A converging-diverging nozzle as a 5° wedge of its body of revolution: compressible air, k-ω SST, steady (rhoSimpleFoam).",
        "Una tobera convergente-divergente como una cuña de 5° de su cuerpo de revolución: aire compresible, k-ω SST, estacionario (rhoSimpleFoam).",
    ),
    (
        "A small orifice in a wall that lets air out into the open atmosphere, as a 5° wedge of the body of revolution: compressible, k-ω SST, steady (rhoSimpleFoam). The practice case of a jet at a pressure ratio of 1.18.",
        "Un orificio pequeño en una pared que deja salir el aire a la atmósfera abierta, como una cuña de 5° del cuerpo de revolución: compresible, k-ω SST, estacionario (rhoSimpleFoam). El caso de práctica de un chorro con relación de presiones 1,18.",
    ),
    (
        "A duct whose wall you dimension in a sketch: lines and arcs held by lengths, heights and radii, meshed by Vortia itself and run as a nozzle (rhoSimpleFoam). The sketch is kept in the case.",
        "Un conducto cuya pared acotas en un croquis: líneas y arcos sujetos por longitudes, alturas y radios, mallado por el propio Vortia y calculado como tobera (rhoSimpleFoam). El croquis se guarda en el caso.",
    ),
    ("Open a geometry", "Abrir una geometría"),
    ("Half section", "Media sección"),
    ("Triangles (unstructured)", "Triángulos (no estructurada)"),
    ("Cell size", "Tamaño de celda"),
    ("Symmetry plane on the axis", "Plano de simetría en el eje"),
    (
        "An STL, OBJ or STEP file, a Fluent case (.cas.h5, .msh) or a Workbench project (.wbpj)",
        "Un archivo STL, OBJ o STEP, un caso de Fluent (.cas.h5, .msh) o un proyecto de Workbench (.wbpj)",
    ),
    (
        "{} cells, {} faces, {} points",
        "{} celdas, {} caras, {} puntos",
    ),
    ("Boundaries: {}", "Contornos: {}"),
    (
        "Only ASCII Fluent meshes (.msh) are read; this one is binary.",
        "Solo se leen mallas de Fluent (.msh) en ASCII; esta es binaria.",
    ),
    (
        "The Workbench project '{}' has no Fluent case or mesh.",
        "El proyecto de Workbench «{}» no tiene ningún caso ni malla de Fluent.",
    ),
    (
        "None of the Fluent files of the Workbench project is in '{}': {}.",
        "Ninguno de los archivos de Fluent del proyecto de Workbench está en «{}»: {}.",
    ),
    (
        "A .wbpj holds no mesh or case: they are in the folder '{}', which must be next to it. Copy the whole project (or archive it from Workbench as a .wbpz). It lists {}.",
        "Un .wbpj no contiene la malla ni el caso: están en la carpeta «{}», que tiene que estar a su lado. Copia el proyecto completo (o archívalo desde Workbench como .wbpz). Contiene {}.",
    ),
    (
        "Workbench project '{}': {}",
        "Proyecto de Workbench «{}»: {}",
    ),
    (
        "Its results ({}) are not shown yet: only the mesh.",
        "Sus resultados ({}) aún no se muestran: solo la malla.",
    ),
    ("Show geometry", "Ver geometría"),
    ("Close geometry", "Cerrar la geometría"),
    ("Reading the geometry…", "Leyendo la geometría…"),
    ("The file could not be read.", "No se pudo leer el archivo."),
    ("{} triangles, {} vertices", "{} triángulos, {} vértices"),
    ("Size {} × {} × {}", "Tamaño {} × {} × {}"),
    ("Area {}", "Área {}"),
    ("Closed solid, volume {}", "Sólido cerrado, volumen {}"),
    (
        "Not closed: {} open edges, {} edges shared by more than two triangles",
        "No está cerrada: {} aristas abiertas, {} aristas compartidas por más de dos triángulos",
    ),
    ("{} triangles have no area", "{} triángulos no tienen área"),
    ("Title", "Título"),
    ("What happened", "Qué ha pasado"),
    ("Open on GitHub", "Abrir en GitHub"),
    (
        "Nothing is sent from here: GitHub opens with this text for you to review. The version, the system and the solver are added; no path or case name is.",
        "Desde aquí no se envía nada: GitHub se abre con este texto para que lo revises. Se añaden la versión, el sistema y el solver; nunca una ruta ni el nombre del caso.",
    ),
    (
        "The text was too long for the address: it is on the clipboard, paste it into the issue.",
        "El texto era demasiado largo para la dirección: está en el portapapeles, pégalo en la incidencia.",
    ),
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
        "One number of saved reports side by side, against the baseline.",
        "Un número de los informes guardados, uno junto a otro, frente al de referencia.",
    ),
    (
        "Save a Section or Forces report, or add a report file, to compare runs.",
        "Guarda un informe de Sección o de Fuerzas, o añade un archivo de informe, para comparar ejecuciones.",
    ),
    ("Baseline", "Referencia"),
    ("{} at {}", "{} en {}"),
    ("Number", "Número"),
    (
        "{} files were not reports.",
        "{} archivos no eran informes.",
    ),
    (
        "A report file from another run",
        "Un archivo de informe de otra ejecución",
    ),
    ("Add", "Añadir"),
    (
        "That report is already in the list.",
        "Ese informe ya está en la lista.",
    ),
    (
        "There are too many reports in the list.",
        "Hay demasiados informes en la lista.",
    ),
    (
        "There are no reports to compare.",
        "No hay informes que comparar.",
    ),
    (
        "Tick at least one more report of the same kind.",
        "Marca al menos un informe más del mismo tipo.",
    ),
    (
        "Different section or reference definitions: compare the assumptions before interpreting a change.",
        "Secciones o referencias distintas: compara las hipótesis antes de interpretar un cambio.",
    ),
    ("Channel with a block", "Canal con un bloque"),
    ("Drawn region", "Región dibujada"),
    ("Around a CAD file", "Alrededor de un archivo CAD"),
    (
        "A part file (.vortia-part)",
        "Un archivo de pieza (.vortia-part)",
    ),
    ("Open part", "Abrir pieza"),
    ("Round edges", "Redondear aristas"),
    ("Chamfer edges", "Achaflanar aristas"),
    ("A point on each edge", "Un punto de cada arista"),
    ("Add an edge", "Añadir una arista"),
    (
        "The edge nearest each point, within a millimetre, is rounded: type a point on it, such as the middle of the edge.",
        "Se redondea la arista más cercana a cada punto, a menos de un milímetro: escribe un punto de ella, como el medio de la arista.",
    ),
    ("Save part", "Guardar pieza"),
    ("Opened {}", "Abierto {}"),
    (
        "The kernel stopped on this part: change the last feature.",
        "El núcleo se detuvo con esta pieza: cambia la última operación.",
    ),
    ("Remove corner", "Quitar vértice"),
    ("Add a corner", "Añadir un vértice"),
    ("NACA", "NACA"),
    ("Use this wing section", "Usar este perfil alar"),
    ("A .dxf drawing", "Un dibujo .dxf"),
    ("Use its largest outline", "Usar su contorno mayor"),
    ("Draw the outline", "Dibujar el contorno"),
    ("Use the drawn outline", "Usar el contorno dibujado"),
    ("Box", "Caja"),
    ("Cylinder", "Cilindro"),
    ("Cone", "Cono"),
    ("Sphere", "Esfera"),
    ("Torus", "Toro"),
    ("Extruded outline", "Contorno extruido"),
    ("Turned outline", "Contorno girado"),
    ("Cut away", "Restar"),
    ("Keep what both share", "Quedarse con lo común"),
    ("Lower corner", "Esquina inferior"),
    ("Centre of the base", "Centro de la base"),
    ("Radius at the base", "Radio en la base"),
    ("Radius at the top", "Radio en la punta"),
    ("Centre", "Centro"),
    ("Radius of the ring", "Radio del anillo"),
    ("Radius of the tube", "Radio del tubo"),
    ("Plane of the outline (z)", "Plano del contorno (z)"),
    (
        "Length it is pushed along z",
        "Longitud que se extruye en z",
    ),
    ("Outline", "Contorno"),
    (
        "The corners in order, in the plane of the outline.",
        "Los vértices en orden, en el plano del contorno.",
    ),
    (
        "A point of the axis (along z)",
        "Un punto del eje (paralelo a z)",
    ),
    ("Turned through", "Giro de"),
    ("Profile", "Perfil"),
    (
        "The corners in order: the distance from the axis, then the height.",
        "Los vértices en orden: la distancia al eje y después la altura.",
    ),
    ("Part", "Pieza"),
    (
        "The part could not be built.",
        "No se pudo construir la pieza.",
    ),
    ("{} corners", "{} vértices"),
    ("Model a part", "Modelar una pieza"),
    ("A wing", "Un ala"),
    (
        "A wing of constant NACA 2412 section: a metre of chord, two of span.",
        "Un ala de perfil NACA 2412 constante: un metro de cuerda y dos de envergadura.",
    ),
    ("Move up", "Subir"),
    ("Move down", "Bajar"),
    ("Remove", "Quitar"),
    ("Building the part…", "Construyendo la pieza…"),
    (
        "Volume {} m³, surface {} m²",
        "Volumen {} m³, superficie {} m²",
    ),
    ("File to export to", "Archivo de destino"),
    ("Save as STL", "Guardar como STL"),
    ("Save as STEP", "Guardar como STEP"),
    ("Pick in the viewer", "Elegir en el visor"),
    (
        "Click the edges to round or bevel; click a chosen edge again to let it go. The picture shows the part as it is before this step.",
        "Pulsa las aristas que redondear o achaflanar; vuelve a pulsar una elegida para soltarla. La imagen muestra la pieza tal como está antes de este paso.",
    ),
    ("Finish picking", "Terminar de elegir"),
    ("Saved {}", "Guardado {}"),
    (
        "Model a part first: add a feature.",
        "Modela primero una pieza: añade una operación.",
    ),
    (
        "The part is still being built.",
        "La pieza todavía se está construyendo.",
    ),
    (
        "The flow comes along x.",
        "La corriente viene a lo largo de x.",
    ),
    (
        "Nothing yet. Add a shape, or a wing to start from; each next shape can be added to the part, cut out of it or intersected with it.",
        "Todavía nada. Añade una forma, o un ala para empezar; cada forma siguiente se puede sumar a la pieza, restarle o intersecarse con ella.",
    ),
    (
        "A part you model here with Nodalia's own kernel, like the ones of Torno and Forja: boxes, cylinders, cones, spheres, tori, a wing section or a DXF outline pushed along an axis, an outline turned about it, each added to the part, cut out of it or intersected with it. The flow around it is meshed in 3D by octree refinement (Spalart–Allmaras or k-ω SST); the part can also be saved as an STL. The drag is the force on the patch `body`.",
        "Una pieza que modelas aquí con el núcleo propio de Nodalia, como el de Torno y Forja: cajas, cilindros, conos, esferas, toros, un perfil alar o un contorno DXF extruido a lo largo de un eje, un contorno girado alrededor de él, cada uno sumado a la pieza, restado de ella o intersecado con ella. El flujo alrededor se malla en 3D por refinamiento de octree (Spalart–Allmaras o k-ω SST); la pieza también se puede guardar como STL. La resistencia es la fuerza sobre el parche `body`.",
    ),
    (
        "A body read from a file in a wind tunnel built around it: an STL, OBJ or STEP surface is meshed in 3D by octree refinement (Spalart–Allmaras or k-ω SST), a DXF drawing, say the side of a car or a wing section, in 2D with triangles (laminar, incompressible). The drag is the force on the patch `body`.",
        "Un cuerpo leído de un archivo en un túnel de viento construido a su alrededor: una superficie STL, OBJ o STEP se malla en 3D por refinamiento de octree (Spalart–Allmaras o k-ω SST); un dibujo DXF, por ejemplo el perfil de un coche o una sección de ala, se malla en 2D con triángulos (laminar, incompresible). La resistencia es la fuerza sobre el parche `body`.",
    ),
    ("Say which file to read.", "Indica qué archivo leer."),
    (
        "The scale of the file must be a positive number.",
        "La escala del archivo debe ser un número positivo.",
    ),
    ("Read a file first.", "Lee antes un archivo."),
    (
        "A .stl, .obj, .step or .dxf file",
        "Un archivo .stl, .obj, .step o .dxf",
    ),
    ("Read file", "Leer archivo"),
    ("Scale of the file", "Escala del archivo"),
    (
        "Multiplies every length of the file: 0.001 turns the millimetres of an STL or OBJ into metres. A STEP file carries its own unit.",
        "Multiplica todas las longitudes del archivo: 0,001 convierte en metros los milímetros de un STL u OBJ. Un archivo STEP lleva su propia unidad.",
    ),
    ("The body", "El cuerpo"),
    (
        "{} triangles, {} × {} × {} m",
        "{} triángulos, {} × {} × {} m",
    ),
    (
        "The surface is not closed: {} open edges, {} shared by more than two triangles.",
        "La superficie no es cerrada: {} aristas abiertas, {} compartidas por más de dos triángulos.",
    ),
    (
        "Reference length (0: along the stream)",
        "Longitud de referencia (0: a lo largo de la corriente)",
    ),
    ("Tunnel ahead of the body", "Túnel delante del cuerpo"),
    ("Tunnel behind the body", "Túnel detrás del cuerpo"),
    ("Tunnel at each side", "Túnel a cada lado"),
    ("Base cells along the length", "Celdas base a lo largo"),
    ("Refinements at the body", "Refinados en el cuerpo"),
    ("Refinements in the wake", "Refinados en la estela"),
    (
        "The free-stream pressure",
        "La presión de la corriente libre",
    ),
    (
        "Pressure and friction on the surface",
        "Presión y fricción en la superficie",
    ),
    (
        "Cp and Cf on the faces of the walls chosen in Forces.",
        "Cp y Cf en las caras de las paredes elegidas en Fuerzas.",
    ),
    ("Plot along", "Dibujar a lo largo de"),
    ("Free-stream pressure", "Presión de la corriente libre"),
    ("Show the coefficients", "Mostrar los coeficientes"),
    ("Reading the faces…", "Leyendo las caras…"),
    (
        "Pressure coefficient (suction up)",
        "Coeficiente de presión (succión arriba)",
    ),
    (
        "Friction along the stream",
        "Fricción a lo largo de la corriente",
    ),
    ("Dynamic pressure", "Presión dinámica"),
    ("Cp, lowest and highest", "Cp, mínimo y máximo"),
    ("Cf, lowest and highest", "Cf, mínimo y máximo"),
    (
        "Share of the area with reversed flow",
        "Parte del área con flujo invertido",
    ),
    ("There are no faces to plot", "No hay caras que dibujar"),
    ("Paint Cp on the body", "Pintar Cp sobre el cuerpo"),
    ("Paint Cf on the body", "Pintar Cf sobre el cuerpo"),
    ("Back to the field", "Volver al campo"),
    ("Hide", "Ocultar"),
    ("Display", "Mostrar"),
    ("Show the case panel", "Mostrar el panel del caso"),
    ("Hide the case panel", "Ocultar el panel del caso"),
    ("Wall by part", "Pared por pieza"),
    (
        "A wall for each of the {} parts, to read their forces apart",
        "Una pared para cada una de las {} piezas, para leer sus fuerzas por separado",
    ),
    ("Prism layers on the wall", "Capas prismáticas en la pared"),
    ("Growth of each layer", "Crecimiento de cada capa"),
    (
        "First layer for y+ (0: a tenth of the cell)",
        "Primera capa para y+ (0: una décima de la celda)",
    ),
    ("Laminar", "Laminar"),
    ("The drawing", "El dibujo"),
    ("{} closed outlines", "{} contornos cerrados"),
    ("Kinematic viscosity", "Viscosidad cinemática"),
    (
        "Cell size at the body (0: automatic)",
        "Tamaño de celda en el cuerpo (0: automático)",
    ),
    ("Depth of the one cell", "Espesor de la única celda"),
    (
        "Name one edge inlet: the fluid comes in there.",
        "Pon el nombre «inlet» a una arista: por ahí entra el fluido.",
    ),
    (
        "Name one edge outlet: the fluid leaves there.",
        "Pon el nombre «outlet» a una arista: por ahí sale el fluido.",
    ),
    ("Name of edge {}", "Nombre de la arista {}"),
    ("Size of the triangles", "Tamaño de los triángulos"),
    ("Check for updates…", "Buscar actualizaciones…"),
    ("A saved sketch", "Un croquis guardado"),
    ("Open sketch", "Abrir croquis"),
    ("Blank", "En blanco"),
    ("Move", "Mover"),
    ("Arc", "Arco"),
    ("Dimension", "Cota"),
    ("Delete", "Borrar"),
    ("Distance", "Distancia"),
    ("Along x", "A lo largo de x"),
    ("Grid", "Rejilla"),
    ("Fit view", "Ajustar la vista"),
    ("Rectangle", "Rectángulo"),
    ("Circle", "Círculo"),
    ("Slot", "Ranura"),
    ("Polygon", "Polígono"),
    ("Fillet", "Empalme"),
    ("Chamfer", "Chaflán"),
    ("Constrain", "Restricción"),
    ("Draw", "Dibujar"),
    ("Modify", "Modificar"),
    ("Hold", "Fijar"),
    ("Horizontal", "Horizontal"),
    ("Vertical", "Vertical"),
    ("Parallel", "Paralela"),
    ("Perpendicular", "Perpendicular"),
    ("Equal", "Igual"),
    ("Tangent", "Tangente"),
    ("Coincident", "Coincidente"),
    ("Fix", "Fijar punto"),
    ("Sides", "Lados"),
    ("Radius", "Radio"),
    (
        "Click a corner of the rectangle.",
        "Pulsa una esquina del rectángulo.",
    ),
    ("Click the opposite corner.", "Pulsa la esquina opuesta."),
    (
        "Click the centre of the circle.",
        "Pulsa el centro del círculo.",
    ),
    (
        "Click a point on the circle.",
        "Pulsa un punto de la circunferencia.",
    ),
    (
        "Click the centre of one round end.",
        "Pulsa el centro de un extremo redondo.",
    ),
    (
        "Click the centre of the other end.",
        "Pulsa el centro del otro extremo.",
    ),
    (
        "Click a point as far from the axis as the slot is wide.",
        "Pulsa un punto tan lejos del eje como ancha sea la ranura.",
    ),
    (
        "Click the centre of the polygon.",
        "Pulsa el centro del polígono.",
    ),
    ("Click a corner.", "Pulsa una esquina."),
    (
        "Click the corner to round.",
        "Pulsa la esquina que quieres redondear.",
    ),
    (
        "Click the corner to cut.",
        "Pulsa la esquina que quieres cortar.",
    ),
    (
        "Click the point to hold in place.",
        "Pulsa el punto que quieres dejar fijo.",
    ),
    ("Click the first edge.", "Pulsa el primer borde."),
    ("Click the second edge.", "Pulsa el segundo borde."),
    ("Click on a corner.", "Pulsa sobre una esquina."),
    ("Pick two different points.", "Elige dos puntos distintos."),
    ("Pick two different edges.", "Elige dos bordes distintos."),
    (
        "Pick a line and an arc or circle.",
        "Elige una línea y un arco o círculo.",
    ),
    (
        "Pick two lines, or two arcs or circles.",
        "Elige dos líneas, o dos arcos o círculos.",
    ),
    ("Pick two lines.", "Elige dos líneas."),
    (
        "Fully defined: nothing can move.",
        "Totalmente definido: nada puede moverse.",
    ),
    (
        "{} degrees of freedom: dimension or constrain more.",
        "{} grados de libertad: añade cotas o restricciones.",
    ),
    ("Click on a point.", "Pulsa sobre un punto."),
    ("Click on an edge.", "Pulsa sobre un borde."),
    ("Drag a point.", "Arrastra un punto."),
    (
        "Click where the line starts.",
        "Pulsa donde empieza la línea.",
    ),
    (
        "Click where it ends; Escape stops.",
        "Pulsa donde acaba; Escape la detiene.",
    ),
    ("Click the centre of the arc.", "Pulsa el centro del arco."),
    (
        "Click where the arc starts.",
        "Pulsa donde empieza el arco.",
    ),
    (
        "Click towards where it ends, counter-clockwise.",
        "Pulsa hacia donde acaba, en sentido antihorario.",
    ),
    ("Click the first point.", "Pulsa el primer punto."),
    ("Click the second point.", "Pulsa el segundo punto."),
    (
        "Click an edge to delete it.",
        "Pulsa un borde para borrarlo.",
    ),
    (
        "A closed outline you dimension in a sketch, with a name on every edge: `inlet` is where the fluid comes in, `outlet` where it leaves, `symmetry` a symmetry plane and any other name a wall. Meshed with triangles by Vortia itself and run as laminar, incompressible, steady flow (simpleFoam). The sketch is kept in the case.",
        "Un contorno cerrado que acotas en un croquis, con un nombre en cada arista: `inlet` es por donde entra el fluido, `outlet` por donde sale, `symmetry` un plano de simetría y cualquier otro nombre una pared. Vortia lo malla con triángulos y lo ejecuta como flujo laminar, incompresible y estacionario (simpleFoam). El croquis se guarda en el caso.",
    ),
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
    ("Open recent", "Abrir reciente"),
    ("Save report", "Guardar informe"),
    ("Overview", "Resumen"),
    ("Sampling", "Muestreo"),
    ("Report", "Informe"),
    ("Line", "Línea"),
    ("Compare", "Comparar"),
    ("Case report", "Informe del caso"),
    (
        "Flux out of all patches",
        "Flujo de salida de todos los patches",
    ),
    ("Total", "Total"),
    (
        "Per unit density: give a density to have them in newtons.",
        "Por unidad de densidad: da una densidad para tenerlas en newtons.",
    ),
    ("Duplicate the case", "Duplicar el caso"),
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
    ("Vortia page", "Página de Vortia"),
    ("GitHub", "GitHub"),
    ("Report an issue", "Informar de un fallo"),
    (
        "Part of the Nodalia suite of open engineering tools.",
        "Parte de la suite de herramientas de ingeniería abiertas de Nodalia.",
    ),
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
    ("Radius of edge {}", "Radio del borde {}"),
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
    ("{} has no internalField.", "{} no tiene internalField."),
    ("{} has no boundaryField.", "{} no tiene boundaryField."),
    (
        "{} is not in the {} folder.",
        "{} no está en la carpeta {}.",
    ),
    (
        "The {} folder holds no fields.",
        "La carpeta {} no contiene campos.",
    ),
    ("'{}' is not a number.", "«{}» no es un número."),
    (
        "{} must keep its form, like '{}'.",
        "{} debe conservar su forma, como «{}».",
    ),
    (
        "'{}' of {} is not written the way the editor reads; edit it in the file.",
        "«{}» en {} no está escrito como lo lee el editor; edítalo en el archivo.",
    ),
    ("the file", "el archivo"),
    (
        "'{}' cannot be set for {}.",
        "«{}» no se puede aplicar a {}.",
    ),
    (
        "The block of {} is not written the way the editor reads; edit it in the file.",
        "El bloque de {} no está escrito como lo lee el editor; edítalo en el archivo.",
    ),
    (
        "The condition type of {} is decided by the mesh.",
        "El tipo de condición de {} lo decide la malla.",
    ),
    ("Could not read {}: {}", "No se pudo leer {}: {}"),
    (
        "'{}' is written over several lines; edit it in the file.",
        "«{}» está escrito en varias líneas; edítalo en el archivo.",
    ),
    (
        "'{}' has no value to replace.",
        "«{}» no tiene un valor que reemplazar.",
    ),
    ("'{}' could not be edited.", "«{}» no se pudo editar."),
    ("{} needs a scheme.", "{} necesita un esquema."),
    (
        "{}: '{}' cannot be part of a scheme.",
        "{}: «{}» no puede formar parte de un esquema.",
    ),
    (
        "'{}/{}' is not a field of the case.",
        "«{}/{}» no es un campo del caso.",
    ),
    (
        "{} has {} values, but the mesh has {} cells.",
        "{} tiene {} valores, pero la malla tiene {} celdas.",
    ),
    (
        "{} has a value on each face, not on each cell.",
        "{} tiene un valor en cada cara, no en cada celda.",
    ),
    (
        "{} cannot be shown on the mesh: {}",
        "{} no se puede mostrar en la malla: {}",
    ),
    (
        "{} is not a vector field, so it has no flow lines.",
        "{} no es un campo vectorial, así que no tiene líneas de flujo.",
    ),
    ("axis {} is not x, y or z", "el eje {} no es x, y ni z"),
    (
        "the position of the section is not a number",
        "la posición de la sección no es un número",
    ),
    (
        "the engine stopped unexpectedly; the case was left as it was",
        "el motor se detuvo de forma inesperada; el caso quedó como estaba",
    ),
    (
        "The engine could not start: {}",
        "El motor no pudo arrancar: {}",
    ),
    (
        "{} of {} checks in a row",
        "{} de {} comprobaciones seguidas",
    ),
    (
        "{} cannot change while the case runs.",
        "{} no se puede cambiar mientras el caso se ejecuta.",
    ),
    ("{} — replaced on save", "{} — se reemplaza al guardar"),
    (
        "Needs {} in the time folder.",
        "Necesita {} en la carpeta de tiempo.",
    ),
    (
        "The {} needs three numbers.",
        "El {} necesita tres números.",
    ),
    ("point", "punto"),
    (
        "The number of points is between 1 and {}.",
        "El número de puntos está entre 1 y {}.",
    ),
    (
        "Flux out of all patches: {} (zero when what comes in goes out)",
        "Flujo de salida de todos los parches: {} (cero cuando lo que entra sale)",
    ),
    ("Not worked out: {}", "No se pudo calcular: {}"),
    ("{} in cell {}", "{} en la celda {}"),
    (
        "({} {} {}) is outside the mesh",
        "({} {} {}) está fuera de la malla",
    ),
    ("{} needs a number.", "{} necesita un número."),
    ("Section at {} {}", "Sección en {} {}"),
    (
        "The report is json, csv or html, not '{}'.",
        "El informe es json, csv o html, no «{}».",
    ),
    (
        "{} is not a report this build can read.",
        "{} no es un informe que esta versión pueda leer.",
    ),
    (
        "{} faces  ·  drag to turn, right-drag to slide, wheel to zoom, double-click to reset",
        "{} caras  ·  arrastra para girar, arrastra con el botón derecho para desplazar, rueda para ampliar, doble clic para restablecer",
    ),
    (
        "  ·  the mesh is larger than the viewer shows",
        "  ·  la malla es mayor de lo que muestra el visor",
    ),
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

    /// The source of the app, one string per file, each cut where its tests begin.
    fn app_sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("i18n.rs")
                {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    let live = text.split("#[cfg(test)]").next().unwrap_or_default();
                    out.push((path.display().to_string(), live.to_owned()));
                }
            }
        }
        let mut out = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut out,
        );
        out
    }

    /// The string literal that starts at `rest` (after optional `&` and spaces), unescaped, and
    /// what follows it. `None` when `rest` does not start one or it uses an escape not handled.
    fn literal(rest: &str) -> Option<(String, &str)> {
        let rest = rest.trim_start().trim_start_matches('&').trim_start();
        let mut chars = rest.strip_prefix('"')?.char_indices();
        let mut text = String::new();
        while let Some((at, c)) = chars.next() {
            match c {
                '"' => return Some((text, rest.get(at + 2..)?)),
                '\\' => match chars.next()?.1 {
                    '"' => text.push('"'),
                    '\\' => text.push('\\'),
                    '\'' => text.push('\''),
                    'n' => text.push('\n'),
                    't' => text.push('\t'),
                    '\n' => {
                        // A line continuation swallows the next line's indentation.
                        while chars.clone().next().is_some_and(|(_, c)| c.is_whitespace()) {
                            chars.next();
                        }
                    }
                    _ => return None,
                },
                c => text.push(c),
            }
        }
        None
    }

    /// Every string literal given straight to `tr`/`trf`, with the file it is in.
    fn translated_literals() -> Vec<(String, String)> {
        let mut found = Vec::new();
        for (file, text) in app_sources() {
            for name in ["tr(", "trf("] {
                for (at, _) in text.match_indices(name) {
                    let before = text.get(..at).and_then(|t| t.chars().next_back());
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    if let Some((english, _)) = text.get(at + name.len()..).and_then(literal) {
                        found.push((file.clone(), english));
                    }
                }
            }
        }
        found
    }

    /// A line passed to `tr` that the table does not know is shown in English to everyone who
    /// runs the app in Spanish.
    #[test]
    fn every_line_given_to_tr_has_a_spanish_row() {
        let known: std::collections::HashSet<&str> = ES.iter().map(|(en, _)| *en).collect();
        let literals = translated_literals();
        assert!(
            literals.len() > 300,
            "the scan found {} lines",
            literals.len()
        );
        let missing: Vec<_> = literals
            .iter()
            .filter(|(_, en)| en.chars().any(char::is_alphabetic) && !known.contains(en.as_str()))
            .map(|(file, en)| format!("{en:?} in {file}"))
            .collect();
        assert!(
            missing.is_empty(),
            "no Spanish row for:\n{}",
            missing.join("\n")
        );
    }

    /// Text built by `format!` and handed to a widget is English in every language: it goes
    /// through `trf` with its `{}` holes instead.
    #[test]
    fn no_widget_is_given_english_prose_built_by_format() {
        let mut offenders = Vec::new();
        for (file, text) in app_sources() {
            for (at, _) in text.match_indices("format!(") {
                let Some((prose, _)) = text.get(at + "format!(".len()..).and_then(literal) else {
                    continue;
                };
                let mut bare = String::new();
                let mut depth = 0;
                for c in prose.chars() {
                    match c {
                        '{' => depth += 1,
                        '}' if depth > 0 => depth -= 1,
                        c if depth == 0 => bare.push(c),
                        _ => {}
                    }
                }
                let words = bare
                    .split(|c: char| !c.is_alphabetic() && c != '\'')
                    .filter(|w| {
                        w.chars().count() > 1
                            && w.chars().all(|c| c.is_ascii_lowercase() || c == '\'')
                    })
                    .count();
                let head = text.get(..at).unwrap_or_default().trim_end();
                let to_a_widget = [
                    "RichText::new(",
                    ".label(",
                    ".heading(",
                    ".button(",
                    "Err(",
                    "Failed(",
                    "Some(",
                ]
                .iter()
                .any(|opener| head.ends_with(opener));
                if words >= 3 && to_a_widget && !prose.contains('\n') {
                    offenders.push(format!("{prose:?} in {file}"));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "English prose built with format!, use trf:\n{}",
            offenders.join("\n")
        );
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
