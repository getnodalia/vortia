# Changelog

Formato basado en [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/).

Las notas de cada versión publicada, bilingües, están en `docs/release-notes/vX.Y.Z.md`: son el
texto de la release en GitHub. Este registro sigue siendo el detalle para quien desarrolla.

## [Unreleased]

## [0.14.0] — 2026-11-26

### Documentación
- **Capturas del perfil NACA 2412 a 8°:** velocidad con líneas de corriente y presión (`docs/screenshots/results-airfoil-*.webp`), del caso que sale de `vortia new airfoil DIR naca=2412 alpha=8` y `vortia solve DIR` (20 480 celdas, converge en 1 280 iteraciones: C_L 1,066, C_D 0,0141, C_m 0,041, pico de succión C_p −3,37; una malla, sin validación).

### Añadido
- **Exportar la pieza a STEP (editor CAD):** «Guardar como STEP» escribe el sólido exacto que construye el kernel (no su malla) en un archivo ISO 10303-21 (`Part::to_step`/`Part::save_step`, escritura atómica). Las piezas están en metros y STEP declara milímetros, así que el cuerpo se escala ×1000 antes de escribirse y otro programa lo abre al mismo tamaño. Probado: un bloque de 2×1×0,5 m con un agujero se lee de nuevo con el lector STEP de Vortia con las mismas dimensiones en metros (±1 µm), el volumen exacto (±1 %) y la superficie cerrada; una pieza vacía se rechaza. La casilla de archivo sirve ahora para STL y STEP.
- **Guardar y abrir piezas, redondeos y chaflanes (editor CAD):** una pieza se guarda como `.vortia-part` (JSON con las operaciones, escritura atómica, lectura con todos los números comprobados y topes de tamaño; `Part::save`/`Part::open`) y se abre desde «Modelar una pieza». Dos operaciones nuevas, «Redondear aristas» y «Achaflanar aristas», actúan sobre lo que lleva la pieza hasta ese punto: cada arista se nombra con un punto cercano (a menos de un milímetro, con el kernel de Nodalia, `solidcore-ops::pick`) y se redondea con una bola rodante o se achaflana a igual distancia por las dos caras. Una arista no encontrada, una ambigua, un tamaño que no es una longitud o un redondeo que no cabe se rechazan con el motivo. Probado con el cubo unidad: el redondeo de 0,1 m quita (1 − π/4)·0,01 de volumen y el chaflán 0,005, ambos dentro de la tolerancia, y la superficie sigue cerrada.

### Añadido
- **«Modelar una pieza» en Nuevo caso (app, segundo paso del editor CAD):** se apilan operaciones sobre el kernel de Nodalia (caja, cilindro, cono, esfera, toro, contorno extruido, contorno girado), cada una sumada, restada o intersecada con la pieza, con sus medidas en metros y la posibilidad de subirlas, bajarlas o quitarlas. La pieza se reconstruye en un hilo cada vez que cambia y se ve en el visor 3D sin perder la vista; muestra volumen y superficie, avisa si no es cerrada y se guarda como STL (escritor binario nuevo en `vortia-mesh::stl`). Un contorno se rellena con un perfil alar NACA de 4 cifras (`vortia-cad::part::naca4`, 2412 por defecto: «Un ala» pone uno de 1 m de cuerda y 2 de envergadura) o con el contorno mayor de un dibujo DXF (`outline_from_drawing`, milímetros a metros). «Crear caso» toma la pieza como el cuerpo de un caso 3D con los mismos ajustes que «Alrededor de un archivo CAD» (la corriente viene a lo largo de x). Probado: el NACA 0012 extruido encierra el 8,2 % de su cuerda² por la envergadura (±2 %), el contorno de un círculo DXF de 10 mm da un cilindro de volumen exacto (±1 %) y cada tipo de forma se construye y se malla; no se ha visto en una ventana real (el entorno de pruebas no tiene pantalla).

### Añadido
- **Piezas modeladas en Vortia (`vortia-cad::part`, primer paso del editor CAD):** una pieza es una lista de operaciones sobre el kernel de Nodalia (el de Torno): caja, cilindro, cono, esfera, toro, polígono extruido y polígono girado, cada una sumada, restada o intersecada con lo anterior. El resultado sale como una superficie cerrada que el mallador toma como cuerpo. Comprobado con un bloque con un agujero pasante (volumen dentro del 1 % del exacto), un prisma y una tubería de revolución; las piezas imposibles (sin operaciones, restar primero, una arista de longitud cero, un resultado vacío) se rechazan con un motivo. Aún sin interfaz.

- **Ensamblajes STEP:** un archivo con varias piezas o con una pieza repetida (los pies de una máquina, los tornillos de una placa) se coloca como dice el archivo, con la estructura de producto y las transformaciones de cada instancia (`solidcore-ops::placed_bodies`), en lugar de apilar las piezas en el origen. Sigue valiendo que las piezas no se solapen para mallar alrededor. Prueba con un ensamblaje real de dos pies girados un cuarto de vuelta.

- **«Alrededor de un archivo CAD» en Nuevo caso (app):** desde Inicio se lee un STL, OBJ, STEP o DXF y se crea el caso sin pasar por la terminal. Una superficie cerrada se malla en 3D en un túnel (velocidad, Reynolds, longitud de referencia, túnel delante, detrás y a los lados, refinados en el cuerpo y en la estela, laminar, Spalart–Allmaras o k-ω SST); un dibujo DXF se malla en 2D con triángulos en un túnel dibujado alrededor (velocidad, viscosidad, túnel, tamaño de celda, espesor) y se ve en una vista previa. Una superficie abierta se rechaza con el recuento de aristas abiertas y la escala del archivo (0,001 para milímetros) se aplica al crear el caso.

### Añadido
- **Dibujos DXF en 2D (`vortia-cad::dxf`):** el lector de DXF de Nodalia (el de Forja) pasa líneas, arcos, círculos, elipses, polilíneas con sus bulges y splines NURBS a contornos cerrados en milímetros, uniendo los extremos y respetando la unidad del archivo (`$INSUNITS`; sin unidad se supone milímetros y se avisa). Dos usos: `vortia new region DIR DIBUJO.dxf` toma el dibujo como la región (la capa de cada pieza da el nombre del parche: `inlet`, `outlet`, `symmetry`, otra cosa es pared) y `vortia new section DIR PERFIL.dxf` toma los contornos cerrados como cuerpos y dibuja un túnel alrededor (`upstream`, `downstream`, `lateral` en múltiplos del cuerpo; entrada, salida, lados de simetría y parche `body`), con celdas de un cuarentavo del cuerpo que crecen hasta 16 veces en los bordes del túnel (`far-cell-size`, nuevo ajuste de las regiones dibujadas). Un contorno con más esquinas de las que cabe en un croquis (400) se simplifica lo mínimo y lo dice. No se leen bloques (`INSERT`), texto, cotas ni sombreados. Cilindro a Re = 40: C_D 1,56 en un túnel amplio, dentro del 1,52–1,59 publicado (`benchmarks/cylinder-re40/`).

