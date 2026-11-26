# Sphere drag at Re = 100 (octree mesh, stepped wall)

| Field | Value |
|---|---|
| Level | Comparison with published results. **Not a validation**: the result misses the reference and the reason is not yet established |
| Physics | Steady incompressible laminar flow around a sphere |
| Geometry | Sphere of diameter 1 m, from `scripts/make_sphere.py` (5 120 triangles), in a tunnel from 3 D ahead to 8 D behind and 4 D to each side, lateral sides as symmetry planes |
| Mesh | `vortia new body`, octree refinement, stepped wall: surface cell D/16, D/32, D/64 (levels 3, 4, 5), wake box one level coarser |
| Reference | Schiller–Naumann correlation, C_D = 24/Re (1 + 0.15 Re^0.687) = 1.09 at Re = 100 (Clift, Grace and Weber, *Bubbles, Drops and Particles*, 1978, give 1.087) |
| Vortia API | `vortia new body DIR sphere.stl model=0 reynolds=100 levels=N`, `vortia solve`, `vortia forces DIR body -rho 1 -uref 1 -aref 0.7853981634` |

## Recorded result (2026-11-26, one core)

| Level | Surface cell | Cells | Iterations | Time | C_D | pressure | viscous |
|---|---|---|---|---|---|---|---|
| 3 | D/16 | 18 144 | 136 | 3 s | 1.0328 | 0.620 | 0.413 |
| 4 | D/32 | 61 920 | 304 | 22 s | 1.0172 | 0.591 | 0.426 |
| 5 | D/64 | 344 308 | 834 | 8 min | 1.0127 | 0.583 | 0.430 |

The three values give an observed order of 1.8 and a grid-convergence index of 0.2 % between
the fine pair (`benchmarks/common/gci.py`), so the numbers are converged **on this kind of
mesh**. They converge to about 1.011, which is **7 % below** the reference.

Two other things were measured with the level 3 mesh:

* A tunnel of 6 D ahead, 15 D behind and 8 D to each side gives 1.010 instead of 1.033, so the
  small tunnel overpredicts by about 2 % (blockage) and the unconfined value is nearer 0.99,
  about 9 % below the reference.
* The wall area of the stepped surface is 55 % larger than the sphere's (4.875 against 3.142),
  and the share of the drag that is viscous is higher than the published split at this Re.

## What this says, and what it does not

* The mesher produces closed, solvable meshes with the right body volume, and the sphere's
  drag is within 10 % of the published value. That is a useful first number, not a validation.
* The gap does not close with refinement. The likely cause is the stepped wall (no snapping
  to the surface, no prism layers), but that is **not shown**: the other candidates are the
  solver's treatment of hanging nodes and of the lateral symmetry planes, and a uniform-grid
  run to separate them has not been made. Until it is, a drag from `vortia new body` is
  an estimate of the right order and must not be quoted as a prediction.
* One Reynolds number, one body, no wake or separation-point comparison.

## Run

```bash
cargo build --release -p vortia-cli          # in rust/
./benchmarks/sphere-drag-re100/scripts/run_rust.sh /path/to/empty/dir 4
```
