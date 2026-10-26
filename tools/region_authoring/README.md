# Region authoring tools

Offline helpers for writing `assets/regions/*.ron`. Nothing here runs in the game or in CI's
Rust build. Generated data goes in sidecar files under `assets/regions/<id>/`, which the
hand-authored main file references (spec §28.11.1). Never paste generated data into the main
file.

## Inputs

Natural Earth, release tag **v5.1.2** of
[`nvkelso/natural-earth-vector`](https://github.com/nvkelso/natural-earth-vector). The data is
public domain. Store the files in `target/natural-earth/`, which is git-ignored, and never commit
them.

| File | URL | SHA-256 |
| --- | --- | --- |
| `ne_50m_admin_0_countries.geojson` (3.1 MB) | `https://raw.githubusercontent.com/nvkelso/natural-earth-vector/v5.1.2/geojson/ne_50m_admin_0_countries.geojson` | `3e458fc036ad0a66411f2c1e6cac49c5d7bfb81cb1123bc513b22511a2b7fdeb` |
| `ne_10m_admin_1_states_provinces.geojson` (40.7 MB) | `https://raw.githubusercontent.com/nvkelso/natural-earth-vector/v5.1.2/geojson/ne_10m_admin_1_states_provinces.geojson` | `22d0e3ad85eb3e27f17cabf8ba2d50e554fbc27a87796ff891d958185da62fb5` |

Admin-1 uses the 1:10m file: the 1:50m admin-1 file has no subdivisions for Ukraine, Denmark or
Algeria.

The research map is `World_Power_Grids_Top_8.svg` (6,924,878 bytes, Matplotlib 3.10.8, dated
2026-10-18), SHA-256
`cb4b8c53008db19f7a26dba3fc65e7355973629ad747d74b7eea55df83777a91`. It is kept outside the
repository and its hash is the provenance.

Treat every downloaded file as untrusted. Run Python with `-I` and pass paths as arguments. The
tools read GeoJSON with `json` only and refuse SVGs that declare XML entities.

## `natural_earth.py`: land and exclusion polygons

```sh
python3 -I tools/region_authoring/natural_earth.py \
    --admin0 target/natural-earth/ne_50m_admin_0_countries.geojson \
    --admin1 target/natural-earth/ne_10m_admin_1_states_provinces.geojson \
    --include ISO:ESP,ISO:MAR --include-admin1 "RUS:Moskva" --exclude-admin1 "UKR:Crimea" \
    --bounds=W,E,S,N --grid WIDTH,HEIGHT [--tolerance-deg T] [--dissolve] \
    [--extent] [--comment TEXT ...] [--out SIDECAR ...]
```

Write `--bounds=` with `=`, because argparse reads a leading `-20,...` as an option.

It writes a geography sidecar: a provenance comment and then the RON document
`(land: [...], exclude: [...])`, as `(lon, lat)` with 3 decimals. It writes the sidecar to each
`--out` path, or to stdout when there is none. A region lists its sidecars in `geography`, and
loading puts their rings ahead of the main file's hand-authored rings.

- `--grid` sets the cell size. The default Douglas–Peucker tolerance is half a cell. After
  clipping to the bounds, land rings under 2 cells' area are dropped. Exclude rings are never
  dropped, because dropping one would turn its area back into land.
- `--dissolve` merges rings that share borders before simplifying. Without it, neighbours are
  simplified separately, and the two simplified borders open slivers between them. The merge
  relies on Natural Earth's exact shared vertices, and it refuses rings that touch at a point or
  overlap.
- `--max-loss-pct P` fails the run when more than P% of the unsimplified (clipped, dissolved)
  area lies outside the simplified output. Every header records the measured share, computed
  by scanline integration.
- `--extent` writes only the exclusions, as `(exclude: [...])`. `--comment` adds header lines.
  The header always ends with the generating command, without the comments or `--out` paths.
- `ISO:<A3>` matches admin-0 `ISO_A3`, falling back to `ISO_A3_EH` (France and Norway carry
  `-99`). `ADM0:<A3>` matches `ADM0_A3`, for Kosovo and Northern Cyprus.
- `<A3>:<name>` matches an admin-1 `name` or `name_en`. Its country is the feature's `adm0_a3` or
  the country of its ISO 3166-2 code, so `UKR:Crimea` works although Natural Earth files Crimea
  under `RUS`. Every selector must match, and an admin-1 selector must match exactly one feature.
- Holes in included countries become `exclude` rings, unless another included feature fills
  them (an enclave).

## Selections

Every committed run is a file in `selections/`, with one argument per line and its `--out`
targets last. Rerun one from the repository root with:

```sh
tr '\n' '\0' < tools/region_authoring/selections/<name>.args \
    | xargs -0 python3 -I tools/region_authoring/natural_earth.py
```

`test_region_authoring.py` checks two things. First, every selection writes a sidecar that its
region references. Second, when the Natural Earth files are in `target/natural-earth/`, it reruns
every selection and compares the output with the committed sidecars byte for byte. Without the
data, the second test is skipped.

## The Ukraine–IPS operating extent

`crates/infrastructure/authoring/ukraine_ips_operating_extent.ron` is the canonical exclusion
that Continental Europe–Maghreb and IPS/UPS share. It holds Natural Earth 1:10m admin-1 "Crimea"
and "Sevastopol", dissolved into one outline. It is a schematic operating extent per the research
notes and takes no sovereignty position.

The research map marks Crimea's connection with a violet point and draws no wider occupied
extent, so mainland Ukraine, Donetsk and Luhansk included, stays in Continental Europe–Maghreb.
Its selection, `selections/ukraine_ips_operating_extent.args`, writes the file and identical
copies at `assets/regions/continental_europe_maghreb/ukraine_ips_operating_extent.ron` and
`assets/regions/ips_ups/ukraine_ips_operating_extent.ron`. A shipped-regions test checks that
the copies match. The selection holds the full `--comment` text. The extent uses a near-raw 0.02° tolerance
rather than half a cell. At half a cell (0.15°), the outline missed about 10% of the raw area
along the coast, and a land cell centre of either region could have sat in raw Crimea outside
the exclusion. `--max-loss-pct 1` makes the run fail if more than 1% of the dissolved,
unsimplified area lies outside the output. On 2026-10-25 it measured 0.878%, with 87 points.

## `research_svg_extent.py`: provenance check

```sh
python3 -I tools/region_authoring/research_svg_extent.py \
    ~/Downloads/World_Power_Grids_Top_8.svg \
    --check-extent crates/infrastructure/authoring/ukraine_ips_operating_extent.ron
```

The tool works as follows.

1. It calibrates pixels to Equal Earth from the labelled graticule `line2d` ticks. It fits both
   the spherical form and Cartopy's default WGS84 form (the same A1–A4 polynomial on the
   authalic latitude) and keeps the closer fit. It inverse-projects with Newton iteration.
