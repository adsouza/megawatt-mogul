"""Offline region geography from Natural Earth GeoJSON.

Prints RON `land: [...]` and `exclude: [...]` polygon lists for a region file, as `(lon, lat)`
pairs with 3 decimals, after a provenance comment. Run with `python3 -I` and pass every input as
a path argument: the downloaded data is untrusted and is read with `json` only.

    python3 -I tools/region_authoring/natural_earth.py \\
        --admin0 target/natural-earth/ne_50m_admin_0_countries.geojson \\
        --admin1 target/natural-earth/ne_10m_admin_1_states_provinces.geojson \\
        --include ISO:ESP,ISO:MAR --include-admin1 "RUS:Moskva" --exclude-admin1 "UKR:Crimea" \\
        --bounds=W,E,S,N --grid WIDTH,HEIGHT [--tolerance-deg T]

Write `--bounds=...` with `=`: argparse would read a leading `-20,...` as an option.

Selectors:
- `ISO:<A3>` picks admin-0 features whose `ISO_A3` is the code, or, when none has it, whose
  `ISO_A3_EH` is (France and Norway carry `-99` in `ISO_A3`). `ADM0:<A3>` matches `ADM0_A3`
  (Kosovo, Northern Cyprus).
- `<A3>:<name>` picks the admin-1 feature with that `name` or `name_en` whose Natural Earth
  `adm0_a3` is the code, or whose ISO 3166-2 country is that code's ISO country. So `UKR:Crimea`
  finds Crimea although the dataset files it under `RUS`.

Each ring is clipped to the bounds, simplified with Douglas-Peucker at the tolerance (half a cell
by default) and rounded. Land rings under two cells' area are dropped; exclusions never are.
`--dissolve` first merges rings that share borders, so separately simplified neighbours cannot
open a sliver between them. `--extent` prints only the exclusions, as a standalone
`(polygons: [...])` file; `--comment` adds header lines. Outer rings of included
features become `land`. Their holes become `exclude`, unless another included feature fills the
hole (an enclave). Outer rings of `--exclude-admin1` features become `exclude`.
"""

import argparse
import collections
import hashlib
import json
import pathlib
import shlex
import sys

RELEASE = "v5.1.2"
SOURCE = "nvkelso/natural-earth-vector"
POINTS_PER_LINE = 5


class SelectionError(ValueError):
    """A selector matched no feature."""


def douglas_peucker(points, tolerance):
    """Simplify an open polyline. The endpoints are always kept."""
    if len(points) < 3:
        return list(points)
    keep = [False] * len(points)
    keep[0] = keep[-1] = True
    stack = [(0, len(points) - 1)]
    while stack:
        first, last = stack.pop()
        index, distance = -1, tolerance
        for i in range(first + 1, last):
            d = segment_distance(points[i], points[first], points[last])
            if d > distance:
                index, distance = i, d
        if index >= 0:
            keep[index] = True
            stack.append((first, index))
            stack.append((index, last))
    return [p for p, k in zip(points, keep) if k]


def segment_distance(p, a, b):
    (px, py), (ax, ay), (bx, by) = p, a, b
    dx, dy = bx - ax, by - ay
    length2 = dx * dx + dy * dy
    if length2 == 0.0:
        return ((px - ax) ** 2 + (py - ay) ** 2) ** 0.5
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / length2))
    cx, cy = ax + t * dx, ay + t * dy
    return ((px - cx) ** 2 + (py - cy) ** 2) ** 0.5


def simplify_ring(ring, tolerance):
    """Simplify an implicitly closed ring.

    The ring is split at the vertex farthest from its first vertex, so both halves are open
    polylines with distinct endpoints. Returns fewer than 3 points when the ring collapses.
    """
    if len(ring) < 4:
        return list(ring)
    start = ring[0]
    far = max(
        range(len(ring)),
        key=lambda i: (ring[i][0] - start[0]) ** 2 + (ring[i][1] - start[1]) ** 2,
    )
    first = douglas_peucker(ring[: far + 1], tolerance)
    second = douglas_peucker(ring[far:] + [start], tolerance)
    out = first[:-1] + second[:-1]
    # The start and split vertices are always kept by the two halves; drop either one when it
    # is collinear (within the tolerance) with its kept neighbours.
    for forced in (start, ring[far]):
        if len(out) > 3:
            i = out.index(forced)
            if segment_distance(out[i], out[i - 1], out[(i + 1) % len(out)]) <= tolerance:
                out.pop(i)
    return out


