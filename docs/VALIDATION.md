# Validación científica

Distinguimos tres niveles (no intercambiables):

1. **Prueba de software:** unidades, parsers, escritores, procesos (tests unitarios/integración).
2. **Verificación numérica:** comparación con soluciones analíticas o benchmarks (Poiseuille, cavidad Ghia).
3. **Validación física:** comparación con experimentos (fases posteriores).

## Matriz de casos

| # | Caso | Nivel | Estado | Evidencia |
|---|---|---|---|---|
| 1 | Poiseuille (perfil analítico y fuerza de pared) | Verificación | 🧪 Nativa, cumple | `benchmarks/poiseuille-laminar-2d/`, registro 2026-11-10 |
| 2 | Cavidad tapa móvil Re=100 (Ghia et al. 1982) | Verificación | 🧪 Nativa (OpenFOAM y motor Rust), cumple | `benchmarks/lid-driven-cavity-ghia-re100/`, registros 2026-11-10 y 2026-11-18 |
| 3 | Placa plana turbulenta, k-ω SST (ley de Prandtl–Schlichting) | Plausibilidad frente a correlación | 🧪 Nativa, dentro de la banda fijada (sesgo −10 %) | `benchmarks/turbulent-flat-plate-sst/`, registro 2026-11-10 |
| 4 | Placa plana laminar (Blasius) | Verificación | 🧪 Motor Rust, cumple (sin GCI) | `benchmarks/blasius-flat-plate/`, registro 2026-11-18 |
| 5 | Canal con escalón | Verificación | ⬜ Pendiente | — |
| 6 | Perfil aerodinámico NACA 0012, Spalart–Allmaras con funciones de pared | Comparación con resultados publicados y con la teoría del perfil delgado | 🟡 Motor Rust: sustentación a 2–4 % de la referencia, resistencia sobreestimada 16–27 %, desprendimiento no reproducido, dos mallas sin GCI | `benchmarks/naca0012-spalart-allmaras/`, registro 2026-11-25 |
| 7 | Sensibilidad a la malla (GCI de tres mallas) | Verificación | 🟡 Un estudio registrado (Poiseuille, motor Rust); faltan los demás casos | `benchmarks/common/gci.py`, `vortia_post::gci`, registro 2026-11-18 |
| 8 | Tobera práctica 2026 (rhoSimpleFoam, wedge, SST) | Verificación | 🟡 Ejecutado, pendiente comparación | examples/nozzle-practice-2026 |
| 9 | Esfera que genera calor en una corriente (transferencia de calor conjugada) | Verificación (interior, analítica) y plausibilidad (película, correlación) | 🟡 Motor Rust: el interior a −2 % de la solución exacta, el Nusselt a +8 % de Ranz–Marshall; sin comparar con `chtMultiRegionFoam`, una malla fina sin GCI | `rust/vortia-cli/tests/solid.rs`, registro 2026-11-26 |

## Procedimiento por caso
Definición física → hipótesis → parámetros (con unidades) → geometría → malla → modelo → esquemas → solución de referencia → métricas de error (L2/L∞) → límites de validez. Todo bajo `benchmarks/<caso>/` con `README.md` reproducible.

## Registro
Las comprobaciones numéricas corren como pruebas de Rust (`cargo test`, workflow
`rust.yml`); un caso omitido cuenta como fallo.

Lo que estos registros **no** demuestran: independencia de malla (solo hay un
GCI registrado, el de Poiseuille), validez fuera de los casos listados, ni validación frente a
experimentos. En la primera ejecución nativa Poiseuille no cumplió (relL2 =
3,1 %) porque se comparó una solución sin converger; se corrigió el presupuesto
de iteraciones y el criterio de convergencia, no la tolerancia.

