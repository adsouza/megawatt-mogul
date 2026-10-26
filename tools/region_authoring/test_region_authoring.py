"""Tests for the offline region authoring tools.

Run: python3 -I tools/region_authoring/test_region_authoring.py

`-I` drops the working directory from sys.path, so the tool modules are loaded by file path.
"""

import contextlib
import hashlib
import importlib.util
import io
import json
import math
import pathlib
import re
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
SELECTIONS = HERE / "selections"


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


ne = load("natural_earth")
svg = load("research_svg_extent")
sgcc = load("sgcc_exclusions")


def square(west, east, south, north):
    return [(west, south), (east, south), (east, north), (west, north)]


def feature(properties, *polygons):
    """A GeoJSON MultiPolygon feature; each polygon is [outer, *holes] as open rings."""
    coords = [[[list(p) for p in ring + ring[:1]] for ring in polygon] for polygon in polygons]
    return {
        "type": "Feature",
        "properties": properties,
        "geometry": {"type": "MultiPolygon", "coordinates": coords},
    }


class SgccExclusionsTest(unittest.TestCase):
    def test_committed_operating_exclusion_is_reproducible(self):
        self.assertEqual(sgcc.generate(ROOT), (ROOT / "assets/regions/sgcc/operating_exclusions.ron").read_text())

    def test_lexical_arrays_ignore_comments_and_quoted_brackets(self):
        source = '(\n land: [ // ]\n [(1,2),(3,4),(5,6)],\n ],\n geography: ["a.ron"],\n)'
        self.assertEqual(sgcc.array_field(source, "geography"), '["a.ron"]')
        self.assertIn('(5,6)', sgcc.array_field(source, "land"))


class DouglasPeuckerTest(unittest.TestCase):
    def test_douglas_peucker_keeps_endpoints_and_drops_collinear(self):
        line = [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)]
        self.assertEqual(ne.douglas_peucker(line, 0.1), [(0.0, 0.0), (3.0, 0.0)])
        bumpy = [(0.0, 0.0), (1.0, 0.52), (2.0, 1.0), (3.0, 0.48), (4.0, 0.0)]
        self.assertEqual(
            ne.douglas_peucker(bumpy, 0.1), [(0.0, 0.0), (2.0, 1.0), (4.0, 0.0)]
        )
        # Closed rings keep their corners and lose the collinear edge midpoints.
        ring = [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (2.0, 2.0), (0.0, 2.0)]
        simplified = ne.simplify_ring(ring, 0.1)
        self.assertEqual(sorted(simplified), sorted(square(0.0, 2.0, 0.0, 2.0)))


