"""Offline, deterministic authoring from archived NASA POWER responses; no network requests.

Run from repository root: python3 -I tools/region_authoring/solar_clearness.py
Annual all-sky / clear-sky energy is a dimensionless cloud attenuation fraction.
Inverse-distance-squared interpolation in latitude-adjusted degrees is an authoring
judgment, not a measured raster. Anchor coordinates snap to their region cell so
that the checked-in cell value reproduces each independent data receipt exactly.
"""
import json
import math
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RECEIPTS = ROOT / 'docs/research/2026-10-26-solar-calibration-anchors.json'


def author(region, anchors):
    source = (ROOT / f'assets/regions/{region}.ron').read_text()
    bounds = re.search(r'bounds:\s*\(west:\s*([-\d.]+),\s*east:\s*([-\d.]+),\s*south:\s*([-\d.]+),\s*north:\s*([-\d.]+)', source)
    west, east, south, north = map(float, bounds.groups())
    grid = re.search(r'grid:\s*\(width:\s*(\d+),\s*height:\s*(\d+)', source)
    width, height = map(int, grid.groups())
    snapped = []
    for anchor in anchors:
        x = min(width-1, max(0, int((anchor['longitude']-west)/(east-west)*width)))
        y = min(height-1, max(0, int((north-anchor['latitude'])/(north-south)*height)))
        value = anchor['all_sky']['ANN'] / anchor['clear_sky']['ANN']
        assert 0 <= value <= 1
        snapped.append((west+(x+.5)*(east-west)/width, north-(y+.5)*(north-south)/height, value))
    rows = []
    for y in range(height):
        lat = north-(y+.5)*(north-south)/height
        row = []
        for x in range(width):
            lon = west+(x+.5)*(east-west)/width
            weights = []
            for a_lon, a_lat, value in snapped:
                d2 = ((lon-a_lon)*math.cos(math.radians(lat)))**2 + (lat-a_lat)**2
                if d2 < 1e-20:
                    result = value
                    break
                weights.append((1/d2, value))
            else:
                result = sum(w*v for w,v in weights)/sum(w for w,_ in weights)
            row.append(repr(result))
        rows.append('    '+', '.join(row)+',')
    output = ROOT / f'assets/regions/{region}/solar_clearness_cells.ron'
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text('// NASA POWER SYN1DEG: annual ALLSKY / CLRSKY; dimensionless clear-sky fraction.\n'
                      '// Receipts: docs/research/2026-10-26-solar-calibration-anchors.json.\n'
                      '// Offline solar_clearness.py; IDW interpolation is an authoring judgment.\n'
                      '(cells: [\n'+'\n'.join(rows)+'\n])\n')


if __name__ == '__main__':
    receipts = json.loads(RECEIPTS.read_text())['anchors']
    for region in sorted({a['region'] for a in receipts}):
        author(region, [a for a in receipts if a['region'] == region])