| Fecha | Caso | Qué se midió | Resultado | Notas |
|---|---|---|---|---|
| 2026-11-05 | Poiseuille / Ghia | Software: identidades analíticas + tablas de referencia + `CaseWriter` acepta casos | PASS (CTest `test_verification`) | **No** es verificación CFD frente a solver |
| 2026-11-05 | Poiseuille / Ghia | Harness write→run→sample→compare | Scripts + CTest (`SKIP` sin OpenFOAM) | Ejecutar `benchmarks/*/scripts/run_openfoam.sh` con OF en PATH para obtener PASS/FAIL real |
| 2026-11-10 | Poiseuille, malla 60×40 | Perfil u(y) en x = L/2 frente al analítico, 81 puntos | relL2 = 7,53e-4, relL∞ = 6,33e-4 (límites 2 % y 3 %). **Cumple** | OpenFOAM ESI v2412 (el motor de la AppImage), `simpleFoam`. Residuos finales Ux = 5,8e-10, p = 7,0e-7. [Ejecución CI](https://github.com/danielmigueltejedor/vortia/actions/runs/42755218855) |
| 2026-11-10 | Poiseuille, malla 60×40 | Fuerza viscosa sobre las dos paredes frente al balance exacto Δp·H·W | 9,9999999e-6 frente a 1e-5 m⁴/s²: error relativo 1e-8 (límite 2 %). **Cumple** | Función `forces` de OpenFOAM leída con `parseForces`. Misma ejecución |
| 2026-11-10 | Cavidad Re = 100, malla 48×48 | u en la línea central vertical frente a Ghia et al. (1982), tabla I, 17 puntos | relL2 = 3,47e-3, relL∞ = 3,89e-3 (límites 5 %). **Cumple** | Convergió en 465 iteraciones. La componente v (tabla II) se muestrea pero aún no se compara. Misma ejecución |
| 2026-11-10 | Placa plana turbulenta Re_L = 1e7, malla 200×60, k-ω SST con funciones de pared | Coeficiente de fricción medio frente a Prandtl–Schlichting | C_D = 0,002698 frente a 0,003004: −10,2 % (banda fijada a priori: 15 %). **Dentro de la banda, con sesgo a la baja** | Convergió en 375 iteraciones. y+ en la placa: 25–36 (media 27), en el extremo bajo del rango de funciones de pared. Una sola malla: sin estudio de independencia. Misma ejecución |
| 2026-11-13 | Placa plana turbulenta Re_L = 1e7, mismo caso, en el motor Rust (`vortia blockmesh` + `vortia solve`) | Coeficiente de fricción medio frente a Prandtl–Schlichting y frente a OpenFOAM v2606 en los mismos ficheros | C_D = 0,00269801634 frente a 0,002698016903 de OpenFOAM (2e-7) y 0,003004 de la correlación: −10,2 %. **Dentro de la banda** | Convergió en 375 iteraciones (OpenFOAM v2606: 374). y+ 25–36 (media 27), igual que OpenFOAM. Campos finales a 1,3e-3 (U), 4,8e-3 (p) y 1e-4 (k, ω, ν_t) de su máximo. Fuerzas integradas con `forces` de OpenFOAM. `benchmarks/turbulent-flat-plate-sst/scripts/run_rust.sh` |
| 2026-11-19 | Placa plana turbulenta Re_L = 1e7 (`vortia new plate-turbulent`), motor Rust frente a OpenFOAM v1912 en los mismos ficheros | Residuos de cada iteración, iteraciones de cada resolución lineal y campos finales | Misma convergencia (471 iteraciones; 481 con `upwind`), residuos de la primera iteración a 1e-14 o mejor, 2314 de 2355 resoluciones con el mismo número de iteraciones, campos finales a 1e-11 (U, k, ω, ν_t) y 9e-11 (p). **Coincide** | La diferencia de 1e-4 frente a v2606 (registro anterior) es de esa versión, no del motor. `openfoam1912` de OpenCFD en un contenedor Ubuntu 20.04, en un runner de GitHub |
| 2026-11-18 | Placa plana turbulenta Re_L = 1e7, el caso hecho por `vortia new plate-turbulent` (`vortia_cases::Plate::turbulent`), calculado y medido entero por el motor Rust, como prueba de `cargo test` | Coeficiente de fricción medio (fuerzas de `vortia-post`) frente a Prandtl–Schlichting | C_D = 0,002698 frente a 0,003004: −10,2 %, el mismo valor que OpenFOAM. **Dentro de la banda del 15 %** | Convergió en 471 iteraciones con los ajustes del caso generado en Rust. Sin OpenFOAM para las fuerzas. `rust/vortia-cli/tests/verification.rs` |
| 2026-11-14 | Cavidad cuadrada calentada lateralmente, Pr = 0,71, motor Rust (`buoyantBoussinesqSimpleFoam`) | Nusselt medio en la pared caliente (extrapolación cuadrática), velocidad máxima u de la línea vertical central y v de la horizontal central, frente a de Vahl Davis (1983) | Ra = 1e3, 20×20: Nu 1,122 (1,118: +0,4 %), u_max 3,649 (3,649), v_max 3,674 (3,697). Ra = 1e4, 32×32: Nu 2,268 (2,243: +1,1 %), u_max 16,155 (16,178), v_max 19,462 (19,617). Ra = 1e5, 64×64: Nu 4,565 (4,519: +1,0 %), u_max 34,57 (34,73), v_max 68,51 (68,59). Ra = 1e6, 80×80: Nu 9,036 (8,800: +2,7 %), u_max 65,58 (64,63), v_max 217,9 (219,36). **Cumple** las bandas fijadas en la prueba | Mallas uniformes: el Nusselt de Ra alto está limitado por la resolución de la capa límite. T centrosimétrica a 1e-6. Sin comparar aún con OpenFOAM en los mismos ficheros. `cargo test -p vortia-solvers --test buoyant` (1e3 y 1e4) y `-- --ignored` en `--release` (1e5 y 1e6) |
| 2026-11-18 | Poiseuille, motor Rust (canal de `vortia new channel`, Re = 10 sobre la altura, lectura en x = 0,9 L) | Perfil u(y) frente al exacto, con malla 80×20; y velocidad en el plano medio con tres mallas de 8, 16 y 32 filas (razón 2) y su índice de convergencia de malla | Malla 80×20: L∞ del perfil sobre el máximo 2,5e-3 (límite 3 %). Tres mallas: 1,4545, 1,4884, 1,4971; orden observado 1,96, extrapolado 1,50007 frente al exacto 1,5, GCI fino 0,25 %, razón asintótica 1,006. **Cumple** | `cargo test -p vortia-cli --test verification` (25 s en debug). El índice es `vortia_post::gci`, que reproduce `benchmarks/common/gci.py` en cuatro tripletes. Solo una cantidad en un caso 2D laminar: no demuestra independencia de malla en otros casos |
| 2026-11-18 | Cavidad Re = 100, malla 32×32, motor Rust (`vortia new cavity`, `simpleFoam` estacionario) | u en la línea central vertical (tabla I, puntos interiores) y v en la horizontal (tabla II) frente a Ghia et al. (1982), interpolando linealmente entre centros de celda y con el valor de pared en las paredes | Diferencia máxima de u 4,2e-3 y de v 8,6e-3 de la velocidad de la tapa (límite 5 %). **Cumple** | `cargo test -p vortia-cli --test verification` (17 s en debug). Se comparan las dos componentes (el registro de OpenFOAM del 2026-11-10 solo comparó u, con otra métrica y otra malla, 48×48). Con 48×48 el motor Rust da 4,5e-3 (u) y 8,4e-3 (v). Una sola malla por caso, sin GCI |
| 2026-11-18 | Placa plana laminar Re_L = 1e5, motor Rust (`vortia new plate`, `simpleFoam` estacionario, 50 celdas a lo largo de la placa, 10 delante, 80 de altura con razón 200, caja de 0,3 m) | Coeficiente de fricción medio (1,328/√Re_L), local en x = L/2 (0,664/√Re_x) y perfil u/U en x = L/2 en todas las celdas hasta η = 5 frente a la función de Blasius integrada en el propio test (Runge-Kutta, f''(0) = 0,332057) | Medio 4,26e-3 frente a 4,20e-3 (+1,4 %); local 2,94e-3 frente a 2,97e-3 (−1 %); mayor diferencia del perfil 0,65 % de la velocidad de la corriente (límites 5 %). **Cumple** | `cargo test -p vortia-cli --test verification`. La placa empieza tras un tramo de pared deslizante (`slip`, condición nueva en el motor): con el borde de ataque pegado a la entrada el coeficiente medio salía un 13 % alto, también en OpenFOAM, que da lo mismo hasta 8 cifras. En la malla por defecto (100×100, caja de 0,5 m), el esfuerzo medio en la placa es 2,1363e-3 m²/s² con el motor Rust y 2,1340e-3 con `simpleFoam` de OpenFOAM v1912 (0,1 %), 1302 y 1304 iteraciones. Una sola malla, sin GCI |
| 2026-11-25 | NACA 0012, Re = 6e6, de 0° a 12°, dos mallas de C (60×32×20 y 120×64×40), `vortia new airfoil`, motor Rust | C_L, C_D y C_m frente a la pendiente 2π de la teoría del perfil delgado y a los resultados de CFL3D con Spalart–Allmaras del NASA Turbulence Modeling Resource (α = 0°: C_D 0,00819; α = 10°: C_L 1,0909, C_D 0,01231) | α = 5°, malla media: C_L 0,537 frente a 0,548 (−2 %), dentro del 8 % fijado antes de la primera ejecución. α = 10°, malla fina: C_L 1,051 (−3,7 %), C_D 0,01561 (+27 %). α = 0°, malla fina: C_D 0,00953 (+16 %). **Sustentación en rango; resistencia sobreestimada** | Con `linearUpwind grad(U)` sin limitar las ejecuciones divergían a ángulos pequeños; el caso usa el gradiente `cellLimited`. A 15° la malla media se desprende (C_L ≈ 1,05) donde la referencia sigue pegada: por encima de unos 12° no es fiable. Sin OpenFOAM para comparar los mismos ficheros. `rust/vortia-cli/tests/verification.rs` |
| 2026-11-10 | Motor integrado | Autoprueba: malla, `checkMesh`, `simpleFoam` en serie, `decomposePar`, 2 procesos MPI, reconstrucción, `foamToVTK` | **Pasa** en Ubuntu 20.04/22.04/24.04, Debian 12/13, Fedora, Arch, openSUSE Tumbleweed, Rocky 9 (contenedores limpios), Ubuntu 22.04/24.04 (VM con FUSE) y macOS 15/26 (runners limpios) | Prueba de funcionamiento del motor, no de exactitud |

*(Solo resultados reales; prohibido registrar campos inventados como validación.)*
| 2026-11-25 | Placa plana laminar calentada, Pr = 1 | Nusselt medio Nu_L del calor que el motor da por la pared frente a Pohlhausen, Nu_L = 0,664 √Re_L (Re_L = 1e5, malla 50×80, esquema `upwind` para T) | Nu_L = 216,9 frente a 210,0: +3,3 % (límite 5 %, fijado antes de la primera ejecución). **Cumple** | Solo una malla y sin GCI; con Pr = 1 la capa térmica coincide con la de velocidad, así que la solución es la de Blasius. Prueba `the_heated_laminar_plate_matches_the_pohlhausen_solution` |
| 2026-11-25 | Placa plana turbulenta calentada (k-ω SST, funciones de pared), Pr = 0,71, Prt = 0,85 | Nu_L frente a la correlación de placa turbulenta Nu_L = 0,037 Re_L^0,8 Pr^(1/3), Re_L = 1e7 | Nu_L = 11 389 frente a 13 141: −13,3 % (límite 20 %). **Cumple** | Una correlación empírica es una comprobación de plausibilidad, no una validación; sin independencia de malla ni comparación con OpenFOAM. Ley de Prt constante, sin corrección de subcapa térmica |
| 2026-11-26 | Esfera de plástico (k = 0,2 W/mK, radio 0,5 m) que genera 300 W en una corriente de aire (Re = 200 sobre el diámetro, Pr = 0,71, ρc_p = 1 206 J/m³K), `vortia new body … solid=plastic heat=300`, motor Rust, aire de 3 445 celdas y sólido de 1 224 | (1) Calor que sale del sólido frente al generado. (2) Temperatura del centro sobre la pared: la solución exacta de una esfera con generación uniforme es `Q/(8π R k)` = 119,4 K. (3) Nusselt medio de la película, `Q/(A ΔT)·D/k_aire`, frente a Ranz–Marshall (Nu = 2 + 0,6 Re^½ Pr^⅓ = 9,57) | (1) 299,99999998 W frente a 300 (7e-11 relativo). (2) La celda más caliente está 116,95 K sobre la temperatura media de la pared: −2,0 % (banda fijada a priori: 5 %). (3) Nu = 10,30: +7,6 % (banda 25 %; Whitaker daría 8,71, +18 %). **Cumple** | La malla de 224 celdas de sólido da 103,1 K (−13,7 %): el error baja al afinar, pero no hay tres mallas para un GCI. La pared sigue la esfera en escalones, así que el área usada en (3) es la de la malla (3,35 m² frente a 3,14 de la esfera). Una correlación es una comprobación de plausibilidad, no una validación. **No** se ha comparado con `chtMultiRegionFoam`: no hay OpenFOAM en esta máquina de trabajo; queda el paso C6 original (bloque calentado en un canal, mismos ficheros) para quien lo tenga. `cargo test -p vortia-cli --test solid` (20 s en debug) |

## Comprobaciones de rendimiento del motor experimental (25/10/2026)

Se comparan copias aisladas del mismo estado de una malla de 12580 celdas,
OpenFOAM v2606 y el mismo bloque 1001–2000, Courant 0,05, tolerancias y
escritura cada 1000. La reconstrucción Minmod emparejada mantiene la función
limitadora y la aritmética originales; la comprobación opcional contrasta cada
cara con OpenFOAM, incluidas caras acopladas MPI. Los campos seriales guardados
p/T/rho/U/nuTilda coinciden a la precisión de escritura de nueve cifras.

Los ensayos MPI de 2/4/8 procesos verifican que cada rango acepta o reintenta
colectivamente los candidatos físicos. Las comparaciones iniciales de 2/4
procesos reproducen p/T/rho a la precisión guardada y U dentro de 1e-8 m/s;
nuTilda presenta diferencias locales de hasta ~3,2 % del máximo global. No
son pruebas de independencia de partición ni de convergencia RANS. El
aproximador meshWave y las operaciones distribuidas también deben estudiarse.

Pruebas de software cubren publicación atómica del último estado, conservación
del estado anterior y carpetas processor preexistentes, fallo de reconstrucción,
recuperación al cancelar, parada de descendientes de procesos y selección del
motor por contenido. El ensayo nativo opcional se activa con
VORTIA_PARALLEL_TEST_CASE y VORTIA_NATIVE_OF_ROOT; añadir
VORTIA_PARALLEL_TEST_CANCEL prueba la recuperación de una escritura completa
al cancelar. Solo deben apuntar a copias aisladas de prueba. Estas comprobaciones
no validan los resultados físicos de la práctica.

La repetición controlada en Apple M4 (4 núcleos de rendimiento y 6 de eficiencia)
midió, con /usr/bin/time y sin otras compilaciones/cálculos simultáneos:

| Motor y procesos | Segundos de reloj, 1000 iteraciones |
|---|---:|
| Anterior, 1 | 35,74 |
| Minmod emparejado, 1 | 30,65 |
| Minmod emparejado, 4 | 14,81 |
| Minmod emparejado, 8 | 13,14 |

La comparación incluye inicio del proceso y escritura de su estado final;
excluye compilación, mallado, descomposición y reconstrucción. La mejora máxima
medida de este bloque es 2,72×; no garantiza el mismo factor en otras mallas o
estados ni establece un tiempo de convergencia. El primer ensayo completo del
orquestador, con compilación del motor y reconstrucción, tardó 23,916 s.

Los monitores de cada ejecución paralela se conservan también en
postProcessing/parallel_run_<id>/; los monitores antiguos no se sobrescriben.
El registro indica la carpeta de trabajo/recuperación correspondiente.


## Esperas MPI en un escritorio compartido (25/10/2026)

Se observó que dos trabajos independientes de ocho procesos en el M4 de diez
núcleos provocaban esperas activas y una ralentización grande. Cada lanzador
cree disponer de todos los núcleos. La política local por defecto pasa a
OMPI_MCA_mpi_yield_when_idle=1 para ceder CPU únicamente en esperas MPI, según
[la documentación oficial de Open MPI](https://docs.open-mpi.org/en/main/launching-apps/scheduling.html#forcing-aggressive-or-degraded-performance-mode).
Se preserva una elección explícita del usuario y no se altera el entorno global.

Ensayo controlado con dos trabajos simultáneos de ocho procesos, 12580 celdas
cada uno, estado inicial 1000, final 1050, Courant 0,05, misma malla, binarias,
tolerancias y escritura final:

| Política de espera | Trabajo A | Trabajo B |
|---|---:|---:|
| Activa, valor 0 | 28,16 s | 28,14 s |
| Ceder CPU, valor 1 | 3,80 s | 3,82 s |

La mejora de ~7,4× solo corresponde a ese escenario con exceso de procesos;
no representa una simulación única. En el ensayo de un solo trabajo de ocho
procesos y 1000 iteraciones se midieron 13,338 s y 13,537 s respectivamente.
Los campos reconstruidos p/T/rho/U/nuTilda coinciden a la precisión guardada
de nueve cifras. Las pruebas de integración verifican la inyección del ajuste
y la conservación de una elección explícita del usuario. Ninguna de estas
mediciones demuestra convergencia física de la tobera.