class ClipTest(unittest.TestCase):
    def test_clip_keeps_rings_inside_bounds(self):
        bounds = (0.0, 10.0, 0.0, 10.0)
        inside = square(2.0, 4.0, 2.0, 4.0)
        self.assertEqual(ne.clip_ring(inside, bounds), inside)
        crossing = ne.clip_ring(square(8.0, 12.0, -2.0, 3.0), bounds)
        for lon, lat in crossing:
            self.assertTrue(0.0 <= lon <= 10.0 and 0.0 <= lat <= 10.0, (lon, lat))
        self.assertAlmostEqual(ne.ring_area(crossing), 2.0 * 3.0)
        self.assertEqual(ne.clip_ring(square(11.0, 12.0, 0.0, 1.0), bounds), [])
        # Cells are 1 x 1 here, so a 1.5-square-degree island falls under the 2-cell floor.
        kept = ne.prepare_rings(
            [square(1.0, 4.0, 1.0, 4.0), square(6.0, 7.5, 6.0, 7.0)],
            bounds,
            tolerance=0.5,
            min_area=2.0,
        )
        self.assertEqual(kept, [square(1.0, 4.0, 1.0, 4.0)])

    def test_exclude_rings_are_never_dropped_for_area(self):
        bounds = (0.0, 10.0, 0.0, 10.0)
        islet = square(6.0, 6.5, 6.0, 6.5)
        lagoon = square(2.0, 2.5, 2.0, 2.5)
        land, exclude = ne.prepare_geometry(
            [square(1.0, 4.0, 1.0, 4.0), islet], [lagoon], bounds, tolerance=0.1, min_area=2.0
        )
        self.assertEqual(land, [square(1.0, 4.0, 1.0, 4.0)])
        self.assertEqual(exclude, [lagoon])

    def test_dissolve_removes_shared_borders(self):
        west = [(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)]
        east = [(1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 0.5)]
        apart = square(5.0, 6.0, 5.0, 6.0)
        merged = ne.dissolve([west, list(reversed(east)), apart])
        self.assertEqual(len(merged), 2)
        self.assertAlmostEqual(sum(ne.ring_area(r) for r in merged), 3.0)
        self.assertEqual(sorted(ne.simplify_ring(merged[0], 0.01)), sorted(square(0.0, 2.0, 0.0, 1.0)))
        self.assertIn(apart, merged)
        corner = square(1.0, 2.0, 1.0, 2.0)
        with self.assertRaises(ValueError):
            ne.dissolve([square(0.0, 1.0, 0.0, 1.0), corner])

    def test_uncovered_fraction_measures_raw_area_outside_the_simplified_rings(self):
        raw = [square(0.0, 10.0, 0.0, 10.0)]
        self.assertEqual(ne.uncovered_fraction(raw, raw), 0.0)
        self.assertAlmostEqual(ne.uncovered_fraction(raw, [square(0.0, 9.9, 0.0, 10.0)]), 0.01, delta=1e-9)
        # Gains elsewhere do not offset a loss.
        shifted = [square(0.5, 10.5, 0.0, 10.0)]
        self.assertAlmostEqual(ne.uncovered_fraction(raw, shifted), 0.05, delta=1e-9)
        # Rings are unions, as the rasteriser's `any` test treats them: the shared edge at x = 5
        # is not double-counted, and the strip between the two covers is lost.
        halves = [square(0.0, 5.0, 0.0, 10.0), square(5.0, 10.0, 0.0, 10.0)]
        covers = [square(0.0, 4.0, 0.0, 10.0), square(6.0, 10.0, 0.0, 10.0)]
        self.assertAlmostEqual(ne.uncovered_fraction(halves, covers), 0.2, delta=1e-9)
        self.assertEqual(ne.uncovered_fraction([], raw), 0.0)

    def test_rings_are_rounded_to_three_decimals_without_repeats(self):
        ring = [(0.00001, 0.0), (0.00002, 0.0), (5.12345, 0.0), (5.0, 5.0)]
        self.assertEqual(
            ne.round_ring(ring), [(0.0, 0.0), (5.123, 0.0), (5.0, 5.0)]
        )
        self.assertEqual(ne.format_point((-0.0001, 12.3456)), "(0.000, 12.346)")


