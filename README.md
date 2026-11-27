<div align="center">
  <a href="https://getnodalia.com">
    <img src="https://raw.githubusercontent.com/getnodalia/nodalia-web/main/assets/brand/nodalia-wordmark-badge.svg" width="150" alt="Nodalia">
  </a>
  <br><br>
  <img src="resources/icons/vortia.png" width="192" alt="Vortia">
  <h1>Vortia</h1>
  <p><strong>The CFD simulation application of the Nodalia suite.</strong></p>

  <p>
    <img src="https://img.shields.io/badge/version-0.16.1-3D9BFF" alt="Version 0.16.1">
    <img src="https://img.shields.io/badge/status-experimental-orange" alt="Experimental">
    <img src="https://img.shields.io/badge/platform-macOS%20arm64%20%7C%20Linux%20x86__64-5E81AC" alt="macOS arm64 and Linux x86_64">
    <img src="https://img.shields.io/badge/Rust-2024-DEA584?logo=rust&logoColor=white" alt="Rust">
    <a href="./LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-2ea44f" alt="GPL-3.0-or-later"></a>
  </p>

  <p>
    <a href="#overview">Overview</a> ·
    <a href="#screenshots">Screenshots</a> ·
    <a href="#what-it-does">What it does</a> ·
    <a href="#installation">Installation</a> ·
    <a href="#verification">Verification</a> ·
    <a href="#building-from-source">Build</a> ·
    <a href="#project-status">Status</a> ·
    <a href="#documentation">Docs</a> ·
    <a href="#contributing">Contribute</a>
  </p>
</div>

<p align="center">
  <img src="docs/screenshots/hero.webp" width="920" alt="Vortia: a nozzle sketch in light mode behind the speed field of a jet in dark mode">
</p>

---

## Overview

