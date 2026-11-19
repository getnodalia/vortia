# Deterministic Engineering Quality Contract

Required evidence: actual check command output, new regression tests for behavioral changes, and no deliberate test skips or coverage/CI weakening. Do not mark a task complete when mandatory checks fail. Changes to this document or CI need human review.

**Machine-enforced:** patch whitespace integrity; Rust formatting and locked workspace metadata (the `quality` check). Clippy and the numerical/regression tests run in `rust.yml` (`fmt · clippy · tests`), also required.

The `quality` check must be added to required branch status checks to prevent bypass. Existing workflows are retained and may cover additional validation. Any new coverage or performance thresholds must be based on repeatable baselines, not invented numbers.