class SelectionTest(unittest.TestCase):
    def test_admin0_selectors_fall_back_to_the_ne_extended_code(self):
        features = [
            feature({"ISO_A3": "-99", "ISO_A3_EH": "FRA", "ADM0_A3": "FRA"}, [square(0, 1, 0, 1)]),
            feature({"ISO_A3": "AUS", "ISO_A3_EH": "AUS", "ADM0_A3": "AUS"}, [square(2, 3, 0, 1)]),
            feature({"ISO_A3": "-99", "ISO_A3_EH": "AUS", "ADM0_A3": "IOA"}, [square(4, 5, 0, 1)]),
            feature({"ISO_A3": "-99", "ISO_A3_EH": "-99", "ADM0_A3": "KOS"}, [square(6, 7, 0, 1)]),
        ]
        self.assertEqual(len(ne.select_admin0(features, ["ISO:FRA"])), 1)
        self.assertEqual(ne.select_admin0(features, ["ISO:AUS"]), [features[1]])
        self.assertEqual(ne.select_admin0(features, ["ADM0:KOS"]), [features[3]])
        with self.assertRaises(ne.SelectionError):
            ne.select_admin0(features, ["ISO:XXX"])

    def test_admin1_selector_matches_the_de_jure_country_code(self):
        admin0 = [feature({"ISO_A3": "UKR", "ISO_A3_EH": "UKR", "ADM0_A3": "UKR", "ISO_A2": "UA"})]
        admin1 = [
            feature({"adm0_a3": "RUS", "iso_3166_2": "UA-43", "name": "Crimea", "name_en": "Autonomous Republic of Crimea"}),
            feature({"adm0_a3": "RUS", "iso_3166_2": "RU-MOW", "name": "Moskva", "name_en": "Moscow"}),
            feature({"adm0_a3": "UKR", "iso_3166_2": "UA-14", "name": "Donets'k", "name_en": "Donetsk"}),
        ]
        codes = ne.iso2_to_country_codes(admin0)
        self.assertEqual(ne.select_admin1(admin1, ["UKR:Crimea"], codes), [admin1[0]])
        self.assertEqual(ne.select_admin1(admin1, ["RUS:Moskva"], codes), [admin1[1]])
        self.assertEqual(ne.select_admin1(admin1, ["UKR:Donetsk"], codes), [admin1[2]])
        with self.assertRaises(ne.SelectionError):
            ne.select_admin1(admin1, ["RUS:Donetsk"], codes)

    def test_holes_become_exclusions_unless_filled_by_another_selection(self):
        lake = square(2.0, 3.0, 2.0, 3.0)
        enclave = square(6.0, 7.0, 6.0, 7.0)
        host = feature({}, [square(0.0, 10.0, 0.0, 10.0), lake, enclave])
        guest = feature({}, [enclave])
        land, exclude = ne.feature_rings([host, guest], [])
        self.assertEqual(land, [square(0.0, 10.0, 0.0, 10.0), enclave])
        self.assertEqual(exclude, [lake])
        cut = feature({}, [square(4.0, 5.0, 4.0, 5.0)])
        land, exclude = ne.feature_rings([host], [cut])
        self.assertEqual(exclude, [lake, enclave, square(4.0, 5.0, 4.0, 5.0)])


def write_geojson(tmp, admin0, admin1):
    paths = []
    for name, data in [("a0.geojson", admin0), ("a1.geojson", admin1)]:
        path = pathlib.Path(tmp) / name
        path.write_text(json.dumps(data))
        paths.append(str(path))
    return paths


class SidecarOutputTest(unittest.TestCase):
    def test_land_mode_writes_a_top_level_sidecar_to_every_out_path(self):
        admin0 = {"type": "FeatureCollection", "features": [
            feature({"ISO_A3": "AAA", "ISO_A3_EH": "AAA", "ADM0_A3": "AAA", "ISO_A2": "AA"},
                    [square(0.0, 4.0, 0.0, 4.0), square(1.0, 2.0, 1.0, 2.0)]),
        ]}
        admin1 = {"type": "FeatureCollection", "features": []}
        with tempfile.TemporaryDirectory() as tmp:
            a0, a1 = write_geojson(tmp, admin0, admin1)
            outs = [pathlib.Path(tmp) / "one.ron", pathlib.Path(tmp) / "two.ron"]
            args = ["--admin0", a0, "--admin1", a1, "--include", "ISO:AAA",
                    "--bounds=-1,5,-1,5", "--grid", "6,6"]
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(ne.main(args), 0)
            printed = stdout.getvalue()
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
                out_args = [arg for out in outs for arg in ("--out", str(out))]
                self.assertEqual(ne.main(args + out_args), 0)
            self.assertEqual(stdout.getvalue(), "")
            written = [out.read_text(encoding="utf-8") for out in outs]
        self.assertEqual(written, [printed, printed])
        body = "\n".join(line for line in printed.splitlines() if not line.startswith("//"))
        self.assertTrue(body.startswith("(\n    land: [\n        [\n"), body[:40])
        self.assertIn("\n    ],\n    exclude: [\n        [\n", body)
        self.assertTrue(body.endswith("\n    ],\n)"), body[-20:])
        self.assertTrue(printed.endswith(")\n"))
        command = [line for line in printed.splitlines() if line.startswith("// Command: ")]
        self.assertEqual(len(command), 1)
        self.assertNotIn("--out", command[0])


