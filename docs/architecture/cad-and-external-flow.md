# CAD in Vortia: importing and modelling a body, and the flow around it

Status: written 2026-11-26 against 0.13.0. Priority set by the project lead: an engineer must
be able to bring a 2D or 3D CAD shape into Vortia, or draw it with a CAD editor as complete
as Forja's (2D drafting) and Torno's (3D parametric solids), and then see how the air flows
around it: a car, an F1 car, a wing. This document is the plan; the progress table at the end
says what has landed. Everything stays native Rust with Vortia's own engine. It fits
[professional-roadmap.md](professional-roadmap.md) as the real-geometry item (8) pulled
forward, plus the editor.

## 1. What exists, checked in the repositories

| Piece | Where | State |
|---|---|---|
| STL reader (binary and ASCII, merged vertices, closed-surface test) | `vortia-mesh/src/stl.rs` | done; the viewer draws it, nothing meshes around it |
| 2D sketch editor (lines, arcs, dimensions, region) and 2D meshing of a drawn region | `vortia-sketch`, `vortia-mesh/tri2d` | done, planar and axisymmetric only |
| Fluent, Workbench, CGNS and Gmsh mesh import | `vortia-mesh` | done (the case, not the CAD) |
| 3D volume meshing around a surface | none | **missing**: `blockMesh` makes boxes and blocks only |
| 3D solver on an unstructured mesh | `vortia-fv`, `vortia-solvers` | the finite-volume code is general polyhedral; 3D has been exercised on block meshes only |
| 3D parametric modelling (B-rep kernel, feature timeline, booleans, fillets, sweeps) | Torno: `solidcore` (Apache-2.0, vendored in `getnodalia/torno`), `torno-doc`, `torno-engine` | about 2.7 million lines; the editor Daniel wants already exists here |
| STEP read and write, STL/OBJ/glTF/3MF/PLY import | Torno: `solidcore-step`, `torno-io` | done in Torno |
| DXF and DWG read and write, 2D drafting engine | Forja: `nodalia-dxf`, `nodalia-dwg` (getnodalia/foundation), `forja-engine` | done in Forja |

The consequence is that rewriting an editor "as complete as Torno's" inside Vortia is not a
plan: it would copy years of work and fork it. The editor is reached by **sharing code**,
and the new engineering is the part no sibling has: meshing a body for flow.

## 2. Phases

### Phase A: bring a shape in (S to M)

- One geometry model in the engine: a triangulated, closed, oriented surface with named
  parts, a unit, and a bounding box (`vortia-mesh`). STL is already read into it.
- Read OBJ and PLY directly, and glTF if wanted, with the same limits as the STL reader
  (size caps, finite coordinates, no panics on hostile files).
