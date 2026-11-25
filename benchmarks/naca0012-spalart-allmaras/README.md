# NACA 0012 airfoil (Spalart–Allmaras, wall functions)

| Field | Value |
|---|---|
| Level | Comparison with published results and thin-airfoil theory (not a validation of the mesh) |
| Physics | Steady incompressible RANS, Spalart–Allmaras, Spalding wall function |
| Geometry | NACA 0012 (closed trailing edge), chord 1 m, C-grid of six blocks, far field 20 chords, wake 20 chords |
| Conditions | U∞ = 1 m/s at the angle of attack, Re = 6 × 10⁶ on the chord, ν̃ = 4ν in the stream |
| Meshes | medium 60 × 32 × 20 (5 120 cells), fine 120 × 64 × 40 (20 480 cells); first cell at y⁺ ≈ 30 |
| References | NASA Turbulence Modeling Resource, NACA 0012 validation case, CFL3D with Spalart–Allmaras (α = 0: C_D = 0.00819; α = 10: C_L = 1.0909, C_D = 0.01231), and the thin-airfoil lift slope 2π per radian |
| Vortia API | `vortia new airfoil DIR alpha=…`, `vortia solve`, `vortia airfoil DIR` |

## Acceptance

The test `a_naca_0012_at_five_degrees_follows_thin_airfoil_theory`
(`rust/vortia-cli/tests/verification.rs`) fixed these before its first run, at 5° on the
medium mesh: C_L within 8 % of 2π·α, C_D between 0.008 and 0.016, |C_m| below 0.02 and a
suction peak below C_p = −1.

## Run

```bash
cargo build --release -p vortia-cli          # in rust/
./benchmarks/naca0012-spalart-allmaras/scripts/run_rust.sh /path/to/empty/dir medium
./benchmarks/naca0012-spalart-allmaras/scripts/run_rust.sh /path/to/empty/dir fine
```

The medium mesh takes about 10 s per angle on one core; the fine one a few minutes.

## Recorded result on the Rust engine (2026-11-25)

C_L, C_D (pressure + viscous) and C_m about the quarter chord, at the iteration where the run
stopped (residual control, or the 4 000-iteration limit at 0°, where the residuals sit at
10⁻⁵ to 10⁻⁶ and the coefficients are steady).

| α | mesh | C_L | C_D | C_m | Reference |
|---|---|---|---|---|---|
| 0° | medium | 0.0000 | 0.01032 | 0.0000 | C_D 0.00819 (+26 %) |
| 0° | fine | 0.0000 | 0.00953 | 0.0000 | C_D 0.00819 (+16 %) |
| 2° | medium | 0.2185 | 0.01048 | −0.0012 | 2πα = 0.2193 (−0.4 %) |
| 5° | medium | 0.5372 | 0.01189 | −0.0042 | 2πα = 0.5483 (−2.0 %) |
| 5° | fine | 0.5410 | 0.01079 | −0.0029 | 2πα = 0.5483 (−1.3 %) |
| 8° | medium | 0.8390 | 0.01497 | −0.0086 | 2πα = 0.8773 (−4.4 %) |
| 10° | medium | 1.0192 | 0.01856 | −0.0132 | C_L 1.0909 (−6.6 %) |
| 10° | fine | 1.0507 | 0.01561 | −0.0085 | C_L 1.0909 (−3.7 %); C_D 0.01231 (+27 %) |
| 12° | medium | 1.1621 | 0.02476 | −0.0196 | none recorded |

At 1° the medium mesh gives C_L = 0.1097 against 0.1097 of the thin-airfoil theory (3 000
iterations, residuals still falling).

## What this does and does not show

- The lift slope agrees with thin-airfoil theory and with the reference at 10° to −4 % on the
  fine mesh, and it approaches it with the mesh (−6.6 % to −3.7 %).
- **The drag is overpredicted by 16 to 27 %.** The first cell sits at y⁺ ≈ 30 on a wall
  function, and the pressure drag at the nose is resolved by a handful of cells. A
  low-Reynolds mesh (y⁺ ≈ 1) would be the way to remove that, and it is not done.
- **Stall is not reproduced.** At 15° the medium mesh separates (C_L ≈ 1.05, C_D ≈ 0.066)
  where the reference stays attached (C_L above 1.5). Treat results above about 12° as
  unreliable.
- Two meshes are not a grid-convergence study; there is no GCI here.
- No OpenFOAM comparison: the same case has not been run in `simpleFoam`.

## What the study found about the solver

`bounded Gauss linearUpwind grad(U)` with an unlimited gradient made the runs diverge at
small angles of attack (the stagnation point sits on the nose, where the mesh turns
quickly). The cases the engine writes use the limited gradient
(`limited cellLimited Gauss linear 1`, `div(phi,U) bounded Gauss linearUpwind limited`),
which converges at every angle above; first-order `upwind` also converges but adds so much
numerical diffusion that C_D reaches 0.024.
