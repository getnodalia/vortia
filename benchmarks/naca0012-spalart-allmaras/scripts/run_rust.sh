#!/bin/sh
# Runs the NACA 0012 sweep on Vortia's own engine and prints one line per angle.
# usage: run_rust.sh WORKDIR [medium|fine]
set -eu
work=${1:?usage: run_rust.sh WORKDIR [medium|fine]}
mesh=${2:-medium}
here=$(cd "$(dirname "$0")" && pwd)
vortia=${VORTIA:-$here/../../../rust/target/release/vortia}
case $mesh in
  medium) cells="cells-chord=60 cells-normal=32 cells-wake=20"; angles="0 2 5 8 10 12" ;;
  fine) cells=""; angles="0 5 10" ;;
  *) echo "mesh is medium or fine" >&2; exit 2 ;;
esac
mkdir -p "$work"
for alpha in $angles; do
  dir="$work/alpha$alpha"
  "$vortia" new airfoil "$dir" alpha="$alpha" $cells >/dev/null
  (cd "$dir" && "$vortia" blockmesh >/dev/null && "$vortia" solve >log 2>&1 && "$vortia" airfoil . | head -3)
done