- STEP: tessellate through `solidcore-step` and `solidcore-tess` (Apache-2.0, compatible
  with Vortia's GPL-3.0-or-later). Open question for the spike: depend on them by git
  revision on `getnodalia/torno`, or on a shared crate moved to Foundation. No dependency on
  any repository outside `getnodalia`.
- 2D CAD: DXF through `nodalia-dxf` into the sketch editor's region, so a drawn profile or an
  imported airfoil outline becomes a 2D case.
- Repair and checks the mesher needs: closed-surface test, open-edge and non-manifold
  report, consistent orientation, scale and unit prompt, a count of triangles and parts.
- *Done when* a STEP, an STL and a DXF of the same part open in the Geometry page with the
  same size and a report that says whether it is watertight.

### Phase B: mesh the air around it (L, the core)

A body in a wind tunnel needs a volume mesh. Plan, in the order they are built:

1. **Domain and patches.** A tunnel box (or a half model with a symmetry plane, a ground
   plane for a car) sized in body lengths, with named patches: inlet, outlet, sides, ground,
   and one wall patch per part.
2. **Cartesian refinement.** An octree around the surface and its wake, refined by distance
   to the body and by surface curvature, with a 2:1 balance between neighbours.
3. **Castellate and snap.** Cells cut by the surface are kept or removed and their vertices
   moved onto the surface (the snappyHexMesh idea, written here, from the public method),
   producing a polyhedral `polyMesh` the existing solvers read as they are.
4. **Prism layers** over the walls, with a growth ratio and a first-layer height from the
   target y+, so the wall functions of k-ω SST and Spalart–Allmaras work on a real body.
5. **Quality.** Non-orthogonality, skewness and cell-volume limits checked with the existing
   `check` code, with a repair pass and a report; a mesh that fails is refused with the
   reason, not run.

*Done when* the flow around a sphere is meshed and run end to end, and its drag coefficient
agrees with the published sphere curve (Schiller–Naumann at low Re, the standard drag curve
in the Re range the model covers) to a stated tolerance, with the mesh study in
`benchmarks/`. Cars and wings come after: they stress the same code with more surface, and
nothing is claimed for them until a published case (for example NACA 0012 in 3D, the Ahmed
body) has been run and compared.

### Phase C: the editor (L, in steps)

The goal is the Forja and Torno editor inside Vortia's Geometry stage. Plan:

1. **Spike (S).** Prove that Vortia can build against `torno-doc` and `torno-engine` (the egui
   version is the same, 0.37; Torno overrides egui with a vendored fork, which a dependent
   workspace must also apply). If it cannot, the fallback is a shared `nodalia-cad` crate
   pulled out of Torno.
2. **Modeller as a library.** A `vortia-cad` crate owning a Torno design: sketches, extrude,
   revolve, loft, booleans, fillet, parameters, undo. The commands are Torno's own; Vortia
   adds none of its own geometry code.
3. **Editor UI.** The Geometry page hosts the modeller with the shared UI kit and the
   Engineering menu being aligned across Vortia, Forja and Torno. First the common tools
   (sketch, extrude, revolve, boolean, fillet, import, measure), then the rest as the UI
   kit components allow. 2D drafting arrives the same way from Forja's engine.
4. **Round trip.** "Open in Torno" and "Open in Forja" through STEP and DXF files for work
   the embedded editor does not yet do, so the engineer is never blocked.

Honest limit: parity with Torno's 400,000-line UI is reached component by component, not in
one release. Each release says which tools are in.

### Phase D: results for bodies (M)

Force and moment coefficients on a named part with a reference area and length, surface
Cp and Cf plots coloured on the body, a drag/lift breakdown by part (a car's wing against
its floor), streamlines seeded from the body, and the Cp curve for 2D sections. Much of this
exists as wall quantities and forces; it needs per-part patches and a 3D surface view.

## 3. Order of work

A first (it unblocks everything and is cheap), then B step 1 to 3 with a sphere (the first
3D body that runs), then C's spike in parallel because it decides how the editor is
built, then B step 4 to 5 and D, then C's editor in steps. Each item is its own pull
request with its tests and its benchmark; a feature release goes out per finished batch.

## 4. Rules for this work

- Import readers treat files as hostile: size caps, finite numbers, bounded recursion, no
  panics, a regression test for every crash fixed.
- A body that is not closed is reported and, where safe, repaired; it is never meshed
  silently.
- Numbers reported for a body (drag, lift, y+) come from a run that was made, on a mesh whose
  quality was checked, and state what was not done (one mesh, no mesh study, no wind-tunnel
  data) the way the benchmarks already do.
- Licences: Apache-2.0 and MIT code can be used; nothing from GPL/LGPL/AGPL tools is read or
  copied while writing the mesher (OpenFOAM's `snappyHexMesh` is described by its published
  method only).

## 5. Progress

| Date | Item | State |
|---|---|---|
| 2026-11-26 | Plan written | this document |
| 2026-11-26 | B1–B2, B5 first stage: `vortia-mesh::bodymesh` (octree, 2:1 balance, stepped wall, tunnel patches) and `vortia new body DIR FILE.stl` (laminar, Spalart–Allmaras, k-ω SST) | in PR #247; mesh quality is checked by the existing `check-mesh` |
| 2026-11-26 | Sphere at Re = 100: C_D 1.033, 1.017, 1.013 on 18 k, 62 k and 344 k cells against 1.09 published | **misses the reference by 7 %**; the gap does not close with refinement and its cause (stepped wall, hanging nodes, symmetry sides) is not yet separated. `benchmarks/sphere-drag-re100/` |
| B3 | snapping to the surface (`bodymesh::snap`); sphere Re = 100 C_D 1.107 / 1.124 / 1.135 (levels 3–5), against 1.09: the stepped −7 % became about +4 %, not closed | done, result not yet within 2 % |
| A2 | STEP read through solidcore (`vortia-cad`, git dependency on `getnodalia/torno`); cylinder test, closed surface, units honoured; `vortia new body part.step` | done for solids; no assemblies or IGES |
| A3 | DXF read through `nodalia-dxf` (`vortia-cad::dxf`): lines, arcs, circles, ellipses, bulged polylines and NURBS splines joined into closed outlines, unit from `$INSUNITS`; `vortia new region` (layers name the patches) and `vortia new section` (outlines as bodies in a tunnel); cylinder Re = 40 C_D 1.56 against 1.52–1.59 | done for 2D; no blocks, DWG or 3D DXF |
| A4 | Home > New > Around a CAD file: read an STL, OBJ, STEP or DXF, set the tunnel and the flow, create the case (`vortia-app`, `app/imported.rs`) | done; no viewer for DXF yet, no assemblies |
| next | B4 prism layers, then re-run the sphere; a uniform-grid run to separate solver from mesh (the viscous drag share, 52 %, looks high) | not started |
| 2026-11-26 | A: OBJ reader (`vortia-mesh::obj`, hostile-input limits, quads fanned, negative indices), `Stl::load` by extension, `scale=` for millimetres; the Home page and `vortia new body` open OBJ as well as STL | in review; STEP/IGES/DXF, PLY and glTF not read yet |
