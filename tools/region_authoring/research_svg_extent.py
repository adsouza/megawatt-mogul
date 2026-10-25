"""Provenance check of the Ukraine-IPS operating extent against the research map.

    python3 -I tools/region_authoring/research_svg_extent.py <World_Power_Grids_Top_8.svg> \\
        --check-extent crates/infrastructure/authoring/ukraine_ips_operating_extent.ron

The extent itself is Natural Earth admin-1 Crimea and Sevastopol, written by `natural_earth.py
--extent`. This tool checks that the research map agrees with it:

1. Read the research map (a Matplotlib SVG in Equal Earth).
2. Calibrate pixels to Equal Earth from the labelled graticule `line2d` ticks. Both the
   spherical form and Cartopy's default WGS84 form (the same polynomial on the authalic
   latitude) are fitted; the one with the smaller residual is kept.
3. Inverse-project with Newton iteration (A1..A4 = 1.340264, -0.081106, 0.000893, 0.003796),
   then convert the authalic latitude back to geodetic for the WGS84 form.
4. Require an IPS violet (#7652b2) point marker over Ukraine (22-41 deg E, 44-53 deg N), and
   every such marker to lie inside the extent: the map marks Crimea's connection with a point.
5. Require that no #7652b2 patch covers mainland Ukraine (22-41 deg E, 44.5-53 deg N, outside the
   extent): the map draws no wider occupied extent.
6. Require the probe points: Sevastopol (on land), Simferopol and Kerch inside; Henichesk,
   Kherson, Donetsk, Kyiv, Odesa and Kharkiv outside.

It exits non-zero on any failure. Run with `python3 -I` and pass paths as arguments: inputs are
untrusted.
"""

import argparse
import dataclasses
import importlib.util
import math
import pathlib
import re
import sys
import xml.etree.ElementTree as ET

A1 = 1.340264
A2 = -0.081106
A3 = 0.000893
A4 = 0.003796
M = math.sqrt(3.0) / 2.0

IPS_VIOLET = "#7652b2"
UKRAINE_BOX = (22.0, 41.0, 44.0, 53.0)
MAINLAND_BOX = (22.0, 41.0, 44.5, 53.0)
# Sevastopol's probe is in the city on land: (33.52, 44.62) is in the bay notch of the 1:10m
# polygon.
PROBES_INSIDE = [("Sevastopol", 33.52, 44.60), ("Simferopol", 34.10, 44.95), ("Kerch", 36.47, 45.36)]
PROBES_OUTSIDE = [
    ("Henichesk", 34.82, 46.17),
    ("Kherson", 32.62, 46.64),
    ("Donetsk", 37.80, 48.00),
    ("Kyiv", 30.52, 50.45),
    ("Odesa", 30.73, 46.48),
    ("Kharkiv", 36.23, 49.99),
]
LABEL_TOLERANCE_PX = 8.0
MAX_RESIDUAL_PX = 1.5
SVG = "{http://www.w3.org/2000/svg}"
LABEL = re.compile(r"^(\d+(?:\.\d+)?)°([NSEW]?)$")
TOKEN = re.compile(r"[A-Za-z]|[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?")