2. It requires a `#7652b2` (IPS violet) point marker over Ukraine (22–41°E, 44–53°N), and every
   such marker to lie inside the extent. The map's Crimea dot is at about 34.0°E, 45.1°N.
3. It requires that no `#7652b2` patch covers mainland Ukraine (22–41°E, 44.5–53°N, outside the
   extent). A patch fails if it has a vertex there, or if it covers an outside probe.
4. It requires these probes:
   - inside: Sevastopol (33.52, 44.60), on land in the city; Simferopol (34.10, 44.95); Kerch
     (36.47, 45.36);
   - outside: Henichesk (34.82, 46.17), Kherson (32.62, 46.64), Donetsk (37.80, 48.00), Kyiv,
     Odesa and Kharkiv.

   The earlier Sevastopol probe, (33.52, 44.62), lies in the Sevastopol Bay notch of the 1:10m
   polygon.

It exits non-zero on any failure. On 2026-10-25 the calibration was exact (WGS84, 0.000 px over
5 parallels and 11 meridians), and the committed extent passed every check.

## Economic-zone cells

The authoring input lives in `crates/infrastructure/authoring/<region>_economic_zones.ron` as
`(zones: [(id: 1, polygons: [[(lon, lat), ...]])])`.

```sh
cargo run -p infrastructure --example author_economic_zone_cells -- \
    assets/regions/<region>.ron crates/infrastructure/authoring/<region>_economic_zones.ron
```

This prints the `(cells: [...])` sidecar that the region's `economic_zones.cells_file` names.
Redirect it to `assets/regions/<region>/economic_zone_cells.ron`. The sidecar has a provenance
header, a comment for each row and 20 values per line. The region's geography includes its
geography sidecars. The tool ignores its current zone cells, so a stale cells sidecar never
blocks regeneration. A shipped-regions test regenerates the sidecar byte for byte. The tool samples cell centres in the terrain the loader builds,
using the loader's even-odd test. A cell that is not playable, or is in no zone, gets 0. A
playable cell that two zones claim is an `InvalidShape { field: "economic_zones.cells" }` error.

## Tests

```sh
python3 -I tools/region_authoring/test_region_authoring.py
cargo nextest run -p infrastructure -E 'test(zone_cells) | test(claimed_by_two_zones) | test(generated_zone_cells)'
```