**Vortia** is the CFD simulation application of [Nodalia](https://getnodalia.com): a free desktop
application, from geometry to post-processing, written entirely in Rust, with a
finite-volume CFD engine of its own derived from OpenFOAM's methods, that takes
one workflow from geometry to results: sketch or box → mesh → physics →
supervised run → results, without touching the terminal. It needs no OpenFOAM,
MPI, Python or Gmsh: the engine, the 2D mesher and the interface are Rust.

Since 0.11.0 this is the application the releases ship. The earlier C++/Qt
application and the macOS edition Vortia Native (SwiftUI), which drove OpenFOAM,
were removed from the repository once the Rust version covered everything they did;
their source stays in the git history
([`docs/architecture/rust-only.md`](docs/architecture/rust-only.md)).

Vortia is a Nodalia app maintained by one person. It does not have parity with commercial CFD suites, and it says so: every
limit is listed in
[`docs/KNOWN_LIMITATIONS.md`](docs/KNOWN_LIMITATIONS.md).

> [!IMPORTANT]
> **Vortia is experimental.** Always check convergence, mass balance and mesh
> dependence, and read [`docs/KNOWN_LIMITATIONS.md`](docs/KNOWN_LIMITATIONS.md)
> and [`docs/VALIDATION.md`](docs/VALIDATION.md) before trusting a result. The
> native cases are numerical verification, not validation of a design.

## Screenshots

| | |
|---|---|
| <img src="docs/screenshots/geometry-dark.webp" alt="2D sketch of a nozzle with its vertex table"> | <img src="docs/screenshots/results-speed-dark.webp" alt="Speed field of a jet in the results view"> |
| Geometry: a nozzle drawn with the sketch's generator | Results: speed field of a jet |
| <img src="docs/screenshots/menu-dark.webp" alt="The main menu open at the top of the side bar"> | <img src="docs/screenshots/preferences-dark.webp" alt="Preferences: appearance, language, theme and window controls"> |
| Main menu: Preferences, Keyboard Shortcuts, About | Preferences, with Nodalia or desktop window controls |

| <img src="docs/screenshots/results-airfoil-speed-dark.webp" alt="Speed field and streamlines around a NACA 2412 airfoil at 8 degrees"> | <img src="docs/screenshots/results-airfoil-pressure-dark.webp" alt="Pressure field around the same airfoil: suction above, stagnation at the nose"> |
| Results: speed and streamlines around a NACA 2412 at 8° | Results: pressure, with the suction peak over the nose |

| <img src="docs/screenshots/body-wake-dark.webp" alt="Flow around a sphere at Reynolds 100: the speed on a cut through the middle, streamlines, and the wake behind the body"> | <img src="docs/screenshots/body-wake-light.webp" alt="The same view in the light theme"> |
| A 3D body: speed on a cut through its middle with streamlines (the 3D view takes the whole window) | The same, in the light theme |

| <img src="docs/screenshots/new-case-dark.webp" alt="New case: a gallery of cards for every kind of case, grouped by what you start from"> | <img src="docs/screenshots/new-case-light.webp" alt="The same gallery in the light theme"> |
| New case: every kind of case on a card, with the CAD icons shared by Nodalia's engineering apps | The same, in the light theme |

| <img src="docs/screenshots/cad-sketch-dark.webp" alt="The region editor: ribbon of tools, Browser, canvas with named edges and constraints, properties and status bar"> | <img src="docs/screenshots/cad-part-dark.webp" alt="The part modeller: a NACA wing built on Nodalia's solid kernel, with its features on the left and the flow settings on the right"> |
| The region editor: ribbon, Browser, canvas, dimensions and degrees of freedom | The part modeller: a wing, its features and the flow around it |

The sphere pictures come from `vortia new body DIR sphere.stl model=0 reynolds=100 levels=6` and `vortia solve DIR`
(124 276 cells, converged in 197 iterations). The jet's results view shows the `examples/nozzle-practice-2026` case; the sketch is a
nozzle made with the sketch's generator. The airfoil pictures come from
`vortia new airfoil DIR naca=2412 alpha=8` and `vortia solve DIR` (about a minute on one
core: 20 480 cells, converged in 1 280 iterations, C_L 1.066, C_D 0.0141, one mesh,
no validation). `geometry-light.webp` and
`results-speed-light.webp` are the same views in light mode. These are interface
captures, not validation: see [`docs/VALIDATION.md`](docs/VALIDATION.md).

## What it does

| Area | Vortia 0.16.1 | Evidence |
|---|---|---|
| Geometry | Rectangular domains, planar and axisymmetric nozzles, a region drawn on a canvas (lines, arcs, dimensions, saved sketch); STL, OBJ, PLY, glTF, STEP (assemblies included) and DXF files read, with a report and repair of surfaces that are not watertight; a part modelled in the app on Nodalia's solid kernel (primitives, outlines, NACA wings, boolean operations, rounds and chamfers picked with the mouse on the edges, an outline drawn with the sketch editor) saved as `.vortia-part` and exported to STL and STEP; Fluent `.cas.h5` viewing | Software tests |
| Meshing | Structured block meshes with grading, checked against `blockMesh`; its own 2D mesher for a drawn region; an octree mesher for flow around a closed 3D body, with optional prism layers over the wall and one wall patch per named part (sphere at Re 100 is 1–4 % above the published drag, and the layers do not remove the excess); Fluent `.msh` and `.cas.h5` import | Compared with OpenFOAM v1912 |
| Incompressible laminar | Transient `icoFoam` (fixed or adaptive time step) and steady `simpleFoam` | Poiseuille, Ghia cavity; adaptive step against a fixed step |
| RANS turbulence | Steady k-ω SST and Spalart–Allmaras with wall functions; NACA 4-digit airfoils with C_L, C_D, C_m, C_p and C_f | Flat plate against a correlation, `simpleFoam` agreement; NACA 0012 against thin-airfoil theory (C_D 16–27 % high) |
| Heat transfer | Passive temperature in `simpleFoam`, laminar and turbulent, with wall heat flux; conjugate heat transfer for a solid body that conducts or generates heat in the stream | Heated plate against Pohlhausen (+3.3 %) and the turbulent flat-plate correlation (−13.3 %); a heated sphere against theory, not against a reference solver |
| Compressible | `rhoSimpleFoam` for nozzles and jets, natural convection of a Boussinesq fluid | Compared with OpenFOAM v1912 on the same meshes; de Vahl Davis cavity |
| Post-processing | Fields, slices, streamlines, probes, lines, wall quantities, pressure and friction coefficients on the surface (plotted and painted on the body), section analysis with HTML/CSV/JSON report, comparison between runs, CSV and VTK export | Software tests |
| Verification | Three-mesh GCI, flow-rate balance, run diagnostics | Software tests |

Not available: LES/DES, radiation, multiphase, combustion,
moving meshes, IGES import, remote or HPC runs,
the results of Fluent `.dat.h5` files, and parallel runs. The
full table, with what "evidence" means, is in the capability matrix.

## Installation

Packages are published on the [Releases page](https://github.com/getnodalia/vortia/releases).

| System | File | Requirements |
|---|---|---|
| macOS | `Vortia-<version>-macos-arm64.dmg` | Apple Silicon. Ad-hoc signed and not notarised: the first time, right-click → Open |
| Linux | `Vortia-<version>-linux-x86_64.AppImage` | x86_64, glibc 2.35 or later. Give it execute permission and open it |
| Linux | `Vortia-<version>-linux-x86_64.tar.gz` | The app and the `vortia` command-line tool, without an AppImage |

Each file has its SHA-256 sum in a `.sha256` file. The AppImage needs FUSE
(`fusermount`); without it, run it with `--appimage-extract-and-run`. The app
checks for updates once a day and from its menu.

## Command line

`vortia` makes, runs and inspects cases without the interface: `vortia new`,
`vortia solve`, `vortia probe`, `vortia sample`, `vortia patch`, `vortia flux`,
`vortia wall`, `vortia solid`, `vortia streamline`, `vortia export`. `vortia help` lists them.

## Verification

The verification cases (Poiseuille, Ghia cavity, Blasius, turbulent flat plate,
three-mesh GCI) run in `cargo test`; their tolerances are in
[`docs/VALIDATION.md`](docs/VALIDATION.md). A case that finishes is not a
verified case: only those compared against their reference within a tolerance
set beforehand count.

## Building from source

**Stack:** Rust (stable, edition 2024) · egui · `nodalia-theme`

```sh
cargo build --release --manifest-path rust/Cargo.toml -p vortia-app -p vortia-cli
cargo test --manifest-path rust/Cargo.toml
./rust/target/release/vortia-app
```

`rust/packaging/linux/package.sh` and `rust/packaging/macos/package.sh` make the
AppImage, the tarball and the DMG the releases carry.

### Application icon

`resources/icons/vortia.png` (512 px) is the application icon, so the window and the
Dock use the same design. It sits on the Nodalix app grid, like the Colloid icons around it: a
tile of 224/256 with a 16 px margin, a 51.5 px radius and a soft shadow, made
by `tools/nodalia-app-icon.py` in nodalix-icon-theme. `resources/icons/vortia-macos.png` is the same artwork at full bleed, and
`resources/icons/vortia-macos-icon.png` (1024 px) the same tile at that size, which the macOS
bundle's `Vortia.icns` and the DMG volume use because macOS does not mask app icons.

The icon is a black-and-white portrait of Daniel Bernoulli over blue, part of
the Nodalia icon family. The source is an 18th-century portrait in the public
domain (ETH-Bibliothek, Wikimedia Commons, `ETH-BIB-Bernoulli, Daniel (1700-1782)-Portrait-Portr 10971.jpg`).

## Project status

Version **0.16.1**, experimental. What does not work yet:

- [`docs/KNOWN_LIMITATIONS.md`](docs/KNOWN_LIMITATIONS.md) — engine and application restrictions
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — what comes next
- [`CHANGELOG.md`](CHANGELOG.md) — changes by version; the release notes of each version are in [`docs/release-notes/`](docs/release-notes/README.md)

## Documentation

The project documentation is written in Spanish.

- Architecture: [`docs/architecture/`](docs/architecture/)
- Technical research and licences: [`docs/architecture/research.md`](docs/architecture/research.md)
- Validation: [`docs/VALIDATION.md`](docs/VALIDATION.md)
- Fluent interoperability: [`docs/FLUENT_COMPATIBILITY.md`](docs/FLUENT_COMPATIBILITY.md)

## Contributing

Issues and pull requests are welcome. [`CONTRIBUTING.md`](CONTRIBUTING.md)
sets the rules; the three that are not negotiable:

1. **No façades.** Every feature has real, tested behaviour or is explicitly labelled as not implemented.
2. **Scientific rigour.** No CFD capability is announced without verification or validation documented in `docs/VALIDATION.md`.
3. **No fictitious data** in flows presented as real. Test doubles live only in `tests/`.

## The Nodalia suite

Vortia is one of the applications of [Nodalia](https://getnodalia.com), an independent software
project developed in public by [Daniel Miguel Tejedor](https://danielmigueltejedor.com). It belongs
to the Engineering suite, with Forja, Torno, Circuito, Ábaco and Código.

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE); the dependency compatibility
review is in `docs/architecture/research.md` §8.

## Trademark notice

Vortia is an original brand and implementation, unrelated to ANSYS, Inc.
ANSYS and Fluent are trademarks of their respective owners. OpenFOAM is a
trademark of OpenCFD Ltd; Vortia is not approved or endorsed by OpenCFD.