def clip_ring(ring, bounds):
    """Sutherland-Hodgman clip of a ring to `(west, east, south, north)`."""
    west, east, south, north = bounds
    edges = [
        (lambda p: p[0] >= west, lambda a, b: cross_x(a, b, west)),
        (lambda p: p[0] <= east, lambda a, b: cross_x(a, b, east)),
        (lambda p: p[1] >= south, lambda a, b: cross_y(a, b, south)),
        (lambda p: p[1] <= north, lambda a, b: cross_y(a, b, north)),
    ]
    out = list(ring)
    for inside, cross in edges:
        if not out:
            break
        source, out = out, []
        prev = source[-1]
        for point in source:
            if inside(point):
                if not inside(prev):
                    out.append(cross(prev, point))
                out.append(point)
            elif inside(prev):
                out.append(cross(prev, point))
            prev = point
    return out if len(out) >= 3 else []


def cross_x(a, b, x):
    t = (x - a[0]) / (b[0] - a[0])
    return (x, a[1] + t * (b[1] - a[1]))


def cross_y(a, b, y):
    t = (y - a[1]) / (b[1] - a[1])
    return (a[0] + t * (b[0] - a[0]), y)


def signed_area(ring):
    """Shoelace area in square degrees, positive for counter-clockwise rings."""
    total = 0.0
    for i, (x0, y0) in enumerate(ring):
        x1, y1 = ring[(i + 1) % len(ring)]
        total += x0 * y1 - x1 * y0
    return total / 2.0


def ring_area(ring):
    """Absolute shoelace area in square degrees."""
    return abs(signed_area(ring))


def dissolve(rings):
    """Merge rings that share borders, so simplification cannot open a sliver between them.

    Every ring is oriented counter-clockwise; an edge that two rings traverse in opposite
    directions is a shared border and is cancelled, and the remaining edges are stitched back
    into rings. Natural Earth subdivisions share exact border vertices, which this relies on.
    Rings that touch at a single vertex, or overlap, are refused.
    """
    oriented = [r if signed_area(r) > 0 else list(reversed(r)) for r in rings]
    edges = [(r[i], r[(i + 1) % len(r)]) for r in oriented for i in range(len(r))]
    present = collections.Counter(edges)
    following = {}
    for a, b in edges:
        if (b, a) in present:
            continue
        if a in following:
            raise ValueError(f"rings touch or overlap at {a}; cannot dissolve them")
        following[a] = b
    out = []
    while following:
        start = next(iter(following))
        ring, point = [start], following.pop(start)
        while point != start:
            if point not in following:
                raise ValueError(f"border at {point} does not close; cannot dissolve")
            ring.append(point)
            point = following.pop(point)
        out.append(ring)
    before, after = sum(map(signed_area, oriented)), sum(map(signed_area, out))
    if abs(before - after) > 1e-9 * max(1.0, abs(before)):
        raise ValueError("dissolved area differs from the inputs; rings overlap")
    return out


def round_ring(ring):
    """Round to 3 decimals and drop consecutive repeats (including the wrap-around)."""
    out = []
    for lon, lat in ring:
        point = (round(lon, 3) + 0.0, round(lat, 3) + 0.0)
        if not out or out[-1] != point:
            out.append(point)
    while len(out) > 1 and out[0] == out[-1]:
        out.pop()
    return out


def prepare_rings(rings, bounds, tolerance, min_area):
    """Clip, simplify and round each ring; drop rings under `min_area` square degrees."""
    out = []
    for ring in rings:
        ring = round_ring(simplify_ring(clip_ring(ring, bounds), tolerance))
        if len(ring) >= 3 and ring_area(ring) >= min_area:
            out.append(ring)
    return out


def prepare_geometry(land, exclude, bounds, tolerance, min_area):
    """Prepared `(land, exclude)`. The area floor drops small land only: an exclusion is never
    dropped, since dropping one would turn its area back into land."""
    return (
        prepare_rings(land, bounds, tolerance, min_area),
        prepare_rings(exclude, bounds, tolerance, 0.0),
    )


def polygons(feature):
    """The feature's polygons as lists of open rings, outer ring first."""
    geometry = feature.get("geometry") or {}
    kind, coords = geometry.get("type"), geometry.get("coordinates")
    if kind == "Polygon":
        coords = [coords]
    elif kind != "MultiPolygon":
        raise ValueError(f"unsupported geometry type {kind!r}")
    out = []
    for polygon in coords:
        rings = []
        for ring in polygon:
            points = [(float(p[0]), float(p[1])) for p in ring]
            if len(points) > 1 and points[0] == points[-1]:
                points.pop()
            rings.append(points)
        out.append(rings)
    return out


