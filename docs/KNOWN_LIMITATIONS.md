# Limitaciones conocidas

Actualizado: 2026-11-19 (0.11.0). Vortia es la aplicación en Rust: motor de volúmenes
finitos propio, mallador 2D propio e interfaz egui. No usa OpenFOAM, MPI, Python ni Gmsh.

## Motor

- **Sin ejecución en paralelo:** cada caso corre en un solo proceso.
- **Resultados de Fluent:** se leen las mallas (`.msh`, `.cas.h5`), pero los resultados
  `.dat.h5` no están implementados.
- **Lector `.cas.h5`:** verificado solo con un fichero de prueba generado por nosotros,
  no con un caso real de Fluent.
- **Física ausente:** transferencia de calor conjugada, LES, multifase y mallado 3D
  automático siguen sin existir (ver [`architecture/simulation-depth.md`](architecture/simulation-depth.md)).

## Aplicación

- **Sin revisión visual completa:** la interfaz se prueba con pruebas automáticas; no se
  ha inspeccionado a mano en todas las plataformas.
- **Instaladores:** AppImage y `tar.gz` para Linux x86_64 (glibc 2.35 o posterior) y DMG
  para macOS con Apple Silicon. No hay instalador para Intel, Windows ni macOS sin firmar
  que se abra sin el aviso de Gatekeeper.
- **Capturas de la web:** aún son las de la versión anterior.

Más detalle sobre las comprobaciones numéricas: [`VALIDATION.md`](VALIDATION.md).