def _load_natural_earth():
    # `-I` keeps this directory off sys.path, so load the sibling module by its path.
    path = pathlib.Path(__file__).resolve().parent / "natural_earth.py"
    spec = importlib.util.spec_from_file_location("natural_earth", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


ne = _load_natural_earth()


class CalibrationError(ValueError):
    """The graticule does not determine the pixel calibration."""


def _derivative(theta):
    t2 = theta * theta
    return A1 + 3.0 * A2 * t2 + t2**3 * (7.0 * A3 + 9.0 * A4 * t2)


def _y_of_theta(theta):
    t2 = theta * theta
    return theta * (A1 + A2 * t2 + t2**3 * (A3 + A4 * t2))


def equal_earth(lon, lat):
    """Spherical Equal Earth on a unit sphere; degrees in, projected units out."""
    theta = math.asin(M * math.sin(math.radians(lat)))
    return math.radians(lon) * math.cos(theta) / (M * _derivative(theta)), _y_of_theta(theta)


MAX_Y = equal_earth(0.0, 90.0)[1]

# Cartopy's default globe is WGS84. Its ellipsoidal Equal Earth applies the same polynomial to
# the authalic latitude (equal-area sphere); the authalic radius is absorbed by the pixel scale.
WGS84_E2 = 0.00669437999014
_E = math.sqrt(WGS84_E2)


def _q(sin_phi):
    return (1.0 - WGS84_E2) * (
        sin_phi / (1.0 - WGS84_E2 * sin_phi * sin_phi)
        - math.log((1.0 - _E * sin_phi) / (1.0 + _E * sin_phi)) / (2.0 * _E)
    )


_QP = _q(1.0)


def authalic_latitude(lat):
    """WGS84 geodetic latitude to authalic latitude, in degrees."""
    ratio = _q(math.sin(math.radians(lat))) / _QP
    return math.degrees(math.asin(max(-1.0, min(1.0, ratio))))


def geodetic_latitude(beta):
    """Inverse of `authalic_latitude` (Snyder 1987, eq. 3-16), in degrees."""
    q = _QP * math.sin(math.radians(beta))
    if abs(q) >= _QP:
        return math.copysign(90.0, beta)
    phi = math.asin(q / 2.0)
    for _ in range(30):
        s, c = math.sin(phi), math.cos(phi)
        one = 1.0 - WGS84_E2 * s * s
        delta = (one * one / (2.0 * c)) * (
            q / (1.0 - WGS84_E2) - s / one + math.log((1.0 - _E * s) / (1.0 + _E * s)) / (2.0 * _E)
        )
        phi += delta
        if abs(delta) < 1e-14:
            break
    return math.degrees(phi)


# Latitude models: geodetic to projection latitude, and back.
MODELS = {
    "sphere": (lambda lat: lat, lambda lat: lat),
    "wgs84": (authalic_latitude, geodetic_latitude),
}


def _theta(y):
    """Newton iteration for the parametric latitude, at most 10 steps; None if it stalls."""
    theta = y
    for _ in range(10):
        delta = (_y_of_theta(theta) - y) / _derivative(theta)
        theta -= delta
        if abs(delta) < 1e-12:
            return theta
    return None


def equal_earth_inverse(x, y):
    """`(lon, lat)` in degrees, or None outside the curved Earth or for non-finite input."""
    if not (math.isfinite(x) and math.isfinite(y)) or abs(y) > MAX_Y + 1e-12:
        return None
    theta = _theta(y)
    if theta is None:
        return None
    sin_lat = math.sin(theta) / M
    lon = math.degrees(M * x * _derivative(theta) / math.cos(theta))
    if abs(sin_lat) > 1.0 + 1e-12 or abs(lon) > 180.0 + 1e-9:
        return None
    return max(-180.0, min(180.0, lon)), math.degrees(math.asin(max(-1.0, min(1.0, sin_lat))))


def parse_svg(text):
    """Parse SVG text, refusing entity declarations (no expansion of untrusted DTDs)."""
    if re.search(r"<!ENTITY", text, re.IGNORECASE):
        raise ValueError("SVG declares XML entities; refusing to parse it")
    return ET.fromstring(text)


def _elements(root):
    """Every element with whether an ancestor (or itself) carries a transform."""
    stack = [(root, False)]
    while stack:
        element, transformed = stack.pop()
        transformed = transformed or element.get("transform") is not None
        yield element, transformed
        stack.extend((child, transformed) for child in reversed(list(element)))


def path_points(d):
    """Subpaths of an absolute SVG path as point lists; curves are flattened to 8 segments."""
    tokens = TOKEN.findall(d)
    subpaths, current, i, command = [], [], 0, None
    pen = (0.0, 0.0)

    def take(n):
        nonlocal i
        values = [float(t) for t in tokens[i : i + n]]
        if len(values) != n or any(t.isalpha() for t in tokens[i : i + n]):
            raise ValueError(f"malformed path data near token {i}")
        i += n
        return values

    while i < len(tokens):
        if tokens[i].isalpha():
            command = tokens[i]
            i += 1
            if command in "zZ":
                if current:
                    subpaths.append(current)
                    pen = current[0]
                current = []
                continue
        if command == "M":
            if current:
                subpaths.append(current)
            pen = tuple(take(2))
            current = [pen]
            command = "L"
        elif command == "L":
            pen = tuple(take(2))
            current.append(pen)
        elif command == "H":
            pen = (take(1)[0], pen[1])
            current.append(pen)
        elif command == "V":
            pen = (pen[0], take(1)[0])
            current.append(pen)
        elif command in ("Q", "C"):
            values = take(4 if command == "Q" else 6)
            controls = [pen] + list(zip(values[::2], values[1::2]))
            for step in range(1, 9):
                current.append(_bezier(controls, step / 8.0))
            pen = controls[-1]
        else:
            raise ValueError(f"unsupported path command {command!r}")
    if current:
        subpaths.append(current)
    return [[p for k, p in enumerate(s) if k == 0 or p != s[k - 1]] for s in subpaths]


def _bezier(points, t):
    while len(points) > 1:
        points = [
            (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)
            for a, b in zip(points, points[1:])
        ]
    return points[0]


def _polyline_distance(point, points):
    if len(points) == 1:
        return math.dist(point, points[0])
    return min(ne.segment_distance(point, a, b) for a, b in zip(points, points[1:]))


@dataclasses.dataclass
class Calibration:
    """`px = cx + sx * X`, `py = cy - sy * Y` for Equal Earth `(X, Y)`."""

    cx: float
    cy: float
    sx: float
    sy: float
    residual_px: float
    parallels: list
    meridians: list
    model: str = "sphere"

    def to_pixel(self, lon, lat):
        x, y = equal_earth(lon, MODELS[self.model][0](lat))
        return self.cx + self.sx * x, self.cy - self.sy * y

    def to_lonlat(self, px, py):
        result = equal_earth_inverse((px - self.cx) / self.sx, (self.cy - py) / self.sy)
        if result is None:
            raise CalibrationError(f"pixel ({px}, {py}) is outside the projected Earth")
        return result[0], MODELS[self.model][1](result[1])


def graticule_ticks(root):
    """`({lat: y}, {lon: points})` for the graticule lines matched to their labels."""
    lines = []
    for element, transformed in _elements(root):
        if element.tag == SVG + "g" and element.get("id", "").startswith("line2d_"):
            for path in element.iter(SVG + "path"):
                if transformed:
                    raise CalibrationError(f"{element.get('id')} is transformed; unsupported")
                points = [p for s in path_points(path.get("d", "")) for p in s]
                if len(points) >= 2:
                    lines.append(points)
    if not lines:
        raise CalibrationError("no line2d graticule paths")
    span = lambda pts, k: max(p[k] for p in pts) - min(p[k] for p in pts)  # noqa: E731
    tallest = max(span(pts, 1) for pts in lines)
    parallels = [pts for pts in lines if span(pts, 1) < 1e-6 and span(pts, 0) > 0.0]
    meridians = [pts for pts in lines if span(pts, 1) >= 0.5 * tallest > 0.0]

    lat_ticks, lon_ticks, labels = {}, {}, {}
    for element in root.iter(SVG + "text"):
        match = LABEL.match("".join(element.itertext()).strip())
        if not match:
            continue
        value, hemisphere = float(match.group(1)), match.group(2)
        if hemisphere == "" and value != 0.0:
            continue
        anchor = (float(element.get("x", "nan")), float(element.get("y", "nan")))
        candidates = []
        if hemisphere in ("N", "S", ""):
            sign = -1.0 if hemisphere == "S" else 1.0
            candidates += [(_polyline_distance(anchor, l), "lat", sign * value, l) for l in parallels]
        if hemisphere in ("E", "W", ""):
            sign = -1.0 if hemisphere == "W" else 1.0
            candidates += [(_polyline_distance(anchor, l), "lon", sign * value, l) for l in meridians]
        candidates = [c for c in candidates if c[0] <= LABEL_TOLERANCE_PX]
        if not candidates:
            continue
        _, kind, value, line = min(candidates, key=lambda c: c[0])
        ticks = lat_ticks if kind == "lat" else lon_ticks
        if value in ticks and ticks[value] is not line:
            raise CalibrationError(f"two {kind} lines are labelled {value}")
        if labels.setdefault(id(line), value) != value:
            raise CalibrationError(f"one {kind} line is labelled {labels[id(line)]} and {value}")
        ticks[value] = line
    return {lat: line[0][1] for lat, line in lat_ticks.items()}, lon_ticks


def _fit(pairs):
    """Least-squares `v = a + b * u` over `(u, v)` pairs."""
    n = len(pairs)
    mu = sum(u for u, _ in pairs) / n
    mv = sum(v for _, v in pairs) / n
    suu = sum((u - mu) ** 2 for u, _ in pairs)
    if suu == 0.0:
        raise CalibrationError("graticule ticks do not span the axis")
    b = sum((u - mu) * (v - mv) for u, v in pairs) / suu
    return mv - b * mu, b


def calibrate(root):
    """Fit the pixel calibration under each latitude model and keep the closer fit."""
    parallels, meridians = graticule_ticks(root)
    if len(parallels) < 2 or len(meridians) < 2:
        raise CalibrationError(
            f"need two labelled parallels and meridians, found {sorted(parallels)} and "
            f"{sorted(meridians)}"
        )
    fits = [_calibrate_with(model, parallels, meridians) for model in MODELS]
    return min(fits, key=lambda c: c.residual_px)


def _calibrate_with(model, parallels, meridians):
    forward = MODELS[model][0]
    y_pairs = [(equal_earth(0.0, forward(lat))[1], y) for lat, y in parallels.items()]
    cy, b = _fit(y_pairs)
    sy = -b
    if sy <= 0.0:
        raise CalibrationError("parallels are not ordered north to south")
    x_pairs = []
    for lon, points in meridians.items():
        for px, py in points:
            theta = _theta(max(-MAX_Y, min(MAX_Y, (cy - py) / sy)))
            if theta is None:
                raise CalibrationError("meridian point does not invert")
            x_pairs.append((math.radians(lon) * math.cos(theta) / (M * _derivative(theta)), px))
    cx, sx = _fit(x_pairs)
    if sx <= 0.0:
        raise CalibrationError("meridians are not ordered west to east")
    residual = max(
        [abs(cy - sy * u - v) for u, v in y_pairs] + [abs(cx + sx * u - v) for u, v in x_pairs]
    )
    return Calibration(cx, cy, sx, sy, residual, sorted(parallels), sorted(meridians), model)


def fill_colour(element):
    match = re.search(r"(?:^|;)\s*fill:\s*(#[0-9a-fA-F]{6})", element.get("style", ""))
    colour = match.group(1) if match else element.get("fill", "")
    return colour.lower()


def _in_box(point, box):
    west, east, south, north = box
    return west <= point[0] <= east and south <= point[1] <= north


def violet_patches(root, calibration, colour=IPS_VIOLET):
    """Lon/lat subpath rings of every on-globe `colour` path, one list per patch."""
    patches = []
    for element, transformed in _elements(root):
        if element.tag != SVG + "path" or fill_colour(element) != colour or not element.get("d"):
            continue
        subpaths = [s for s in path_points(element.get("d")) if len(s) >= 3]
        if not subpaths:
            continue
        if transformed:
            raise CalibrationError("a candidate patch is transformed; unsupported")
        try:
            patches.append([[calibration.to_lonlat(px, py) for px, py in s] for s in subpaths])
        except CalibrationError:
            continue  # legend swatches and other marks off the globe
    return patches


def violet_markers(root, calibration, colour=IPS_VIOLET):
    """Lon/lat of every on-globe `colour` point marker (`<use>`)."""
    markers = []
    for element, transformed in _elements(root):
        if element.tag != SVG + "use" or fill_colour(element) != colour:
            continue
        if transformed:
            raise CalibrationError("a candidate marker is transformed; unsupported")
        try:
            markers.append(
                calibration.to_lonlat(float(element.get("x", "nan")), float(element.get("y", "nan")))
            )
        except CalibrationError:
            continue
    return markers


def check_extent(root, calibration, rings):
    """Every way the research map or the probes disagree with the extent `rings`."""
    inside = lambda lon, lat: any(ne.contains(r, lon, lat) for r in rings)  # noqa: E731
    failures = []
    markers = [m for m in violet_markers(root, calibration) if _in_box(m, UKRAINE_BOX)]
    if not markers:
        failures.append(f"no {IPS_VIOLET} point marker over Ukraine {UKRAINE_BOX}")
    for lon, lat in markers:
        if not inside(lon, lat):
            failures.append(f"{IPS_VIOLET} marker at ({lon:.2f}, {lat:.2f}) is outside the extent")
    for patch in violet_patches(root, calibration):
        mainland = [
            p for ring in patch for p in ring if _in_box(p, MAINLAND_BOX) and not inside(*p)
        ]
        covered = [
            name
            for name, lon, lat in PROBES_OUTSIDE
            if sum(ne.contains(ring, lon, lat) for ring in patch) % 2
        ]
        if mainland or covered:
            where = f"vertex ({mainland[0][0]:.2f}, {mainland[0][1]:.2f})" if mainland else ""
            where += (", " if where and covered else "") + (", ".join(covered) if covered else "")
            failures.append(f"a {IPS_VIOLET} patch covers mainland Ukraine: {where}")
    return failures + failed_probes(rings)


NUMBER = r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?"
PAIR = re.compile(rf"\(\s*({NUMBER})\s*,\s*({NUMBER})\s*\)")


def parse_extent(text):
    """Rings of a RON `(polygons: [[(lon, lat), ...], ...])` file; `//` comments allowed."""
    body = re.sub(r"//[^\n]*", "", text)
    match = re.fullmatch(r"\s*\(\s*polygons\s*:\s*\[(.*)\]\s*,?\s*\)\s*", body, re.DOTALL)
    if not match:
        raise ValueError("extent is not a (polygons: [...]) RON value")
    rings = []
    for ring_match in re.finditer(r"\[([^\[\]]*)\]", match.group(1)):
        ring = [(float(a), float(b)) for a, b in PAIR.findall(ring_match.group(1))]
        if PAIR.sub("", ring_match.group(1)).strip(" \t\n,") or len(ring) < 3:
            raise ValueError("extent ring is not a list of at least three (lon, lat) pairs")
        rings.append(ring)
    if re.sub(r"\[[^\[\]]*\]", "", match.group(1)).strip(" \t\n,"):
        raise ValueError("extent has text outside its rings")
    if not rings:
        raise ValueError("extent has no rings")
    return rings


def failed_probes(rings):
    """Names of the binding probe points that the extent gets wrong."""
    inside = lambda lon, lat: any(ne.contains(r, lon, lat) for r in rings)  # noqa: E731
    failed = [f"{n} inside" for n, lon, lat in PROBES_INSIDE if not inside(lon, lat)]
    failed += [f"{n} outside" for n, lon, lat in PROBES_OUTSIDE if inside(lon, lat)]
    return failed


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("svg")
    parser.add_argument("--check-extent", required=True, metavar="RON")
    args = parser.parse_args(argv)
    root = parse_svg(pathlib.Path(args.svg).read_bytes().decode("utf-8"))
    calibration = calibrate(root)
    print(
        f"calibration: cx={calibration.cx:.4f} cy={calibration.cy:.4f} sx={calibration.sx:.4f} "
        f"sy={calibration.sy:.4f}, {calibration.model}, max residual "
        f"{calibration.residual_px:.3f} px, parallels {calibration.parallels}, "
        f"meridians {calibration.meridians}"
    )
    if calibration.residual_px > MAX_RESIDUAL_PX:
        print(f"error: residual exceeds {MAX_RESIDUAL_PX} px", file=sys.stderr)
        return 1
    rings = parse_extent(pathlib.Path(args.check_extent).read_text(encoding="utf-8"))
    failures = check_extent(root, calibration, rings)
    for failure in failures:
        print(f"error: {failure}", file=sys.stderr)
    if failures:
        return 1
    markers = [m for m in violet_markers(root, calibration) if _in_box(m, UKRAINE_BOX)]
    print(
        f"ok: {len(rings)} polygons; {IPS_VIOLET} markers over Ukraine "
        f"{[(round(a, 2), round(b, 2)) for a, b in markers]} are inside; no {IPS_VIOLET} patch "
        f"covers mainland Ukraine; probes inside {[p[0] for p in PROBES_INSIDE]}, outside "
        f"{[p[0] for p in PROBES_OUTSIDE]}"
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
