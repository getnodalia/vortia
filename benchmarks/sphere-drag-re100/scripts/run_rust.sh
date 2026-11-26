#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Runs the sphere at Re = 100 on the Rust engine at one surface level and prints its drag.
# usage: run_rust.sh EMPTY_DIR LEVEL   (LEVEL 3, 4 or 5; 5 takes about 8 minutes)
set -euo pipefail
dir=${1:?an empty folder to work in}
level=${2:-3}
here=$(cd "$(dirname "$0")" && pwd)
vortia=${VORTIA:-$here/../../../rust/target/release/vortia}
mkdir -p "$dir"
python3 "$here/make_sphere.py" "$dir/sphere.stl" 4
"$vortia" new body "$dir/case" "$dir/sphere.stl" model=0 reynolds=100 levels="$level" \
    wake-levels=$((level - 1))
"$vortia" solve "$dir/case" | grep -E "converged|Time ="  | tail -1
"$vortia" forces "$dir/case" body -rho 1 -uref 1 -aref 0.7853981634 -lref 1 | grep -E "coefficient|drag"
