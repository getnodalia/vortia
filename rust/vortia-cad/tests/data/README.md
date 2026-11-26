`cylinder-mm.step` is the synthetic closed-spline cylinder (radius 1 mm, height 2 mm) from
Torno's solidcore STEP fixtures (`getnodalia/torno`, `solidcore/crates/solidcore-step/tests/fixtures/closed-spline-cylinder.step`,
MIT OR Apache-2.0, written for that project, no third-party model). It is used unchanged.

`assembly-feet.step` is `assembly_extract.stp` from Torno's solidcore tests (`getnodalia/torno`,
`solidcore/crates/solidcore-ops/tests/data/`, MIT OR Apache-2.0): an extract of an exported
assembly with its names made neutral, a root holding two instances of a foot, each made of a
washer and an angle bracket. One foot stands at the origin of the root and the other is turned
a quarter turn about Z and moved to (52.1, -662.9) mm. It is used unchanged.
