#!/usr/bin/env bash
# Steady flow past a circular cylinder at Re = 40, from a DXF drawing, with the Rust engine.
#
#   scripts/run_rust.sh [LATERAL] [UPSTREAM] [DOWNSTREAM]   (tunnel sizes in diameters, default 30 30 40)
#
# The drawing is a circle of radius 10 mm in millimetres, the inlet speed is 1 m/s and the viscosity
# 5e-4 m²/s, so Re = U D / nu = 40. The drag coefficient is the force per unit density over
# 1/2 U² D t (t the 10 mm the 2D case is extruded by): the drag Vortia prints divided by 1e-4.
set -euo pipefail

lateral="${1:-30}"
upstream="${2:-30}"
downstream="${3:-40}"
vortia="${VORTIA:-vortia}"
work="$(mktemp -d)"
trap 'rm -r "$work"' EXIT

cat > "$work/cylinder.dxf" <<'DXF_END'
0
SECTION
2
HEADER
9
$INSUNITS
70
4
0
ENDSEC
0
SECTION
2
ENTITIES
0
CIRCLE
8
0
10
0
20
0
40
10
0
ENDSEC
0
EOF
DXF_END

"$vortia" new section "$work/case" "$work/cylinder.dxf" viscosity=0.0005 \
    lateral="$lateral" upstream="$upstream" downstream="$downstream"
"$vortia" solve "$work/case" | tail -n 3
"$vortia" forces "$work/case" body | awk '/^    Drag  / { printf "C_D = %.3f\n", $2 / 1.0e-4 }'
