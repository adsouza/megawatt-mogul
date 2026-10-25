"""Tests for the offline region authoring tools.

Run: python3 -I tools/region_authoring/test_region_authoring.py

`-I` drops the working directory from sys.path, so the tool modules are loaded by file path.
"""

import importlib.util
import math
import pathlib
import sys
import unittest

HERE = pathlib.Path(__file__).resolve().parent


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


ne = load("natural_earth")
svg = load("research_svg_extent")


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


def synthetic_svg(cx, cy, sx, sy, patches=(), wgs84=False):
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

    def test_violet_patches_inside_the_box_are_inverse_projected(self):
        donbas = square(37.0, 39.5, 47.0, 49.0)
        far = square(60.0, 80.0, 50.0, 60.0)
        root = svg.parse_svg(
            synthetic_svg(
                1000.0,
                500.0,
                380.0,
                380.0,
                patches=[("#7652b2", donbas), ("#7652b2", far), ("#78a7dd", donbas)],
            )
        )
        rings = svg.violet_rings(root, svg.calibrate(root))
        self.assertEqual(len(rings), 1)
        for (lon, lat), (want_lon, want_lat) in zip(rings[0], donbas):
            self.assertAlmostEqual(lon, want_lon, delta=1e-5)
            self.assertAlmostEqual(lat, want_lat, delta=1e-5)

    def test_extent_check_names_failing_probes(self):
        crimea = square(32.5, 36.6, 44.3, 46.2)
        self.assertEqual(svg.failed_probes([crimea]), ["Donetsk inside"])
        everything = square(22.0, 41.0, 44.0, 53.0)
        self.assertEqual(
            svg.failed_probes([everything]),
            ["Kyiv outside", "Odesa outside", "Kharkiv outside"],
        )
        self.assertEqual(svg.failed_probes([crimea, square(37.0, 40.0, 47.0, 48.5)]), [])


if __name__ == "__main__":
    unittest.main()