def selection_args(path):
    """A selection's arguments without its `--out` targets, and the targets."""
    args, outs = [], []
    lines = iter(path.read_text(encoding="utf-8").splitlines())
    for arg in lines:
        if arg == "--out":
            outs.append(next(lines))
        else:
            args.append(arg)
    return args, outs


class SelectionsTest(unittest.TestCase):
    """Every committed Natural Earth sidecar is the output of a selection (spec section 28.11.1)."""

    def test_every_selection_writes_sidecars_its_regions_reference(self):
        selections = sorted(SELECTIONS.glob("*.args"))
        self.assertGreaterEqual(len(selections), 8)
        written = set()
        for selection in selections:
            _, outs = selection_args(selection)
            self.assertTrue(outs, selection.name)
            for out in outs:
                self.assertTrue((ROOT / out).is_file(), out)
                written.add(out)
                match = re.fullmatch(r"assets/regions/([a-z0-9_]+)/([a-z0-9_]+\.ron)", out)
                if match:
                    region, sidecar = match.groups()
                    main = (ROOT / f"assets/regions/{region}.ron").read_text(encoding="utf-8")
                    self.assertRegex(main, r'geography: \[[^\]]*"' + re.escape(sidecar) + '"')
        for region_dir in (ROOT / "assets/regions").iterdir():
            if not region_dir.is_dir():
                continue
            for sidecar in region_dir.glob("*.ron"):
                text = sidecar.read_text(encoding="utf-8")
                if text.startswith("// Generated by tools/region_authoring/natural_earth.py"):
                    self.assertIn(str(sidecar.relative_to(ROOT)), written)

    def test_every_selection_reproduces_its_committed_sidecars(self):
        data = ROOT / "target/natural-earth"
        needed = ["ne_50m_admin_0_countries.geojson", "ne_10m_admin_1_states_provinces.geojson"]
        if not all((data / name).is_file() for name in needed):
            self.skipTest("Natural Earth v5.1.2 is not in target/natural-earth (see README.md)")
        for selection in sorted(SELECTIONS.glob("*.args")):
            args, outs = selection_args(selection)
            with self.subTest(selection=selection.name):
                run = subprocess.run(
                    [sys.executable, "-I", "tools/region_authoring/natural_earth.py", *args],
                    cwd=ROOT, capture_output=True, check=True,
                )
                for out in outs:
                    self.assertEqual(run.stdout, (ROOT / out).read_bytes(), out)


class ExtentModeTest(unittest.TestCase):
    def test_extent_mode_prints_dissolved_exclusions_as_a_sidecar(self):
        admin0 = {"type": "FeatureCollection", "features": [
            feature({"ISO_A3": "UKR", "ISO_A3_EH": "UKR", "ADM0_A3": "UKR", "ISO_A2": "UA"}),
        ]}
        admin1 = {"type": "FeatureCollection", "features": [
            feature({"adm0_a3": "RUS", "iso_3166_2": "UA-43", "name": "Crimea"},
                    [[(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)]]),
            feature({"adm0_a3": "RUS", "iso_3166_2": "UA-40", "name": "Sevastopol"},
                    [[(1.0, 0.0), (1.2, 0.0), (1.2, 1.0), (1.0, 1.0), (1.0, 0.5)]]),
        ]}
        with tempfile.TemporaryDirectory() as tmp:
            paths = []
            for name, data in [("a0.geojson", admin0), ("a1.geojson", admin1)]:
                path = pathlib.Path(tmp) / name
                path.write_text(json.dumps(data))
                paths.append(str(path))
            args = [
                "--admin0", paths[0], "--admin1", paths[1],
                "--exclude-admin1", "UKR:Crimea,UKR:Sevastopol",
                "--bounds=-1,3,-1,2", "--grid", "40,30", "--extent", "--dissolve",
                "--comment", "No sovereignty position is taken.",
            ]
            out = io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(ne.main(args), 0)
            text = out.getvalue()
            digest = hashlib.sha256(pathlib.Path(paths[1]).read_bytes()).hexdigest()
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                ne.main(args + ["--include", "ISO:UKR"])
            # At 0.9 deg the 1.2 x 1 outline collapses, losing everything.
            coarse = args + ["--tolerance-deg", "0.9", "--max-loss-pct", "1"]
            with contextlib.redirect_stdout(io.StringIO()) as lost, contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(ne.main(coarse), 1)
            self.assertEqual(lost.getvalue(), "")
            out = io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(ne.main(args + ["--max-loss-pct", "1"]), 0)
            self.assertIn("// Unsimplified area outside the output: 0.000%", out.getvalue())
        header = [line for line in text.splitlines() if line.startswith("//")]
        self.assertTrue(any(digest in line for line in header), header)
        self.assertIn("// No sovereignty position is taken.", header)
        self.assertTrue(any(line.startswith("// Command: python3 -I") for line in header), header)
        self.assertIn("\n(exclude: [\n", text)
        rings = svg.parse_extent(text)
        self.assertEqual(len(rings), 1)
        self.assertEqual(sorted(rings[0]), sorted(square(0.0, 1.2, 0.0, 1.0)))


