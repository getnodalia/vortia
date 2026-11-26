# Sphere drag at Re = 100 (octree mesh, stepped and snapped wall)

| Field | Value |
|---|---|
| Level | Comparison with published results. **Not a validation**: the result misses the reference and the reason is not yet established |
| Physics | Steady incompressible laminar flow around a sphere |
| Geometry | Sphere of diameter 1 m, from `scripts/make_sphere.py` (5 120 triangles), in a tunnel from 3 D ahead to 8 D behind and 4 D to each side, lateral sides as symmetry planes |
| Mesh | `vortia new body`, octree refinement, wall stepped or snapped to the surface: surface cell D/16, D/32, D/64 (levels 3, 4, 5), wake box one level coarser |
| Reference | Schiller–Naumann correlation, C_D = 24/Re (1 + 0.15 Re^0.687) = 1.09 at Re = 100 (Clift, Grace and Weber, *Bubbles, Drops and Particles*, 1978, give 1.087) |
| Vortia API | `vortia new body DIR sphere.stl model=0 reynolds=100 levels=N`, `vortia solve`, `vortia forces DIR body -rho 1 -uref 1 -aref 0.7853981634` |

## Recorded result, stepped wall (2026-11-26, one core)

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

## Recorded result, wall snapped to the surface (2026-11-26, one core)

Each wall point is moved to the nearest point of the surface, the move is spread into the cells
around it and reduced where a cell would be squashed or skewed (`vortia-mesh::bodymesh::snap`);
about 65 % of the wall points end on the surface, the rest stay part of the way.

| Level | Surface cell | Cells | Iterations | Time | C_D | pressure | viscous |
|---|---|---|---|---|---|---|---|
| 3 | D/16 | 18 144 | 135 | 3 s | 1.1070 | 0.551 | 0.556 |
| 4 | D/32 | 61 920 | 303 | 23 s | 1.1243 | 0.544 | 0.580 |
| 5 | D/64 | 344 308 | 834 | 8 min | 1.1348 | 0.549 | 0.586 |

Snapping moves the drag from **7 % below** the reference (stepped, 1.013) to **4 % above** it
(1.135 at level 5, still rising by 0.011 between levels 4 and 5, so about 1.15 if the trend
continues). The small tunnel accounts for about 2 % of that (next section), leaving +3 %. The viscous
share is 52 % of the total; I have not checked it against a published split.

## Snapped wall in a larger tunnel (2026-11-26)

The same surface levels with the tunnel from 6 D ahead to 15 D behind and 8 D to each side
(`upstream=6 downstream=15 lateral=8`, wake one level coarser than the surface):

| Level | Cells | Iterations | C_D | pressure | viscous |
|---|---|---|---|---|---|
| 3 | 61 232 | 158 | 1.0842 | 0.538 | 0.546 |
| 4 | 105 008 | 319 | 1.1010 | 0.531 | 0.570 |
| 5 | 387 396 | 868 | 1.1113 | 0.536 | 0.575 |

So the small tunnel was worth about 2 % of the +4 % found above. The unconfined values rise by
0.017 and 0.010 between levels, so they approach about 1.12 (+3 % against 1.087), and the
viscous share stays at 52 %. Non-orthogonal correctors (0 to 3) change nothing at level 3, so
the skew of the snapped cells is not the cause.

## With prism layers (2026-11-26)

`vortia new body ... layers=N layer-growth=R [yplus=Y]` stacks N thin cells on the snapped wall
(`vortia-mesh::bodymesh::layers`): each wall point is moved out along its normal, the stack is at
most half the wall face it grows out of, and it is made thinner when a cell would turn inside
out. Without `yplus` the first layer is a tenth of the surface cell.

| Tunnel | Level | Layers | Cells | C_D plain | C_D with layers |
|---|---|---|---|---|---|
| small | 3 | 3, growth 1.3 | 21 887 | 1.1070 | 1.1191 |
| small | 3 | 5, growth 1.2, y+ = 1 | 24 383 | 1.1070 | 1.1198 |
| large | 3 | 4, growth 1.2 | 66 223 | 1.0842 | 1.0966 |
| large | 4 | 4, growth 1.2 | 124 495 | 1.1010 | 1.1076 |

The layers move the drag **up** by 0.6 to 2 %, never down, and the viscous share stays about
half. So the first-order wall gradient on a cell as thick as the boundary layer was **not** what
made the snapped sphere read high: the remaining candidates are the faceted snapped wall itself
(its area and curvature) and the solver's treatment of hanging nodes. The layered meshes are
valid (no inverted or open cells) but thin cells on a skewed wall raise the worst
non-orthogonality to about 80 degrees on the faces between neighbouring layer cells.

## What this says, and what it does not

* The mesher produces closed, solvable meshes with the right body volume, and the sphere's
  drag is within 10 % of the published value. That is a useful first number, not a validation.
* The stepped wall misses the reference by 7 % and snapping overshoots it by 4 %, so the wall
  treatment moves the answer by more than the gap: that is what the stepped wall was costing,
  and it also shows the error is not yet below a few percent either way. The remaining
  candidates are the first-order wall gradient on cells with no prism layers and the solver's
  treatment of hanging nodes; non-orthogonal correction was ruled out. A uniform-grid run (25 M
  cells at D/32) was not made. Until the remaining 3 % is explained, a drag from
  `vortia new body` is an estimate within about 3 to 5 % on this case and must not be quoted
  as a prediction.
* One Reynolds number, one body, no wake or separation-point comparison.

## Run

```bash
cargo build --release -p vortia-cli          # in rust/
./benchmarks/sphere-drag-re100/scripts/run_rust.sh /path/to/empty/dir 4
```
