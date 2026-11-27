# Compatibilidad ANSYS Fluent — Vortia

Matriz viva. Una capacidad solo se marca como soportada tras verificarla con
archivos reales (ver `docs/VALIDATION.md`).

## Formatos

| Formato | Extensión | Detección | Importación | Conversión a OpenFOAM | Visualización VTK | Estado |
|---|---|---|---|---|---|---|
| Malla tradicional | `.msh` | cabecera + estructura | ✅ parser ASCII | ✅ conversor poliédrico (tet/hex/pyramid/wedge) | parcial | P0 |
| Malla CFF | `.msh.h5` | magic HDF5 | ⬜ P1 | ⬜ P1 | ✅ vía vtkFLUENTCFFReader | P1 |
| Caso tradicional | `.cas` | cabecera | ⬜ P1 | ⬜ P1 | ⬜ | P1 |
| Caso CFF | `.cas.h5` | magic HDF5 | ✅ metadatos + geometría | ⬜ P1 | ✅ vtkFLUENTCFFReader | P0 (viz) |
| Resultados tradicionales | `.dat` | cabecera | ⬜ P1 | n/a | ⬜ | P1 |
| Resultados CFF | `.dat.h5` | magic HDF5 | ✅ campos | n/a | ✅ vtkFLUENTCFFReader | P0 (viz) |
| Comprimido | `.gz` | magic gzip | detección | ⬜ | ⬜ | P2 |
| Journal | `.jou` | extensión | ⬜ P2 | n/a | n/a | P2 |
| Workbench | `.wbpj` | XML del índice | ✅ abre el caso o la malla de `<nombre>_files` (resuelto > Setup > malla) | según el archivo | según el archivo | P1 |
| Workbench archivado / Fluent | `.wbpz` / `.flprj` | extensión | ⬜ P2 | n/a | n/a | P2 |

## Exportación

| Formato | Comando | Estado |
|---|---|---|
| Malla tradicional `.msh` (ASCII) | `vortia foamtofluent [-type PATCH=TYPE]... FILE [CASE]` | ✅ 3D (tet/hex/pirámide/cuña/poliedro) y 2D (extrusión recta en z); ida y vuelta con el lector propio verificada. **No abierto todavía en Fluent.** Tipos de contorno: `wall` y `symmetry` por el tipo del parche; el resto por el nombre (`inlet`) o `-type`, avisando de cada suposición. Sin parches periódicos, procesador ni cuña. |
| Caso `.cas`/`.dat`, `.msh.h5`, CGNS, VTK/EnSight | — | ⬜ pendiente |

## Herramientas disponibles en esta máquina (macOS ARM64)

- `fluentMeshToFoam`, `fluent3DMeshToFoam`, `checkMesh`, `blockMesh`: **no disponibles**
  (OpenFOAM no es nativo en macOS). La conversión directa del MVP usa un
  escritor propio de `polyMesh`; la validación con `checkMesh` queda pendiente
  de un backend Linux/WSL2/Docker.
- `vtkFLUENTCFFReader` (VTK 9.7): disponible ✅
- HDF5 2.2.0: disponible ✅

## Niveles de compatibilidad reportados

- `fully-compatible`: malla tet/hex/pyramid/wedge sin zonas no mapeables.
- `partially-compatible`: hay celdas no soportadas o zonas sin mapeo directo.
- `visualization-only`: solo se puede visualizar (CFF .cas.h5/.dat.h5).
- `needs-configuration`: falta completar parámetros antes de ejecutar.
- `incompatible`: no se puede usar.

## Limitaciones conocidas (MVP)

- Solo se convierten celdas tetraédricas, hexaédricas, piramidales y de cuña.
- Las condiciones de contorno se mapean a tipos de parche (`wall`, `patch`,
  `symmetry`, `empty`); los **valores** no se transfieren.
- No se infiere ningún modelo de turbulencia ni propiedad física.
- La conversión CFF → caso ejecutable de OpenFOAM no está soportada (P1).
- No se ha validado con archivos reales de ANSYS Fluent en esta máquina.
