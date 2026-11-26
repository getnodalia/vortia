# Vortia as a professional simulation tool: audit and roadmap

Status: written 2026-11-25 against 0.12.0. This is the plan of record for the work
after the move to Rust. It supersedes the ordering (not the content) of
[simulation-depth.md](simulation-depth.md), whose P1 to P10 are folded into the tiers
below, and it builds on [rust-engine.md](rust-engine.md). Every item here is native Rust
with Vortia's own engine: no OpenFOAM, Gmsh, Python or MPI at run time.

The rule of the engine stays: a feature is done when a case runs on it, matches a
reference (OpenFOAM on the same files where OpenFOAM has the solver, an analytic
solution or a published benchmark otherwise), has its numbers in the repository
(`benchmarks/`), and can be set up and looked at in the app, not only on the command line.
Nothing is reported as validated that was not run.

## 1. Where Vortia stands (0.12.0)

| Area | What exists | What a professional tool has that Vortia lacks |
|---|---|---|
| Meshing | Own `blockMesh` (multi-block hex, grading, line/arc/spline edges, merge), 2D sketch editor, planar and axisymmetric profile meshing, Gmsh/Fluent/CGNS/ANSYS import | Automatic 3D meshing from CAD or STL (cut-cell, tetra/poly), prism layers, mesh adaptation, mesh repair |
| Incompressible flow | PISO (`icoFoam`), SIMPLE (`simpleFoam`) laminar, k-ε, k-ω SST, wall functions | Adjustable time step, PIMPLE, Spalart–Allmaras, transition, LES/DES, low-Re models |
| Compressible flow | SIMPLE/SIMPLEC for a perfect gas (`rhoSimpleFoam`), density-based central-upwind (`rhoCentralFoam`, SA) | Implicit coupled density-based solver, real-gas and tabulated properties, shock-adaptive meshing |
| Heat transfer | Laminar Boussinesq natural convection (`buoyantBoussinesqSimpleFoam`), scalar transport, `mixed` thermal BCs | Thermal wall functions with a thermal sublayer, conjugate heat transfer, radiation, thermophysics library |
| Other physics | none | Species and mixing, multiphase (VOF), combustion, moving and dynamic meshes, particle tracking |
| Numerics | Gauss schemes, limited and bounded schemes, PCG, PBiCGStab, symmetric Gauss–Seidel | Multigrid (GAMG), coupled solvers, parallel runs, GPU |
| Post-processing | Probes, line sampling, streamlines, planes and slices, forces and coefficients, wall shear, y+, wall heat rate, section analysis, comparison, sealed reports, CSV and VTK export | Surface Cp and Cf plots, time-history monitors during a run, iso-surfaces, field algebra, animations |
| Workflow | Case templates, ribbon, 3D viewer, run and convergence panel, project files, reports | Parametric studies and design of experiments, optimisation, scripting API, remote and batch runs |
| Validation | Cavity (Ghia), Poiseuille, Blasius, turbulent plate, buoyant cavity (de Vahl Davis), nozzle against OpenFOAM | A published validation suite per physics, with mesh-independence studies |

The honest summary: Vortia has an exact, OpenFOAM-compatible kernel for 2D and simple 3D
flow, and a good workflow around it. What stops it being a general engineering tool is
(a) physics that engineers reach for next (turbulent heat transfer, conjugate heat
transfer, transient turbulence), (b) real-geometry meshing, (c) speed, and (d) a worked
path for the domains it targets: aerospace, thermal and fluid engineering.

## 2. Tiers

Sizes: S (days), M (one to two weeks), L (several weeks).

### Tier 1: the aerospace and heat-transfer core (now)

1. **External aerodynamics: airfoils (M).** A NACA 4-digit airfoil case with a C-grid
   made by the engine's own `blockMesh`, Spalart–Allmaras and k-ω SST, far-field
   conditions at any angle of attack, and `Cl`, `Cd`, `Cm` and the surface `Cp`
   distribution as results. Needs Spalart–Allmaras in the incompressible solver (it
   exists only in `rhoCentralFoam`). *Done when* `Cl(α)` of the NACA 0012 agrees with the
   published values to a stated tolerance and the lift slope with thin-airfoil theory.