def interior_point(ring):
    """A point strictly inside a simple ring: the middle of the first span on a scanline."""
    ys = sorted({y for _, y in ring})
    y = (ys[0] + ys[1]) / 2.0 if len(ys) > 1 else ys[0]
    xs = []
    prev = ring[-1]
    for point in ring:
        (x0, y0), (x1, y1) = prev, point
        if (y0 > y) != (y1 > y):
            xs.append(x0 + (y - y0) * (x1 - x0) / (y1 - y0))
        prev = point
    xs.sort()
    return ((xs[0] + xs[1]) / 2.0, y) if len(xs) >= 2 else ring[0]


def contains(ring, px, py):
    """Even-odd ray casting, as in crates/infrastructure/src/region/raster.rs."""
    inside = False
    prev = ring[-1]
    for x, y in ring:
        if (y > py) != (prev[1] > py) and px < (prev[0] - x) * (py - y) / (prev[1] - y) + x:
            inside = not inside
        prev = (x, y)
    return inside


def feature_rings(included, excluded):
    """`(land, exclude)` rings from the included and excluded features."""
    parsed = [polygons(f) for f in included]
    outers = [rings[0] for polys in parsed for rings in polys if rings]
    land, exclude = list(outers), []
    for polys in parsed:
        for rings in polys:
            for hole in rings[1:]:
                px, py = interior_point(hole)
                if not any(contains(o, px, py) for o in outers if o is not rings[0]):
                    exclude.append(hole)
    for f in excluded:
        exclude.extend(rings[0] for rings in polygons(f) if rings)
    return land, exclude


def select_admin0(features, selectors):
    out = []
    for selector in selectors:
        scheme, _, code = selector.partition(":")
        if scheme == "ISO":
            found = [f for f in features if f["properties"].get("ISO_A3") == code]
            if not found:
                found = [f for f in features if f["properties"].get("ISO_A3_EH") == code]
        elif scheme == "ADM0":
            found = [f for f in features if f["properties"].get("ADM0_A3") == code]
        else:
            raise SelectionError(f"admin-0 selector {selector!r} must be ISO:<A3> or ADM0:<A3>")
        if not found:
            raise SelectionError(f"admin-0 selector {selector!r} matched nothing")
        out.extend(f for f in found if f not in out)
    return out


def iso2_to_country_codes(admin0):
    """ISO 3166-1 alpha-2 to the alpha-3 codes Natural Earth files that country under."""
    codes = {}
    for f in admin0:
        p = f["properties"]
        iso2 = p.get("ISO_A2")
        if iso2 and iso2 != "-99":
            codes.setdefault(iso2, set()).update(
                c for c in (p.get("ISO_A3"), p.get("ISO_A3_EH"), p.get("ADM0_A3")) if c != "-99"
            )
    return codes


def select_admin1(features, selectors, iso2_codes):
    out = []
    for selector in selectors:
        code, sep, name = selector.partition(":")
        if not sep or not name:
            raise SelectionError(f"admin-1 selector {selector!r} must be <A3>:<name>")
        found = []
        for f in features:
            p = f["properties"]
            if name not in (p.get("name"), p.get("name_en")):
                continue
            iso_country = (p.get("iso_3166_2") or "").partition("-")[0]
            if p.get("adm0_a3") == code or code in iso2_codes.get(iso_country, ()):
                found.append(f)
        if len(found) != 1:
            raise SelectionError(f"admin-1 selector {selector!r} matched {len(found)} features")
        out.extend(f for f in found if f not in out)
    return out


def format_point(point):
    lon, lat = (round(v, 3) + 0.0 for v in point)
    return f"({lon:.3f}, {lat:.3f})"


def format_rings(name, rings, indent="    "):
    lines = [f"{indent}{name}: ["]
    for ring in rings:
        lines.append(f"{indent}    [")
        for i in range(0, len(ring), POINTS_PER_LINE):
            chunk = ", ".join(format_point(p) for p in ring[i : i + POINTS_PER_LINE])
            lines.append(f"{indent}        {chunk},")
        lines.append(f"{indent}    ],")
    lines.append(f"{indent}],")
    return "\n".join(lines)


def format_polygons(rings):
    """A top-level RON `(polygons: [...])` value."""
    lines = ["(polygons: ["]
    for ring in rings:
        lines.append("    [")
        for i in range(0, len(ring), POINTS_PER_LINE):
            chunk = ", ".join(format_point(p) for p in ring[i : i + POINTS_PER_LINE])
            lines.append(f"        {chunk},")
        lines.append("    ],")
    lines.append("])")
    return "\n".join(lines)