class EqualEarthTest(unittest.TestCase):
    def test_equal_earth_inverse_round_trips(self):
        for lat10 in range(-600, 751, 25):
            for lon10 in range(-1800, 1801, 75):
                lon, lat = lon10 / 10.0, lat10 / 10.0
                x, y = svg.equal_earth(lon, lat)
                got = svg.equal_earth_inverse(x, y)
                self.assertIsNotNone(got, (lon, lat))
                self.assertAlmostEqual(got[0], lon, delta=1e-6)
                self.assertAlmostEqual(got[1], lat, delta=1e-6)
        self.assertIsNone(svg.equal_earth_inverse(3.0, 0.0))
        self.assertIsNone(svg.equal_earth_inverse(0.0, 1.4))
        self.assertIsNone(svg.equal_earth_inverse(math.nan, 0.0))

    def test_equal_earth_matches_published_values(self):
        # Plan 6 Task 9 / crates/game region_atlas: A1..A4 = 1.340264, -0.081106, 0.000893, 0.003796.
        self.assertEqual((svg.A1, svg.A2, svg.A3, svg.A4), (1.340264, -0.081106, 0.000893, 0.003796))
        for (lon, lat), (want_x, want_y) in [
            ((0.0, 0.0), (0.0, 0.0)),
            ((180.0, 0.0), (2.7066299836960748, 0.0)),
            ((0.0, 90.0), (0.0, 1.3173627591574)),
        ]:
            x, y = svg.equal_earth(lon, lat)
            self.assertAlmostEqual(x, want_x, delta=1e-12)
            self.assertAlmostEqual(y, want_y, delta=1e-12)


def synthetic_svg(cx, cy, sx, sy, patches=(), wgs84=False, markers=()):
    """A matplotlib-shaped SVG: two parallel and two meridian graticule ticks, with labels.

    With `wgs84`, latitudes are projected through their WGS84 authalic latitude, as Cartopy's
    default ellipsoidal Equal Earth does.
    """

    def pixel(lon, lat):
        x, y = svg.equal_earth(lon, svg.authalic_latitude(lat) if wgs84 else lat)
        return cx + sx * x, cy - sy * y

    def path(points):
        return "M " + " L ".join(f"{x:.6f} {y:.6f}" for x, y in points)

    parts = []
    n = 0
    for lat, label in [(0.0, "0°"), (30.0, "30°N")]:
        n += 1
        (x0, y), (x1, _) = pixel(-170.0, lat), pixel(170.0, lat)
        parts.append(
            f'<g id="line2d_{n}"><path d="{path([(x0, y), (x1, y)])}" style="fill: none"/></g>'
        )
        parts.append(f'<g id="text_{n}"><text x="{x0 + 2:.6f}" y="{y - 3.6:.6f}">{label}</text></g>')
    for lon, label in [(0.0, "0°"), (60.0, "60°E")]:
        n += 1
        points = [pixel(lon, lat / 2.0) for lat in range(-180, 181, 5)]
        parts.append(
            f'<g id="line2d_{n}"><path d="{path(points)}" style="fill: none"/></g>'
        )
        lx, ly = pixel(lon, -60.0)
        parts.append(f'<g id="text_{n}"><text x="{lx:.6f}" y="{ly + 1.5:.6f}">{label}</text></g>')
    # A short dashed border must not be mistaken for a tick.
    bx, by = pixel(10.0, 10.0)
    parts.append(f'<g id="line2d_{n + 1}"><path d="M {bx} {by} L {bx + 3} {by + 4}"/></g>')
    for i, (colour, ring) in enumerate(patches):
        d = path([pixel(lon, lat) for lon, lat in ring]) + " z"
        parts.append(f'<g id="patch_{i + 1}"><path d="{d}" style="fill: {colour}"/></g>')
    for i, (colour, (lon, lat)) in enumerate(markers):
        mx, my = pixel(lon, lat)
        parts.append(
            f'<g id="PathCollection_{i + 1}"><use href="#m" x="{mx:.6f}" y="{my:.6f}" '
            f'style="fill: {colour}"/></g>'
        )
    body = "\n".join(parts)
    return (
        '<?xml version="1.0"?>\n<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2000 1000">'
        f"<g id=\"axes_1\">{body}</g></svg>"
    )