### Añadido
- **Archivos STEP como superficie (`vortia-cad`):** nuevo crate que lee los sólidos de un STEP con el kernel propio de Nodalia (`solidcore-step`, `solidcore-tess`, por dependencia git de `getnodalia/torno` fijada por revisión), los malla con las caras unidas por sus aristas y los pasa a metros según la unidad del archivo. `vortia new body DIR PIEZA.step` y la página de inicio de la app lo abren como STL u OBJ. Un archivo con varios sólidos da una superficie con todos (sirve como cuerpo solo si no se solapan); no hay ensamblajes con transformaciones ni IGES/DXF todavía. Prueba con un cilindro (volumen al 1 %, unidad respetada, malla de aire que pasa `checkMesh`).
- **Esfera a Re = 100 en un túnel grande:** con 6 D aguas arriba, 15 D aguas abajo y 8 D a cada lado, el C_D es 1,084 / 1,101 / 1,111 en los niveles 3 / 4 / 5 (+3 % sobre 1,087 al extrapolar), así que el túnel pequeño explicaba unos 2 puntos del +4 % anterior; los correctores de no ortogonalidad no cambian nada. Detalle en `benchmarks/sphere-drag-re100/`.

### Añadido
- **Ajuste de la pared a la superficie (`bodymesh::snap`):** cada punto de la pared del mallador de cuerpos se lleva al punto más cercano de la superficie, el movimiento se reparte hacia el interior y se reduce (varias rondas) donde una celda se aplastaría, se daría la vuelta o se deformaría pasando de oblicuidad 3,5, 65° de no ortogonalidad, 10 % de su volumen o 2 % del área de una cara. Es el comportamiento por defecto (`BodySpec::snap`). Esfera a Re = 100: la pared escalonada daba C_D = 1,013 (−7 %); con el ajuste sale 1,107 / 1,124 / 1,135 en los niveles 3 / 4 / 5 (+4 % sobre 1,09 y aún subiendo), así que el error cambió de signo en vez de cerrarse; el reparto viscoso (52 %) es mayor que el publicado. Unos dos tercios de los puntos de pared llegan a la superficie; las capas de prisma siguen sin existir. Detalle en `benchmarks/sphere-drag-re100/`.

### Añadido
- **Archivos OBJ como superficie:** `vortia-mesh::obj` lee `v` y `f` (cuádruples y polígonos en abanico, índices negativos, líneas continuadas) con límites de tamaño, de vértices y de esquinas por cara, y un error que nombra la línea; el resto (colores, normales, grupos, materiales) se ignora. `Stl::load` elige por extensión (`.stl`, `.obj`) y `vortia new body DIR ARCHIVO.obj scale=0.001` lo pasa de milímetros a metros; la página de inicio abre OBJ además de STL. IGES, DXF, PLY y glTF siguen sin leerse.

### Añadido
- **Flujo alrededor de un cuerpo (`vortia new body DIR FICHERO.stl`), primera etapa:** un mallador octree propio (`vortia-mesh::bodymesh`) corta un túnel en cubos que se afinan hacia una superficie cerrada y su estela, con balance 2:1 y nodos colgantes cerrados, y quita los cubos del cuerpo; el caso sale con parches `body`, `inlet`, `outlet` y cuatro lados de simetría, y se resuelve con `simpleFoam` (laminar, Spalart–Allmaras o k-ω SST). Es la base para importar CAD y simular coches o alas (plan en `docs/architecture/cad-and-external-flow.md`). **Límites:** sin capas de prisma todavía, no hay lector de STEP/OBJ/DXF ni editor CAD aún (solo STL), y la esfera a Re = 100 se queda a unos 5 % del valor publicado (ver arriba). Detalle en `benchmarks/sphere-drag-re100/`.

