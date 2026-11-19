# Agent instructions — vortia

## Repository-specific deterministic quality contract (2026-10-26)

**Project:** Rust workspace (`rust/`): CFD engine, 2D mesher and egui desktop app.

**Engineering requirements:** Protect mesh validity, dimensional correctness, incompressible flow conservation, Fluent mesh interchange and localization. Use the deterministic nozzle benchmarks and `cargo test`.

**Verification commands:** from `rust/`: cargo fmt --all -- --check; cargo clippy --all-targets --locked -- -D warnings; cargo test --locked.

Only claim checks passed after executing them. Every feature and fix should add or maintain tests for behavior, boundaries and failure cases. Never disable/weaken tests, suppress warnings or reduce coverage thresholds solely to pass CI. `AGENTS.md`, `QUALITY_GATES.md`, tests and workflows are review-sensitive. Require `quality` in branch protection. Disclose native dependencies and pre-existing CI failures. Preserve existing project rules.