class SvgTest(unittest.TestCase):
    def test_svg_calibration_recovers_graticule_ticks(self):
        root = svg.parse_svg(synthetic_svg(1000.0, 500.0, 380.0, 382.5))
        calibration = svg.calibrate(root)
        self.assertAlmostEqual(calibration.cx, 1000.0, delta=1e-4)
        self.assertAlmostEqual(calibration.cy, 500.0, delta=1e-4)
        self.assertAlmostEqual(calibration.sx, 380.0, delta=1e-4)
        self.assertAlmostEqual(calibration.sy, 382.5, delta=1e-4)
        self.assertLess(calibration.residual_px, 1e-3)
        self.assertEqual(calibration.parallels, [0.0, 30.0])
        self.assertEqual(calibration.meridians, [0.0, 60.0])
        lon, lat = calibration.to_lonlat(*calibration.to_pixel(31.5, 47.25))
        self.assertAlmostEqual(lon, 31.5, delta=1e-6)
        self.assertAlmostEqual(lat, 47.25, delta=1e-6)

    def test_calibration_detects_an_ellipsoidal_map(self):
        root = svg.parse_svg(synthetic_svg(1000.0, 500.0, 381.0, 381.0, wgs84=True))
        calibration = svg.calibrate(root)
        self.assertEqual(calibration.model, "wgs84")
        self.assertLess(calibration.residual_px, 1e-3)
        self.assertAlmostEqual(calibration.sx, 381.0, delta=1e-4)
        self.assertAlmostEqual(calibration.sy, 381.0, delta=1e-4)
        lon, lat = calibration.to_lonlat(*calibration.to_pixel(37.8, 48.0))
        self.assertAlmostEqual(lon, 37.8, delta=1e-6)
        self.assertAlmostEqual(lat, 48.0, delta=1e-6)
        spherical = svg.calibrate(svg.parse_svg(synthetic_svg(1000.0, 500.0, 381.0, 381.0)))
        self.assertEqual(spherical.model, "sphere")

    def test_authalic_latitude_round_trips(self):
        self.assertEqual(svg.authalic_latitude(0.0), 0.0)
        self.assertAlmostEqual(svg.authalic_latitude(90.0), 90.0, delta=1e-9)
        # The WGS84 authalic latitude trails the geodetic one by about 0.128 deg at 45 deg.
        self.assertAlmostEqual(45.0 - svg.authalic_latitude(45.0), 0.1280, delta=1e-3)
        for lat10 in range(-900, 901, 25):
            lat = lat10 / 10.0
            self.assertAlmostEqual(svg.geodetic_latitude(svg.authalic_latitude(lat)), lat, delta=1e-9)

    def test_calibration_needs_two_ticks_on_each_axis(self):
        text = synthetic_svg(1000.0, 500.0, 380.0, 380.0).replace("30°N", "")
        with self.assertRaises(svg.CalibrationError):
            svg.calibrate(svg.parse_svg(text))

    def test_one_line_with_two_labels_is_refused(self):
        text = synthetic_svg(1000.0, 500.0, 380.0, 380.0)
        thirty = text.index(">30°N<")
        start = text.rindex("<text", 0, thirty)
        # A second label, "60°N", on the 30°N parallel.
        relabelled = text[start:thirty] + ">60°N</text>"
        text = text.replace("</g></svg>", f"<g>{relabelled}</g></g></svg>")
        svg.parse_svg(text)
        with self.assertRaises(svg.CalibrationError):
            svg.calibrate(svg.parse_svg(text))

    def test_entity_declarations_are_refused(self):
        hostile = '<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY a "aaaa">]><svg>&a;</svg>'
        with self.assertRaises(ValueError):
            svg.parse_svg(hostile)

    def test_extent_check_names_failing_probes(self):
        crimea = square(32.5, 36.7, 44.3, 46.0)
        self.assertEqual(svg.failed_probes([crimea]), [])
        self.assertEqual(
            svg.failed_probes([square(32.5, 35.0, 44.3, 46.0)]), ["Kerch inside"]
        )
        everything = square(22.0, 41.0, 44.0, 53.0)
        self.assertEqual(
            svg.failed_probes([everything]),
            [f"{name} outside" for name in
             ["Henichesk", "Kherson", "Donetsk", "Kyiv", "Odesa", "Kharkiv"]],
        )

    def test_check_extent_accepts_the_crimea_marker_inside(self):
        crimea = square(32.5, 36.7, 44.3, 46.0)
        root = svg.parse_svg(
            synthetic_svg(
                1000.0, 500.0, 380.0, 380.0,
                patches=[("#7652b2", square(60.0, 80.0, 50.0, 60.0)), ("#78a7dd", square(30.0, 38.0, 46.5, 50.0))],
                markers=[("#7652b2", (34.0, 45.1)), ("#7652b2", (106.9, 47.9))],
            )
        )
        self.assertEqual(svg.check_extent(root, svg.calibrate(root), [crimea]), [])

    def test_check_extent_rejects_misplaced_markers_and_mainland_patches(self):
        crimea = square(32.5, 36.7, 44.3, 46.0)

        def failures(patches=(), markers=()):
            root = svg.parse_svg(
                synthetic_svg(1000.0, 500.0, 380.0, 380.0, patches=patches, markers=markers)
            )
            return svg.check_extent(root, svg.calibrate(root), [crimea])

        crimea_dot = [("#7652b2", (34.0, 45.1))]
        self.assertEqual(len(failures()), 1, "no violet marker over Ukraine")
        self.assertEqual(len(failures(markers=[("#7652b2", (30.5, 50.4))])), 1)
        donbas = [("#7652b2", square(37.0, 39.5, 47.0, 49.0))]
        self.assertEqual(len(failures(patches=donbas, markers=crimea_dot)), 1)
        # A violet patch covering the whole of Ukraine has no vertex in it, but covers Kyiv.
        cover = [("#7652b2", square(20.0, 45.0, 40.0, 55.0))]
        self.assertEqual(len(failures(patches=cover, markers=crimea_dot)), 1)
        # A violet patch wholly inside the extent is consistent with it.
        self.assertEqual(
            failures(patches=[("#7652b2", square(33.0, 34.0, 44.6, 45.4))], markers=crimea_dot),
            [],
        )

    def test_parse_extent_reads_ron_exclusions(self):
        text = """// header
(exclude: [
    [
        (32.500, 44.300), (36.700, 44.300), // a comment
        (36.700, 46.000), (-32.500, 46.000),
    ],
    [(1.000, 2.000), (3.000, 4.000), (5.000, 6.000)],
])
"""
        self.assertEqual(
            svg.parse_extent(text),
            [
                [(32.5, 44.3), (36.7, 44.3), (36.7, 46.0), (-32.5, 46.0)],
                [(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)],
            ],
        )
        for wrong in ["(land: [])", "(polygons: [[(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)]])"]:
            with self.assertRaises(ValueError):
                svg.parse_extent(wrong)


if __name__ == "__main__":
    unittest.main()