### Corregido
- **Cinta de Resultados: los iconos vuelven y las etiquetas ya no se pisan:** los ocho botones (Resumen, Paredes, Fuerzas, Sondas, Línea, Sección, Comparar y Guardar informe) pedían iconos en volumen que la cinta no sabía dibujar y salían solo con texto. Ahora se dibujan, y la celda de una herramienta se ensancha si su etiqueta no cabe («Comparar» y «Guardar informe» se superponían). Lo segundo viene de `nodalia-theme` (nodalia-design #86).

## [0.13.1] — 2026-11-26

### Corregido
- **macOS: los botones de la ventana van sobre la barra superior de Vortia:** la ventana mantenía una barra de título del sistema encima de la barra propia. Ahora se oculta (`fullsize_content_view`, como Forja) y los botones nativos se pintan sobre el extremo izquierdo de la barra, que les deja sitio, mueve la ventana al arrastrar y la amplía con doble clic. Linux (controles NDL) y Windows (los del sistema) no cambian.

## [0.13.0] — 2026-11-25

### Añadido
- **Perfiles aerodinámicos (`vortia new airfoil`):** un NACA de 4 cifras en una malla C de seis bloques hecha por el `blockMesh` propio, con la corriente al ángulo de ataque, Spalart–Allmaras (o k-ω SST) y funciones de pared. `vortia airfoil CASO [-csv ARCHIVO]` da C_L, C_D (presión y viscosa), C_m en el cuarto de cuerda y la distribución de C_p y C_f; la página «Nuevo caso» de la app lo ofrece. NACA 0012 a Re = 6e6: C_L entre el −0,4 % y el −4,4 % de la teoría del perfil delgado hasta 8° y a −3,7 % de la referencia a 10° (malla fina), pero la resistencia sale un 16–27 % alta y por encima de unos 12° el perfil se desprende de forma que la referencia no muestra. Detalle y límites en `benchmarks/naca0012-spalart-allmaras/`.
- **Transferencia de calor en `simpleFoam` (laminar y turbulenta):** con un `T` en el tiempo inicial y `Pr` en `transportProperties` (y `Prt`, 0,85 por defecto), el solver resuelve `∇·(φT) − ∇·(α_eff ∇T) = 0` con `α_eff = ν/Pr + ν_t/Prt` tras el modelo de turbulencia; en la pared se usa el ν_t de la función de pared. Sin `Pr` el caso queda como estaba. `vortia post wall` y los resultados de la app dan el flujo de calor y su total en K m/s y K m³/s (por unidad de ρ Cp) cuando el flujo es incompresible. Placa laminar con Pr = 1: Nu_L +3,3 % sobre Pohlhausen; placa turbulenta con Pr = 0,71 (k-ω SST): −13,3 % respecto a la correlación Nu_L = 0,037 Re^0,8 Pr^(1/3). Una malla por caso, sin GCI ni comparación con OpenFOAM; el esquema de T es `upwind` y la ley es de Prt constante, sin corrección de subcapa térmica (no vale para Pr ≫ 1).
- **Paso de tiempo adaptativo en `icoFoam` (`adjustTimeStep yes`, `maxCo`, `maxDeltaT`):** el paso sigue al número de Courant con las reglas de OpenFOAM (crece como mucho un 20 % por paso, nunca pasa de `maxDeltaT`) y, con `writeControl adjustableRunTime`, aterriza en cada instante de escritura y en el final. En la cavidad del tutorial (20 × 20, t = 0,5, `maxCo` 0,5) hace 186 pasos con Courant máximo 0,502 y su campo de velocidad difiere como mucho 0,0033 (un 0,33 % de la velocidad de la tapa) del de un paso fijo de 0,001 s (500 pasos). Con paso fijo el resultado no cambia: los casos que coinciden con OpenFOAM siguen pasando. Solo `icoFoam`; `simpleFoam` es estacionario y los demás solvers transitorios siguen con paso fijo.
- **Spalart–Allmaras en `simpleFoam`:** el modelo que ya existía en `rhoCentralFoam` funciona también en el solver incompresible estacionario, con `nutUSpaldingWallFunction`. En la placa turbulenta da un C_D un 5,6 % por encima de Prandtl–Schlichting (k-ω SST: −10,2 %).
- **Abrir un proyecto de ANSYS Workbench (`.wbpj`):** «Abrir una geometría» acepta un `.wbpj`. Vortia lee su índice de archivos y abre, de la carpeta `<nombre>_files` que Workbench guarda a su lado, el caso resuelto (`.cas.h5` de la celda Solution), si no el de Setup, si no la malla (`.msh.h5` o `.msh`), y dice si hay resultados (`.dat.h5`), que aún no se muestran. Un `.wbpj` no contiene la malla ni el caso: si falta esa carpeta, el mensaje la nombra y lista los archivos que espera. Comprobado con el índice de un proyecto real de Workbench 2024 R2; la carpeta de ese proyecto no estaba disponible, así que la lectura del caso sigue comprobada solo con el `.cas.h5` del propio proyecto.
- **Mallas de Fluent tradicionales (`.msh`) en «Abrir una geometría»:** el mismo conversor que `vortia fluenttofoam`; una malla binaria se rechaza con un mensaje que lo dice.

### Cambiado
- **Iconos NDL de ingeniería en la cinta:** Ejecutar, Pausar, Parar, Abrir, Cerrar, Malla, Condiciones, Fluido, Turbulencia, Viscosidad, Ajustes, Numéricos, Esquemas y Sondas se dibujan con los iconos en volumen de NDL, los mismos que Torno y Forja. Los iconos nuevos (caso, malla, condiciones, fluido, turbulencia, viscosidad, esquemas, sondas, resultados, residuos) viven en nodalia-design. El árbol del Browser conserva sus iconos planos pequeños.

## [0.11.0] — 2026-11-19

### Añadido
- **Ver un caso de Fluent (R8):** «Abrir una geometría» acepta también un `.cas.h5` o `.msh.h5`:
  se lee con el lector HDF5 propio, se convierte con el mismo código que un `.msh` y se muestra
  con sus celdas, caras, puntos, contornos y la superficie para girarla. Está comprobado con un
  archivo hecho en este proyecto, no con uno real de Fluent; si un archivo real nombra distinto un
  conjunto de datos, el error dice cuál falta y qué contiene su grupo. Aún no lee los resultados
  (`.dat.h5`).
- **Lector HDF5 propio (R8):** el crate `vortia-h5` lee archivos HDF5 sin libhdf5, la base para
  ver los `.cas.h5` y `.dat.h5` de Fluent: grupos, conjuntos de datos contiguos, compactos y por
  bloques (con deflate y shuffle, cualquier orden de bytes) y atributos. Lo que no sabe leer
  (grupos densos, índices de bloques de la versión 4) devuelve un error, y un archivo dañado
  nunca provoca un fallo. Aún falta la capa de Fluent y el visor en la app.
- **Abrir un croquis guardado (R2):** «Región dibujada» tiene un campo y un botón «Abrir croquis»
  que cargan en el lienzo un croquis (el `sketch.vortia-sketch` que guarda cada caso, por ejemplo).
  Si el archivo no se lee o no se puede resolver, se rechaza y el dibujo queda como estaba.
- **Actualizador integrado en la app Rust (R8):** el menú tiene «Buscar actualizaciones…» y la app
  comprueba una vez al día, poco después de arrancar, si hay versión nueva. Usa el flujo común de
  Nodalia (`nodalia-update`): descarga con comprobación SHA-256, sustituye el AppImage o la app de
  macOS y no toca las instalaciones gestionadas (Flatpak, .deb…), donde abre la página de la release.
- **Dibujar la región sobre el lienzo (R2):** «Región dibujada» tiene ahora un lienzo con
  herramientas para mover puntos, trazar líneas y arcos, añadir cotas y borrar bordes, con
  rejilla y imán a los puntos. Se puede empezar desde un lienzo en blanco; el contorno se
  cierra al volver al primer punto y cada cambio se resuelve de nuevo o se rechaza dejando el
  croquis como estaba.
- **Comparar informes en la app (R6):** la pestaña Resultados tiene una tarjeta «Comparar» que
  lista los informes guardados en el caso (Sección y Fuerzas) o añadidos desde un archivo, deja
  elegir la referencia y el número, muestra el cambio frente a ella, avisa cuando las definiciones
  difieren y guarda un CSV en `postProcessing/comparison/`. Es lo mismo que `vortia compare`
  (`vortia_post::compare`); los informes de otro tipo se dejan fuera y un archivo que no es un
  informe íntegro se rechaza diciendo por qué.
- **Regiones cerradas dibujadas, con bordes con nombre (R2/R3):** un boceto puede ser un contorno
  cerrado (más contornos interiores como huecos) cuyos bordes llevan nombre de parche
  (`Sketch::name`, guardado en el archivo del boceto). `vortia_cases::Drawn` lo malla con
  triángulos y lo corre como flujo laminar incompresible (`simpleFoam`): `inlet` entra con la
  velocidad del caso perpendicular al borde, `outlet` va a presión cero, `symmetry` es plano de
  simetría y cualquier otro nombre es pared, un parche por nombre. `vortia sketch template
  channel-with-block FILE` y `vortia new region DIR FILE` lo prueban. Comparado con OpenFOAM
  v1912 sobre el mismo caso (canal con bloque): `checkMesh` da «Mesh OK», ambos convergen en 110
  iteraciones y la presión y la velocidad coinciden a 8 cifras; un test comprueba que el caudal
  que entra es el que sale y que la pared del bloque no deja pasar nada. En la app, «Nuevo caso
  ▸ Región dibujada» parte de un canal con un bloque, deja acotarlo, poner nombre a cada arista
  (se dibujan con el color de lo que hacen), elegir el tamaño de los triángulos y crear el caso;
  si falta `inlet` o `outlet` lo dice antes de escribir nada y el croquis se guarda con el caso.
  Aún falta trazar líneas y arcos nuevos sobre el lienzo.
- **Mallador 2D propio, no estructurado (R3):** `vortia_mesh::tri2d` convierte una región (lazos
  cerrados de segmentos con nombre de parche y, si quieren, tamaño propio) en triángulos de
  Delaunay con refinamiento de Ruppert (ángulo mínimo de 20°, tamaño que crece con la distancia a
  la frontera) y los extruye a prismas de una celda de grosor, planos (`frontAndBack` vacío) o en
  cuña de 5° con el eje colapsado (`wedgeFront` y `wedgeBack`); rechaza fronteras que se cruzan,
  lazos abiertos, números que no lo son y tamaños que darían demasiadas celdas. La tobera lo usa
  con `cell-size=MM` (`vortia new nozzle DIR cell-size=5`, también con `half=1`, `nozzle-wedge` y
  `new sketch`) y Home lo ofrece como «Triángulos (no estructurada)» con su tamaño de celda, para
  las toberas y para el boceto. Comparado con OpenFOAM v1912 (`checkMesh` da «Mesh OK» y
  `rhoSimpleFoam` sobre la misma malla da el mismo caudal de salida a 1e-5 en la media tobera y a
  6 cifras en la cuña); un test compara el caudal con la malla de bloques (0,2 % en la media
  sección, 1 % en la cuña).
- **Chorro a la atmósfera (R4):** `vortia_cases::Jet`, «Jet into the atmosphere» en Home y
  `vortia new jet DIR` escriben el caso de `examples/nozzle-practice-2026` a partir de sus números
  (orificio de 0,1 mm en una pared, cuña de 5°, atmósfera abierta con presión ambiente; `rhoSimpleFoam`,
  k-ω SST, `totalPressure`, `pressureInletVelocity` y `pressureInletOutletVelocity`). Un test lo
  resuelve en `cargo test`: velocidad en el orificio a menos del 5 % de la isentrópica (169 m/s,
  Mach 0,50) y lo que entra por el orificio sale por la atmósfera (1e-3).
- **Media sección de la tobera con plano de simetría (R4):** el motor compresible acepta
  `symmetry` y `symmetryPlane` también en T, k, ω, νt y αt, y el flujo de `HbyA` en un plano de
  simetría o deslizante ya no lleva componente normal (sin ello la media sección divergía). La
  tobera plana de Home y de `vortia new nozzle DIR half=1` puede ser la mitad superior con el
  plano en el eje; un test la compara con la ranura completa (mismo caudal de salida ×2 y mismos
  U, T y p en un punto, a 1e-3) y comprueba que por el plano no pasa nada.
- **Informe de problemas y vista de STL (R8):** «Informar de un problema» (menú, buscador de
  órdenes) abre el formulario de incidencias de GitHub con tu texto, la versión, el sistema y el
  solver; nunca una ruta ni el nombre del caso, y un texto demasiado largo para la dirección se
  queda en el portapapeles. Home abre un STL (`vortia-app --stl FICHERO`): `vortia_mesh::stl`
  lo lee binario o ASCII (el tamaño decide, rechaza lo hostil) y la app muestra tamaño, área,
  volumen si está cerrado y las aristas abiertas, con la superficie para girar con el ratón.
- **DMG de macOS de la app Rust (R9):** `rust/packaging/macos/package.sh` monta `Vortia.app` (la app
  y la herramienta `vortia`, firmada ad hoc y sin notarizar) en la ventana de instalación de
  siempre, y `rust-packages.yml` la construye en `macos-15` y la guarda como artefacto.
- **Interfaz en español e inglés (R8):** la app Rust habla el idioma del sistema y Preferencias
  tiene un selector (sistema, English, Español) que cambia los textos al momento. Los textos están
  en `vortia-app/src/i18n.rs` (el inglés es la clave y lo que falta se queda en inglés); un test
  comprueba que cada línea de la tabla sigue existiendo en el código. Los mensajes del motor y
  de los ficheros siguen en inglés, y la barra de menús que dibuja el escritorio (menú global)
  conserva el idioma con el que arrancó hasta el siguiente inicio.
- **Placa plana turbulenta:** `vortia new plate-turbulent` y la casilla «Turbulent» de la placa en Home
  escriben el caso de `benchmarks/turbulent-flat-plate-sst` (k-ω SST con funciones de pared,
  Re = 10⁷). El motor Rust lo resuelve en `cargo test` y su coeficiente de fricción queda dentro de
  la banda del 15 % de Prandtl–Schlichting (−10,2 %, igual que OpenFOAM).
- **Boceto acotado y malla desde el boceto (solo línea de órdenes por ahora):** el crate
  `vortia-sketch` guarda un perfil de pared hecho de puntos, líneas y arcos con restricciones,
  lo resuelve y lo convierte en una malla estructurada propia (sin Gmsh). `vortia sketch template
  duct|nozzle|rounded-nozzle ARCHIVO`, `vortia sketch info ARCHIVO` y `vortia new sketch CARPETA
  ARCHIVO [planar|axisymmetric]` crean el caso. En la app, «Nuevo > From sketch» parte de una
  plantilla, edita sus cotas (una cota que no se puede resolver se rechaza y el boceto queda como
  estaba), dibuja la pared y guarda `sketch.vortia-sketch` en el caso. Dibujar líneas nuevas y
  arrastrar puntos llega después.
- **Tipo de condición de contorno como lista:** en Condiciones, el tipo de cada parche que no
  decide la malla se elige de una lista de lo que el motor sabe hacer (`fixedValue`, `noSlip`,
  `zeroGradient`, y `slip` y `symmetry` en `icoFoam` y `simpleFoam`; `fixedValue`, `zeroGradient` y
  `fixedGradient` en escalares). Al guardar, el bloque del parche se escribe de nuevo con lo que
  pide el tipo nuevo (`value`, `gradient`) y el resto del archivo no cambia.
- **Pausar y reanudar una ejecución:** un botón «Pause/Resume» en la cinta, en los menús Archivo y
  Caso y en el buscador de comandos detiene el motor al inicio de su siguiente paso, sin dejar
  ningún paso a medias, y lo deja seguir cuando se reanuda. Detener funciona también con la
  ejecución en pausa. La barra de estado dice «Paused».
- **Fuerzas y coeficientes sobre paredes, informes sellados y comparación:** `vortia forces CASE
  PARED[,PARED…] [-rho R] [-uref U] [-aref A] [-lref L]` integra presión y esfuerzo viscoso sobre las
  paredes y da arrastre, sustentación, momento y sus coeficientes (en casos cinemáticos, fuerza por
  unidad de densidad; un gas exige densidad de referencia). Los informes JSON (sección y fuerzas)
  se escriben ahora sellados: sobre su contenido canónico se guarda un SHA-256, que
  `vortia compare NÚMERO BASE INFORME…` comprueba antes de comparar y se niega a mezclar informes de definiciones
  distintas. La pestaña Resultados tiene una tarjeta de fuerzas. Frente a Blasius, el
  coeficiente de arrastre de la placa coincide con la fricción media.
- **Placa plana como caso nuevo y condición `slip`:** `vortia new plate DIR [length= upstream=
  height= cells-x= cells-up= cells-y= grading= speed= viscosity=]` y, en Home, «New › Flat plate»
  escriben la placa plana laminar (Re = 10⁵ sobre la placa por defecto) con un tramo de pared
  deslizante delante, para que el borde de ataque quede dentro de la corriente. El motor
  incompresible acepta `slip`, `symmetry` y `symmetryPlane` sobre planos normales a un eje
  (`directionMixed` con la normal fija en cero). Frente a Blasius, en `cargo test -p vortia-cli
  --test verification`: coeficiente de fricción medio +1,4 %, local −1 %, y perfil de velocidad
  a 0,65 % de la corriente (límite 5 %); con `simpleFoam` de OpenFOAM v1912, el esfuerzo medio
  de la pared coincide al 0,1 %.
- **Cavidad con tapa móvil como caso nuevo:** `vortia new cavity DIR [side= cells= lid-speed=
  viscosity=]` y, en Home, «New › Cavity» escriben la cavidad cuadrada cerrada con la tapa
  deslizante (Re = 100 por defecto, el caso de Ghia et al., `simpleFoam`). La malla de bloques sale
  idéntica a la de `blockMesh` de OpenFOAM v1912. Comparada con las tablas I y II de Ghia et al.
  (1982) en una malla de 32×32, el motor Rust da diferencias máximas de 4,2e-3 (u) y 8,6e-3 (v) de
  la velocidad de la tapa (límite fijado: 5 %), en `cargo test -p vortia-cli --test verification`.
- **Verificación con el motor Rust y GCI:** `cargo test -p vortia-cli --test verification` crea un
  canal con `vortia-cases`, lo resuelve con `vortia-solvers` y compara el perfil con el exacto de
  Poiseuille (L∞ 2,5e-3 del máximo, límite 3 %); con tres mallas (8, 16 y 32 filas) el orden
  observado es 1,96, el valor extrapolado del plano medio 1,50007 frente al exacto 1,5 y el índice
  de convergencia de malla fino 0,25 %. El cálculo del índice es `vortia_post::gci`, que
  reproduce `benchmarks/common/gci.py`.
- **Listas de modelos y aviso de divergencia en la aplicación Rust:** en Turbulence y Fluid, el
  tipo de simulación, el modelo RAS (kOmegaSST, kEpsilon, SpalartAllmaras) y la energía
  (sensibleInternalEnergy, sensibleEnthalpy) se eligen de una lista con lo que el motor tiene, y
  junto al modelo se dice qué campos necesita en la carpeta de tiempo. Una palabra que el caso ya
  tenga y no esté en la lista se conserva. La tarjeta de residuos avisa mientras corre la
  simulación si un residuo deja de ser un número o si, tras haber bajado, vuelve a 1 o por encima
  (mil veces su valor más bajo, o más de 10⁶): qué ecuación, en qué paso, y qué revisar.
- **Crear casos en la aplicación Rust:** en Home, «New» (o `File › New case`) crea un caso nuevo
  sin salir de la aplicación: un canal 2D laminar (`simpleFoam`), una tobera plana (rendija entre
  dos paredes) o una tobera axisimétrica (cuña de 5°), ambas compresibles con k-ω SST
  (`rhoSimpleFoam`). Se eligen las medidas y las condiciones y se escribe la carpeta completa:
  `blockMeshDict`, la malla hecha por el `blockMesh` del motor propio, los campos iniciales y los
  ajustes. En la línea de comandos, `vortia new channel|nozzle|nozzle-wedge CARPETA [NOMBRE=VALOR...]`.
  Las mallas de bloques de las toberas son las del Qt (perfil de suavizado entre entrada, garganta
  y salida) hechas en Rust, y salen idénticas a las de `blockMesh` y `checkMesh` de OpenFOAM v1912
  (mismas celdas, puntos y caras, «Mesh OK») para los tres casos. La tobera plana es la rendija
  entera y no la mitad con plano de simetría porque el motor compresible aún no tiene condición de
  simetría para la temperatura y la turbulencia. Las toberas entran con velocidad fija (20 m/s por
  defecto): con presión total de entrada y el fluido en reposo, tanto el motor como `rhoSimpleFoam`
  de OpenFOAM divergen en las primeras iteraciones (temperatura negativa).
- **Balance de masa de la corrida:** la pestaña Run muestra lo que entra frente a lo que sale por
  cada contorno, a partir del flujo `phi` que escribe el solver: media y peor caso en porcentaje
  sobre los últimos 10 tiempos escritos, y el neto del último (el mismo control que hace el
  Qt con sus toberas). En la línea de comandos, `vortia balance CASO [VENTANA]`.
- **Residuos de la corrida en la aplicación Rust:** la pestaña Run dibuja, en escala logarítmica,
  el residuo inicial de cada ecuación (Ux, Uy, p, T, k, omega…) paso a paso, leído del registro del
  solver mientras corre. La pestaña pasa a desplazarse entera para que el registro siga a mano.
- **Análisis de sección y informe en la aplicación Rust:** en Results, la tarjeta «Section» corta el
  dominio con un plano perpendicular a x, y o z y da, integrando de forma exacta los campos
  constantes por celda, el área, el gasto másico con signo (y por separado el directo y el
  inverso), el gasto volumétrico, la presión y la densidad medias por área, la velocidad normal
  media, el flujo de cantidad de movimiento, el término de presión, la resultante normal, la
  potencia cinética y los coeficientes. Admite densidad del campo o constante, presión cinemática
  (p/ρ), valores de referencia y cuña axisimétrica (≤ 5°, eje x). Se guarda como JSON (con
  SHA-256), CSV o HTML en `postProcessing/section/<tiempo>/`. La resultante es el flujo de cantidad
  de movimiento más la presión a través de la sección, no un empuje de motor. En la línea de
  comandos, `vortia section CASO [opciones]`. En la tobera plana de prueba el gasto de la sección
  intermedia queda a menos de un 0,4 % del de entrada y salida.
- **Física del caso en la aplicación Rust:** en Setup, «Fluid», «Turbulence» y «Viscosity» editan
  `thermophysicalProperties`, `turbulenceProperties` y `transportProperties` de `constant/`
  (modelo de turbulencia, propiedades del gas, viscosidad) sin tocar el resto del archivo; cada
  vista aparece solo si el caso tiene ese archivo. Se abren también con `--fluid`,
  `--turbulence` y `--viscosity`.

## [0.10.0] — 2026-11-15

### Cambiado
- **Motor propio por defecto:** los casos de tobera se calculan con el motor
  propio de Vortia, sin OpenFOAM, a menos que se desmarque en Preferencias ›
  Cálculo. Corre en un proceso: si se piden varios (el valor automático lo
  hace), ya no falla, se ejecuta en uno. Quien ya había elegido una opción
  la conserva.

### Añadido
- **Exportar resultados:** `vortia export CASO csv|vtu ARCHIVO [TIEMPO [CAMPO...]]` escribe todas
  las celdas como CSV (número, centro, volumen y los campos) o la malla con sus campos como
  malla no estructurada VTK (`.vtu`, ASCII) para ParaView: cada celda es un poliedro de sus
  caras, con valores por celda tal como los tiene el solver. Sin campos, escribe todos los
  campos de volumen escalares y vectoriales. Probado con un lector XML y con la estructura
  del formato; no abierto aún en ParaView.
- **Cortes en el visor:** con un campo elegido, la casilla «Slice» corta la malla por un plano
  (eje X, Y o Z y un deslizador): quita la mitad que queda entre el plano y quien mira (la
  mitad que se quita cambia sola si se gira la vista) y colorea con el campo las celdas que
  el plano atraviesa. En la biblioteca, `Case::slice(origen, normal)` da el polígono de
  cada celda cortada. Se calcula en otro hilo, de uno en uno mientras se arrastra el plano.
  Aún no hay planos oblicuos ni elegidos por el usuario.
- **Líneas de corriente:** `vortia streamline CASO CAMPO X Y Z [-backward] [TIEMPO]`
  sigue una partícula por un campo vectorial (cada celda con su velocidad, de
  cara a cara) y la escribe como CSV (x, y, z, velocidad). Para en la frontera,
  en un punto de estancamiento o al llegar al límite de puntos, y lo dice. En
  la biblioteca, `Case::streamline` (una) y `Case::streamlines` (varias, leyendo el
  campo una vez). En la pestaña Malla, con un campo vectorial elegido, aparecen las
  casillas «Streamlines» y «Arrows»: líneas por una rejilla de semillas sobre los dos
  lados largos de la malla y una flecha por semilla, coloreada por la velocidad.
  Se calculan en otro hilo; las semillas son una rejilla uniforme (el chorro fino
  de una tobera queda poco muestreado).
- **Motor propio, convección natural:** nuevo solver `buoyantBoussinesqSimpleFoam`
  (laminar, estacionario, fluido de Boussinesq con `p_rgh`, gravedad en
  `constant/g` y `hRef`, `fixedFluxPressure` en las paredes). Frente a la
  cavidad cuadrada calentada lateralmente de de Vahl Davis (Pr = 0,71): el
  número de Nusselt medio sale 1,122 (referencia 1,118) en Ra = 1e3 y 2,268
  (2,243) en Ra = 1e4, con la velocidad máxima de la línea central a 0,1 % de la
  referencia; en Ra = 1e5 y 1e6 (prueba lenta, `--ignored`) el Nusselt queda a
  1,0 % y 2,7 % en malla uniforme. Aún sin comparar con OpenFOAM en los mismos
  ficheros, sin turbulencia ni transitorio, y sin pantalla en la app.
- **Motor propio, condiciones de calor:** el contorno `mixed` de OpenFOAM
  (`refValue`, `refGradient`, `valueFraction`) ya se resuelve en las ecuaciones
  escalares: con un `valueFraction` entre 0 y 1 es una pared que cede calor al
  ambiente por un coeficiente de película, y con 0 y un `refGradient` un flujo
  de calor fijo. Comprobado en la cavidad a Ra = 1, donde el campo es la
  solución analítica de conducción (película y flujo fijo, a 2e-3).
- **App en Rust:** la ventana sigue ahora la suite de ingeniería de Nodalia
  (la de Forja y Torno): barra de título con el icono, Inicio, una cinta con
  los pasos del flujo (Case, Mesh, Setup, Results), el navegador con el árbol
  del caso (malla y contornos, campos, solver), las propiedades del caso o del
  contorno elegido, y una barra de estado con el estado del cálculo y sus
  residuos. Ctrl+K (o el campo de la barra) abre la búsqueda de comandos.
- **App en Rust:** usa la tipografía de ingeniería compartida con Forja y
  Torno (IBM Plex Sans y Mono), que llega desde el tema de Nodalia.
- **Motor propio, paredes:** `vortia wall` da el esfuerzo cortante, la y+, las
  fuerzas de presión y viscosas y el calor que pasa por un contorno de pared,
  y la pestaña Results los muestra en una tarjeta Walls (el esfuerzo es el del
  gradiente en la primera celda; sin comparar aún con `postProcess`).
- **Motor propio, mirar los resultados:** el comando `vortia` lee un caso ya
  calculado y da el valor en un punto (`probe`), a lo largo de una línea en
  CSV (`sample`), la media, la integral y el rango sobre un contorno
  (`patch`), el caudal que lo atraviesa (`flux`) y la media sobre todo el
  dominio (`volume`).
- **App en Rust:** la pestaña Results muestra un caso calculado: el rango de
  un campo en el dominio y en cada contorno con el caudal que lo atraviesa,
  una sonda en un punto y un perfil a lo largo de una línea, que se dibuja y
  se guarda como CSV en `postProcessing/line`.
- **App en Rust:** edita el caso desde sus pestañas Conditions, Numerics,
  Schemes y Settings (condiciones de contorno, números del solver, esquemas
  y ajustes de ejecución).

## [0.9.3] — 2026-11-13

### Corregido
- **Linux:** los botones de la ventana vuelven a la esquina superior derecha.
  Cuando los menús se quedan en la ventana, ocupan la fila de arriba, y los
  botones suben con ellos en lugar de quedarse en la fila de las pestañas.
- **Linux:** si la barra superior del escritorio ya muestra los menús de
  Vortia, como en Nodalix, la ventana deja de repetirlos en su propia fila.

## [0.9.2] — 2026-11-13

La 0.9.0 y la 0.9.1 no llegaron a publicarse: con la bienvenida abierta, los
botones de la ventana se salían de la barra superior en Linux, y la 0.9.0
además fallaba al salir en macOS. Esta versión las sustituye.

### Añadido
- **Bienvenida:** sin proyecto abierto, Vortia muestra Nuevo proyecto, Abrir
  proyecto y los proyectos recientes, como Vortia Native. Vuelve al cerrar el
  último proyecto. Un proyecto abierto desde ella ocupa el lugar del proyecto
  sin título, en lugar de abrirse en otra pestaña.
- **Vortia Native se actualiza sola:** busca versiones nuevas una vez al día y
  desde Vortia Native ▸ Buscar actualizaciones…, descarga su propia imagen de
  disco, comprueba la suma SHA-256 publicada y la firma, y se reinicia con la
  versión nueva. Si la nueva no arranca, vuelve la anterior. Para llegar a esta
  versión hay que descargarla una última vez a mano.

## [0.8.0] — 2026-11-13

Vortia Native se parece por fin a Vortia: las mismas seis etapas, un editor de
geometría de verdad y los resultados a toda la ventana. La aplicación Qt no
cambia.

### Cambiado
- **Vortia Native** (macOS): la barra lateral sigue las etapas de Vortia,
  Proyecto, Geometría, Malla, Física, Cálculo y Resultados, con la etapa en el
  centro y sus ajustes en el inspector. Cada etapa dice cuál es el paso
  siguiente, y el proyecto muestra qué etapas están hechas.
- **Geometría:** un editor del contorno con herramientas para seleccionar,
  añadir puntos y desplazar, zoom, deshacer y rehacer, cotas, coordenadas bajo
  el cursor y un inspector para el vértice o el segmento elegido (posición,
  longitud, curvatura, dividir, borrar). La tobera paramétrica es el punto de
  partida de un proyecto nuevo.
- **Malla y física:** se dibujan sobre el contorno; en física se elige cada
  contorno con un clic y los colores de la leyenda dicen qué es cada uno.
- **Resultados:** el campo ocupa la ventana, con su nombre y unidades, cortes,
  reflejo sobre el eje, sonda, perfil a lo largo del eje y guardar imagen.
- **Bienvenida y menús:** proyectos recientes en la bienvenida y en Archivo, y
  menús Etapas (⌘1–⌘6) y Simulación (Mallar ⌘B, Calcular ⌘R, Detener ⌘.).

## [0.7.1] — 2026-11-13

La actualización desde la app vuelve a funcionar en macOS, y en Linux los menús
tienen su propia fila.

### Corregido
- **Actualización en macOS:** fallaba siempre con «firma de código no válida»,
  porque la comprobación rechazaba los metadatos del Finder que lleva todo
  archivo de un disco de instalación. Ahora comprueba la firma sin ese modo
  estricto y el error dice por qué falla.
- **Actualización en macOS:** desde la 0.7.0 podía descargar el disco de Vortia
  Native en lugar del de Vortia. Ahora toma siempre el suyo.
- **Actualización en Linux y macOS:** ofrece la versión más reciente que tenga
  paquete para tu sistema, y nunca un archivo `.zsync` en lugar del AppImage.

### Cambiado
- **Menús en Linux:** Archivo, Simulación, Herramientas, Vista, Resultados y
  Ayuda van en su propia fila encima de las pestañas de proyecto. Si la barra
  superior del escritorio muestra los menús, esa fila desaparece.

### Nota
- En macOS, el botón de actualizar de la 0.7.0 es justo el que fallaba:
  descarga la 0.7.1 a mano una vez. Desde la 0.7.1 la app se actualiza sola.

## [0.7.0] — 2026-11-12

Vortia Native para macOS, el motor de casos sin interfaz y el motor propio en
Rust con importación de mallas. La aplicación Qt y sus resultados de cálculo no
cambian.

### Añadido
- **Vortia Native** (macOS): edición en SwiftUI, cliente de `vortia-engine`.
  Las etapas de un caso en la barra lateral, comprobaciones en vivo en el
  inspector, malla, cálculo y parada en la barra de herramientas, residuales con
  Swift Charts y el resultado como corte coloreado con sus estadísticas y su
  perfil central. Liquid Glass en macOS 26.
- **Resultado en 3D** en Vortia Native: la superficie exterior del dominio,
  coloreada con el campo elegido y dibujada con SceneKit, con órbita y zoom
  desde el trackpad (conmutador Corte / 3D).
- **Flujo compresible con contorno** en Vortia Native: contorno del fluido
  sobre el eje con segmentos coloreados por su tipo de frontera, vértices
  arrastrables, arcos, tobera paramétrica a partir de cinco dimensiones;
  configuración del flujo (tamaños de malla, gas, una frontera por segmento,
  estado inicial, criterios de parada) y tarjeta de convergencia.
- **Análisis de secciones e informe de ingeniería** en Vortia Native: dónde corta
  la sección, el sentido hacia delante, los valores de referencia, las catorce
  métricas y el informe guardado como HTML, CSV o JSON.
- **`vortia-engine`:** el flujo de un caso sin interfaz (proyectos, validación,
  escritor y ejecutor del caso de OpenFOAM con residuales en vivo, análisis de
  resultados VTK, croquis, toberas y flujo compresible), con protocolo JSON por
  línea en stdin/stdout.
- **Segundo instalador de macOS:** `Vortia-<versión>-native-macos-arm64.dmg`
  junto al DMG de Qt, con la ventana de instalación de las apps de Nodalia.
- **Motor propio en Rust** (todavía no integrado en la aplicación): solucionadores
  `laplacianFoam`, `scalarTransportFoam`, `icoFoam`, `simpleFoam` (laminar, k-ε y
  k-ω SST) y `rhoSimpleFoam` con k-ω SST compresible y condiciones de presión;
  `blockMesh` propio (graduaciones, aristas curvas, celdas colapsadas, vértices
  con nombre y zonas de celdas) e importación de mallas con `vortia gmshtofoam`,
  `fluenttofoam` y `cgnstofoam` (CGNS en formato ADF, como lo exportan Fluent y
  ANSYS Meshing). Cada paso se valida contra OpenFOAM v1912 sobre los mismos
  archivos ([`docs/architecture/rust-engine.md`](docs/architecture/rust-engine.md)).

### Cambiado
- Los bocetos `.vsketch.json` se leen y escriben fuera del editor, de modo que
  la aplicación Qt y el motor comparten el formato.
- Compila con VTK 9.6, que no tiene `SetRenameArrays`.
- La integración continua corre en runners Blacksmith y añade CodeQL y OpenSSF
  Scorecard.

## [0.6.0] — 2026-11-11

Interfaz con la identidad de Nodalia. No cambia el motor ni los resultados de
cálculo.

### Añadido
- **Islas de Nodalia:** barra lateral, área de trabajo e inspector son paneles
  redondeados sobre el marco de la ventana, con títulos en Sora, iconos en
  negrita y la etapa actual invertida.
- **Menú principal** en la cabecera de la barra lateral, como en las demás apps
  de Nodalia: Preferencias (Ctrl+,), Atajos de teclado (Ctrl+?), Acerca de
  Vortia y Salir. Sin una barra superior que reciba los menús, también lleva los
  menús de la aplicación.
- **Ventana de atajos de teclado**, generada a partir de las acciones reales y
  de las etapas (Alt+1…6).
- **Menús en la barra superior de Nodalix** en Wayland, exportados por D-Bus; en
  ese caso la ventana no repite la barra de menús.
- **Controles de ventana configurables:** Preferencias › Apariencia › Controles
  de ventana › Nodalia o Del sistema (GNOME, KDE…). Se aplica al reiniciar.
- **Acerca de** con el estilo de Nodalia: icono, nombre en Sora, versión y enlaces.

### Cambiado
- Los diálogos y avisos no usan la barra de título del sistema: cabecera de
  Nodalia con el título y el botón de cierre. «Proyecto sin guardar» ofrece
  «Descartar» y «Cancelar» en vez de Sí/No.
- Botones de ventana circulares de 28 px con tinte al pasar el puntero (cian,
  ámbar, coral) y nuevos glifos de maximizar y restaurar.
- El inspector es una sola isla con barra de título propia; acoplado no se
  arrastra y en modo ventana se mueve por su título.
- Avisos (actualización, CFD experimental) como islas con icono, sin la línea de
  caja.
- Menús, listas desplegables y tooltips con esquinas redondeadas; el visor 3D
  respeta las esquinas de su isla.
- Los botones que ya muestran su nombre no lo repiten en un tooltip.
- El icono de la app tiene el mismo tamaño que el de las demás apps en Nodalix
  y GNOME.

### Corregido
- Las islas no dejaban margen con el borde derecho de la ventana.
- Las flechas de los campos numéricos se salían del campo al pasar el puntero.
- El nombre de la app en la barra superior salía como «dev.vortia.vortia».
- Las pestañas del inspector se cortaban.

## [0.5.0] — 2026-11-10

Rediseño visual. No cambia el motor ni los resultados de cálculo.

### Cambiado
- **Nueva interfaz con el Nodalia Design Language (NDL)**, el lenguaje de
  diseño común de las aplicaciones de Nodalia, en lugar del estilo propio
  anterior. Acento cian, neutros fríos, controles rectos de 6 px de radio en
  vez de píldoras, pestañas subrayadas, barra lateral hundida y selección como
  tinte suave. Claro y oscuro. Ver `docs/DESIGN.md`.
- **Los 55 iconos de herramientas se han redibujado** con una sola norma de
  rejilla, grosor y terminaciones. Ver `docs/UI_ICONS.md`.
- Los campos de valor ya no ocupan todo el ancho del panel.

### Corregido
- Las casillas sin marcar no mostraban su recuadro.
- El texto de los campos numéricos quedaba pegado al borde izquierdo.
- En modo oscuro, las flechas de desplegables y campos numéricos se dibujaban
  en negro y no se veían.
- El panel de herramientas del croquis mostraba un fondo distinto al del panel
  que lo contiene.
- La barra lateral quedaba unos píxeles más baja y más corta que los paneles
  de su derecha; ahora empieza y termina a su misma altura.
- El visor 3D usaba un fondo fijo ajeno al tema; ahora toma la superficie
  hundida y el color de texto del tema.

### Para desarrollo
- La densidad «Compacta» de Preferencias ahora usa la densidad compacta de NDL
  (controles de 24 px). `VORTIA_DENSITY=compact|regular|comfortable` la
  cambia para una ejecución. Los tokens y la hoja base de `resources/ndl/` se generan en
  `nodalia-design` y se copian sin editar.

## [0.4.1] — 2026-11-10

### Corregido
- **La actualización desde la propia aplicación no se instalaba en macOS**:
  descargaba la versión nueva, se quedaba «cargando» y al reabrir seguía la
  antigua. El cierre para reiniciar podía quedar cancelado (por ejemplo, con
  un proyecto sin guardar) y el ayudante esperaba indefinidamente, sin dejar
  registro. Ahora Vortia avisa antes si hay proyectos sin guardar, se cierra
  de forma no cancelable, el ayudante deja registro, restaura la versión
  anterior si algo falla y el motivo se muestra al volver a abrir.
  **Quien tenga 0.4.0 o anterior debe instalar esta versión a mano una vez.**
- **DMG de macOS**: al abrirse la ventana más ancha quedaba una franja blanca,
  los nombres de los iconos no se leían sobre el fondo oscuro y había que
  redimensionar la ventana para verlo todo. El fondo se ha redibujado en el
  mismo estilo, más grande y nítido en Retina, con una placa clara tras cada
  nombre y una flecha entre los iconos; la ventana es más alta y sin barra
  lateral.

## [0.4.0] — 2026-11-10

Interfaz rediseñada y coherente.

### Corregido
- **La fila de herramientas aparecía y desaparecía según la etapa**: Geometría
  y Física no tenían, y al cambiar de etapa la navegación lateral y el área de
  trabajo subían y bajaban. Ahora todas las etapas tienen su fila, con la
  misma altura, y nada se mueve.
- En Linux no se veían las etiquetas «Coordenada X/Y» del editor de croquis.

### Añadido
- Herramientas propias de **Geometría** en la fila superior: deshacer, rehacer,
  copiar, pegar, eliminar vértice, archivos de croquis y borrar croquis (ya no
  están repartidas entre el panel lateral y un botón suelto bajo el lienzo).
  **Física** tiene accesos a Modelos y Contornos.
- **Lienzo**, lenguaje de diseño exportable (variante Papel): tokens en
  `resources/lienzo/tokens.json`, hoja de estilos Qt con marcadores y variables
  CSS generadas. Guía en `docs/LIENZO.md`.
- **Linux: una sola barra superior.** Los controles de ventana, el menú y las
  pestañas de documento comparten la barra de la aplicación; ya no hay barra de
  título del sistema ni barra de menús aparte. El lado y los botones siguen la
  configuración del escritorio (GNOME, KDE). `VORTIA_NATIVE_TITLEBAR=1` vuelve
  a la barra del sistema.

### Cambiado
- Tema unificado: un solo aspecto de «seleccionado», acento azul, paneles como
  tarjetas, botones en píldora. El esquema claro pasa a ser el predeterminado
  en instalaciones nuevas; quien tuviera elegido el oscuro lo conserva.

## [0.3.0] — 2026-11-10

Instaladores que abren en una máquina limpia, con el motor CFD dentro.

### Corregido
- **Linux: la AppImage no abría en la mayoría de distribuciones.** Causas
  reproducidas en contenedores limpios con la 0.2.1: `AppRun` era un enlace
  simbólico sin rutas de plugins de Qt; faltaban librerías que linuxdeploy
  excluye (`libharfbuzz`, freetype, fontconfig, `libstdc++`…) y que el Qt de
  conda-forge necesita más nuevas que las de Ubuntu 22.04/24.04 o Debian 12;
  el ejecutable exigía glibc 2.38; y `libOpenGL.so.0` no viene instalada en
  Ubuntu/Debian. Ahora la AppImage lleva todas sus dependencias, elige en el
  arranque el `libstdc++` más nuevo entre el suyo y el del sistema, y se
  prueba en Ubuntu 20.04/22.04/24.04, Debian 12/13, Fedora, Arch, openSUSE
  Tumbleweed y Rocky 9 antes de publicarse.
- **macOS: el DMG 0.2.1 declaraba macOS 13 pero exigía macOS 27** (Qt/VTK de
  Homebrew). El DMG se construye ahora en CI con dependencias de conda-forge y
  se prueba en runners limpios.
- Los casos estacionarios (`simpleFoam`) escritos por Vortia abortaban en la
  primera iteración con OpenFOAM real: faltaba un esquema en `fvSchemes`.
- El `controlDict` estacionario ignoraba el número de iteraciones del caso.
- Los programas que lanza Vortia (OpenFOAM, Python) ya no heredan las
  librerías empaquetadas de la aplicación.

### Añadido
- **OpenFOAM integrado** en ambos instaladores (v2606 en macOS, v2412 en
  Linux, con Open MPI) y **mallador integrado** (Python 3.12 + Gmsh 4.15).
  `--verify-engine` ejecuta un caso real en serie y en paralelo;
  `--verify-mesher` genera una malla; `--verify-startup` abre la ventana
  principal y el visor.
- Modelo de turbulencia **k-ω SST** estacionario con funciones de pared,
  seleccionable en el panel del solucionador.
- **Fuerzas de pared y coeficientes** C_D, C_L y C_p (biblioteca y herramienta
  `force_report`).
- **Verificación nativa en CI** con el motor empaquetado: Poiseuille (perfil y
  fuerza de pared), cavidad de Ghia Re = 100 y placa plana turbulenta.
- Si no hay OpenGL 3.2, Vortia reintenta con renderizado por software y, si
  tampoco, lo explica en un diálogo. Un arranque fallido desde el escritorio
  deja un registro en `~/.local/state/vortia/startup.log`.
- Icono nuevo, con original vectorial en `resources/icons/vortia.svg`.
- [`docs/CAPABILITY_MATRIX.md`](docs/CAPABILITY_MATRIX.md): qué hace Vortia y
  qué no frente a un solver industrial.

### Notas
- Sin paridad con ANSYS Fluent; ver la matriz de capacidades.
- macOS: firma ad-hoc, sin notarización. Requiere macOS 14+ en Apple Silicon.
- Linux: glibc 2.31+. El flujo compresible experimental no está disponible en
  la AppImage (ver `docs/KNOWN_LIMITATIONS.md`).
- El DMG pasa de ~55 MB a ~400 MB y la AppImage de ~100 MB a varios cientos
  por el motor integrado.

## [0.2.1] — 2026-11-06

### Corregido
- Actualizador in-app en macOS: las builds públicas van firmadas **ad-hoc** (`TeamIdentifier=not set`). 0.1.1/0.2.0 rechazaban ese caso tras descargar el DMG, así que la app no se sustituía y al reiniciar seguía pidiendo actualizar. Ahora se acepta ad-hoc → ad-hoc con el mismo `Identifier` (`dev.vortia.app`); Developer ID sigue exigiendo el mismo Team ID.

### Notas
- Quien esté en **0.1.1** o **0.2.0** debe instalar este DMG **a mano** una vez (el binario antiguo sigue con el fallo). A partir de 0.2.1 las actualizaciones in-app macOS vuelven a aplicar.

## [0.2.0] — 2026-11-06

Fundamentos de verificación y validación (milestones A–B del plan 0.2.0). No se afirman resultados CFD nativos de OpenFOAM hasta registrarlos bajo `benchmarks/*/results/`.

### Añadido
- Plan técnico `docs/VORTIA_0.2.0.md` y módulo `vortia::verification` (métricas L2/L∞, Poiseuille, Ghia Re=100, Blasius clásico, CSV/interpolación).
- Árbol `benchmarks/` (Poiseuille 2D, cavidad Ghia Re=100, placa plana Blasius) con referencias citadas.
- CLI `benchmark_write_case`, `benchmarks/common/compare_profiles.py` y harness CTest (smoke de escritura; runs OF con `SKIP_RETURN_CODE` si no hay solver).
- GCI de tres mallas (Roache/Celik): `computeGci` (C++) y `benchmarks/common/gci.py`, con rechazo de oscilatorios y de órdenes no positivos.
- Valoración de balance de flujo volumétrico inlet/outlet (`assessVolumeFluxBalance`).

### Notas
- Empaquetado macOS DMG + Linux AppImage como en 0.1.1 (ad-hoc; sin notarización Apple).
- OpenFOAM sigue siendo motor externo; no va embebido en los instaladores.
- Hitos C–F del plan 0.2.0 (fuerzas/CL·CD, endurecimiento de workflow, rendimiento, RC completo) se continúan en **0.3.0**.

## [0.1.1] — 2026-11-05

### Añadido
- Actualización in-app desde GitHub Releases (banner, Ayuda → Buscar actualizaciones, preferencia al arrancar).
- AppImage Linux (`Vortia-*-linux-x86_64.AppImage`) vía GitHub Actions.
- DMG macOS con artwork de instalador.

### Corregido
- Hallazgos SonarCloud de packaging (pin de acción micromamba, curl solo HTTPS).

## [0.1.0] — 2026-11-05

### Añadido
- Estructura inicial del repositorio y documentación fundacional.
- Stack implementado: C++20, Qt 6.12, VTK 9.7, OpenFOAM (ESI primero), CMake, GoogleTest.
- Núcleo: unidades dimensionales, `Expected<T>`, logger, ejecutor de procesos seguro, formato de proyecto `.vortia`.
- Geometría: importador STL ASCII/binario, dominio rectangular.
- Malla: generador de `blockMeshDict`.
- Física: material, condiciones de contorno, validación, serialización JSON del setup.
- Solver OpenFOAM: detección, tabla de capacidades, captura de entorno, escritor de casos, parser de logs, orquestador de ejecución.
- Visualización: viewport VTK embebido en Qt (STL, contorno de dominio, ejes, tema).
- Aplicación Qt: ventana principal, explorador de proyecto, panel de propiedades, consola, temas, i18n scaffold, preferencias.
- CI: GitHub Actions (Linux core + sanitizers, macOS con UI).
- 101 tests unitarios/integración + test de orquestación con backend falso.

### Notas
- La validación numérica real (cavidad Ghia Re=100) requiere OpenFOAM instalado y está pendiente en macOS (sin binarios nativos).