def command_line(argv):
    """The invocation, for provenance, without its `--comment` values (printed separately)."""
    shown, skip = [], False
    for arg in argv:
        if skip:
            skip = False
        elif arg == "--comment":
            skip = True
        elif not arg.startswith("--comment="):
            shown.append(arg)
    return "python3 -I tools/region_authoring/natural_earth.py " + shlex.join(shown)


def sha256(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def read_features(path):
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    if data.get("type") != "FeatureCollection" or not isinstance(data.get("features"), list):
        raise ValueError(f"{path} is not a GeoJSON FeatureCollection")
    return data["features"]


def numbers(text, count, name):
    parts = text.split(",")
    if len(parts) != count:
        raise argparse.ArgumentTypeError(f"--{name} needs {count} comma-separated numbers")
    return [float(p) for p in parts]


def selectors(text):
    return [s.strip() for s in text.split(",") if s.strip()] if text else []


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--admin0", required=True)
    parser.add_argument("--admin1")
    parser.add_argument("--include", default="")
    parser.add_argument("--include-admin1", default="")
    parser.add_argument("--exclude-admin1", default="")
    parser.add_argument("--bounds", required=True, help="W,E,S,N in degrees")
    parser.add_argument("--grid", required=True, help="WIDTH,HEIGHT in cells")
    parser.add_argument("--tolerance-deg", type=float)
    parser.add_argument("--dissolve", action="store_true", help="merge shared borders first")
    parser.add_argument(
        "--extent",
        action="store_true",
        help="print the exclusions as a standalone (polygons: [...]) file",
    )
    parser.add_argument("--comment", action="append", default=[], help="extra header line")
    args = parser.parse_args(argv)
    west, east, south, north = numbers(args.bounds, 4, "bounds")
    width, height = (int(v) for v in numbers(args.grid, 2, "grid"))
    if not (west < east and south < north and width > 0 and height > 0):
        parser.error("bounds must be ordered and the grid non-empty")
    cell_w, cell_h = (east - west) / width, (north - south) / height
    tolerance = args.tolerance_deg if args.tolerance_deg is not None else min(cell_w, cell_h) / 2
    include, include1, exclude1 = (
        selectors(args.include),
        selectors(args.include_admin1),
        selectors(args.exclude_admin1),
    )
    if (include1 or exclude1) and not args.admin1:
        parser.error("admin-1 selectors need --admin1")
    if args.extent and (include or include1):
        parser.error("--extent prints exclusions only; drop --include and --include-admin1")

    admin0 = read_features(args.admin0)
    included = select_admin0(admin0, include)
    excluded = []
    if args.admin1:
        admin1 = read_features(args.admin1)
        codes = iso2_to_country_codes(admin0)
        included += select_admin1(admin1, include1, codes)
        excluded = select_admin1(admin1, exclude1, codes)
    land, exclude = feature_rings(included, excluded)
    if args.dissolve:
        land, exclude = dissolve(land), dissolve(exclude)
    bounds = (west, east, south, north)
    land, exclude = prepare_geometry(land, exclude, bounds, tolerance, 2.0 * cell_w * cell_h)

    header = [
        "Generated by tools/region_authoring/natural_earth.py",
        f"Natural Earth {RELEASE} ({SOURCE}), public domain.",
        f"admin-0: {pathlib.Path(args.admin0).name} SHA-256 {sha256(args.admin0)}",
    ]
    if args.admin1:
        header.append(f"admin-1: {pathlib.Path(args.admin1).name} SHA-256 {sha256(args.admin1)}")
    header += [
        f"include: {','.join(include) or '-'}",
        f"include-admin1: {','.join(include1) or '-'}",
        f"exclude-admin1: {','.join(exclude1) or '-'}",
        f"bounds {args.bounds}, grid {width}x{height}, Douglas-Peucker {tolerance:g} deg"
        + (", shared borders dissolved" if args.dissolve else "")
        + ", land rings under 2 cells dropped",
        *args.comment,
        f"Command: {command_line(argv)}",
    ]
    if args.extent:
        print("\n".join("// " + line for line in header))
        print(format_polygons(exclude))
    else:
        print("\n".join("    // " + line for line in header))
        print(format_rings("land", land))
        print(format_rings("exclude", exclude))
    print(
        f"{len(land)} land and {len(exclude)} exclude rings, "
        f"{sum(map(len, land)) + sum(map(len, exclude))} points",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
