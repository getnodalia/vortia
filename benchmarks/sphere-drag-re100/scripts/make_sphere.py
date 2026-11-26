#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Writes a unit-diameter sphere (an icosphere, outward normals) as an ASCII STL."""
import math
import sys

out = sys.argv[1] if len(sys.argv) > 1 else "sphere.stl"
depth = int(sys.argv[2]) if len(sys.argv) > 2 else 4  # 20 * 4**depth triangles

t = (1 + 5 ** 0.5) / 2
v = [(-1, t, 0), (1, t, 0), (-1, -t, 0), (1, -t, 0), (0, -1, t), (0, 1, t),
     (0, -1, -t), (0, 1, -t), (t, 0, -1), (t, 0, 1), (-t, 0, -1), (-t, 0, 1)]
v = [tuple(c / math.sqrt(sum(x * x for x in p)) for c in p) for p in v]
f = [(0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11), (1, 5, 9), (5, 11, 4),
     (11, 10, 2), (10, 7, 6), (7, 1, 8), (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8),
     (3, 8, 9), (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1)]
for _ in range(depth):
    cache, nf = {}, []

    def mid(a, b):
        k = (min(a, b), max(a, b))
        if k not in cache:
            p = [(v[a][i] + v[b][i]) / 2 for i in range(3)]
            n = math.sqrt(sum(x * x for x in p))
            v.append(tuple(x / n for x in p))
            cache[k] = len(v) - 1
        return cache[k]

    for a, b, c in f:
        ab, bc, ca = mid(a, b), mid(b, c), mid(c, a)
        nf += [(a, ab, ca), (b, bc, ab), (c, ca, bc), (ab, bc, ca)]
    f = nf

with open(out, "w") as o:
    o.write("solid sphere\n")
    for a, b, c in f:
        p = [[x * 0.5 for x in v[i]] for i in (a, b, c)]
        u = [p[1][i] - p[0][i] for i in range(3)]
        w = [p[2][i] - p[0][i] for i in range(3)]
        n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]]
        if sum(n[i] * p[0][i] for i in range(3)) < 0:
            p[1], p[2] = p[2], p[1]
        o.write("facet normal 0 0 0\nouter loop\n")
        for q in p:
            o.write("vertex %.9f %.9f %.9f\n" % tuple(q))
        o.write("endloop\nendfacet\n")
    o.write("endsolid sphere\n")
print(f"wrote {out}: {len(f)} triangles")