2. **Transient incompressible flow with adjustable step (M).** `adjustTimeStep` by
   Courant number with `maxCo` and `maxDeltaT`, second-order time schemes, PIMPLE
   correctors, averaging. *Done when* the Taylor–Green vortex decays at the analytic
   rate to 1 %, and a cylinder wake reproduces the Strouhal number to 2 %.
3. **Turbulent heat transfer (M).** Temperature in turbulent incompressible flow with
   `αt = νt/Prt`, thermal wall functions, and Nusselt number and wall heat flux as
   results; forced and mixed convection. *Done when* a heated channel reproduces the
   Dittus–Boelter / Gnielinski correlation within the correlation's scatter.
4. **Conjugate heat transfer (L).** Solid and fluid regions in one case, coupled
   temperature interfaces, solid materials. *Done when* a plate fin matches the analytic
   fin solution and a heated block in a channel matches OpenFOAM's `chtMultiRegionFoam`.

### Tier 2: capability

5. **Compressible aerospace (L).** Implicit density-based solver with local time
   stepping and low-Mach preconditioning, shock-capturing validation (oblique shock,
   Sod, supersonic wedge, RAE 2822 transonic), real-gas properties by table.
6. **Turbulence beyond RANS (L).** LES (Smagorinsky, WALE), DES, k-ω SST with
   transition (γ–Reθ), low-Re near-wall treatment.
7. **Radiation and species (M each).** Surface-to-surface, discrete ordinates;
   passive scalars and gas mixtures.
8. **Real-geometry meshing (L).** Cut-cell hexahedral meshing from STL, prism layers,
   quality repair. Without it nothing above reaches CAD geometry. **Pulled forward on
   2026-11-26 by the project lead, together with CAD import and an in-app CAD editor:**
   see [cad-and-external-flow.md](cad-and-external-flow.md).
9. **Speed (L).** Threaded assembly and solves, GAMG, domain decomposition.

### Tier 3: breadth

10. **Multiphase (VOF) and dynamic meshes**, particle tracking.
11. **Studies and optimisation.** Parameter sweeps, design of experiments, adjoint-free
    optimisation, scripting.
12. **Combustion**, only on demand.

## 3. Interface

The app keeps one language: `nodalia-theme` with the Engineering suite look
(`nodalia_theme::engineering`), shared with Torno and Forja. Chrome (menus, ribbon,
panels) is owned by the Engineering-suite alignment work; this plan adds screens, not
chrome. Each new physics arrives in the app as a case template (the New case page) that
writes the settings, the boundary conditions, the schemes and the solver numbers, and shows
what it chose. Results that matter most to each domain (Cp curve, Nusselt, coefficients)
get a plot in the Results stage, not only a number in the log.

## 4. Order of work

Tier 1 runs in the order 1, 2, 3, 4: aerospace first because it needs the least new
infrastructure (the mesher already draws the C-grid), transient and heat transfer next
because they are what every later case uses, conjugate heat transfer after them because
it needs regions in the case format. Each item is its own pull request with its tests and
its benchmark, and a feature release (a minor version) goes out when a batch is complete.

## 5. Progress

| Date | Item | State |
|---|---|---|
| 2026-11-25 | Audit and this plan | written |
| 2026-11-25 | Item 1, airfoils: `vortia new airfoil`, incompressible Spalart–Allmaras, `vortia airfoil` (Cl, Cd, Cm, Cp, Cf), New case page; NACA 0012 Cl within 4 % of the reference to 10°, Cd over by 16–27 %, stall not reproduced | merged in #241 (see `benchmarks/naca0012-spalart-allmaras/`) |
| 2026-11-25 | Item 2, first part: `adjustTimeStep` (`maxCo`, `maxDeltaT`, write-time landing) in `icoFoam`; cavity to t = 0.5 in 186 steps vs 500, velocity within 0.33 % of the lid speed of a 0.001 s step. Second-order time schemes, PIMPLE, Taylor–Green and cylinder-wake validation not done | partial |
| 2026-11-25 | Item 3, first part: temperature in `simpleFoam` (`Pr`, `Prt`, `αeff = ν/Pr + νt/Prt`), wall heat flux and total in the wall report; heated plate Nu within +3.3 % (laminar, Pr = 1, Pohlhausen) and −13.3 % (turbulent, Pr = 0.71, flat-plate correlation). The Dittus–Boelter channel/pipe validation, `linearUpwind` for T and a thermal sublayer correction are not done | partial |
