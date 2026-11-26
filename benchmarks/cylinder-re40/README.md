# Cylinder drag at Re = 40 (2D, from a DXF drawing)

| Field | Value |
|---|---|
| Level | Comparison with published results (steady, so the flow is symmetric and has no vortex shedding) |
| Physics | Steady incompressible laminar flow past a circular cylinder, Re = U D / ν = 40 |
| Geometry | A circle of diameter D = 20 mm in a DXF file (`scripts/run_rust.sh` writes it), set in a wind tunnel by `vortia new section`: inlet on the left, outlet on the right, symmetry planes above and below; the case is one 10 mm cell deep |
| Mesh | Triangles extruded one cell: D/40 along the cylinder, growing at 30 % to 16 times that at the tunnel's edges |
| Reference | C_D = 1.52–1.59 for an unbounded flow: Dennis and Chang (1970) 1.52, Tritton (1959) 1.59, Fornberg (1980) 1.50 |
| Vortia API | `vortia new section DIR cylinder.dxf viscosity=0.0005 lateral=L upstream=U downstream=W`, `vortia solve`, `vortia forces DIR body` |

C_D is the drag Vortia prints (a force per unit density) over ½ U² D t, with U = 1 m/s and t = 10 mm.

## Recorded result (2026-11-26, one core)

| Tunnel (D ahead, behind, to each side) | Cells | Iterations | C_D |
|---|---|---|---|
| 5, 15, 10 | 10 198 | 449 | 1.734 |
| 20, 30, 20 | 42 311 | 589 | 1.575 |
| 30, 40, 30 | 85 762 | 666 | 1.562 |

The tunnel's sides hold the flow in, so a narrow tunnel reads high: with the sides 10 D away the cylinder blocks 9 % of the tunnel and the drag is 11 % over the unbounded value. From 20 D to 30 D the value moves by 0.8 % and sits inside the published range. The mesh was not refined at a fixed tunnel, so **this is a check of the DXF-to-case chain and of the 2D solver at one resolution, not a grid-convergence study**.

## Reproduce

```sh
VORTIA=path/to/vortia scripts/run_rust.sh 30 30 40
```
