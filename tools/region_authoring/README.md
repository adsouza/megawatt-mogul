# Region authoring tools

Offline helpers for writing `assets/regions/*.ron`. Nothing here runs in the game or in CI's
Rust build. Their output is reviewed and pasted into region files by hand.

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
    [--extent] [--comment TEXT ...]
```

Write `--bounds=` with `=`, because argparse reads a leading `-20,...` as an option.

It prints a provenance comment and then the RON `land: [...]` and `exclude: [...]` lists, as
`(lon, lat)` with 3 decimals.

- `--grid` sets the cell size. The default Douglas–Peucker tolerance is half a cell. After
  clipping to the bounds, land rings under 2 cells' area are dropped. Exclude rings are never
  dropped, because dropping one would turn its area back into land.
- `--dissolve` merges rings that share borders before simplifying. Without it, neighbours are
  simplified separately, and the two simplified borders open slivers between them. The merge
  relies on Natural Earth's exact shared vertices, and it refuses rings that touch at a point or
  overlap.
- `--extent` prints only the exclusions, as a standalone `(polygons: [...])` file. `--comment`
  adds header lines. The header always ends with the generating command, without the comments.
- `ISO:<A3>` matches admin-0 `ISO_A3`, falling back to `ISO_A3_EH` (France and Norway carry
  `-99`). `ADM0:<A3>` matches `ADM0_A3`, for Kosovo and Northern Cyprus.
- `<A3>:<name>` matches an admin-1 `name` or `name_en`. Its country is the feature's `adm0_a3` or
  the country of its ISO 3166-2 code, so `UKR:Crimea` works although Natural Earth files Crimea
  under `RUS`. Every selector must match, and an admin-1 selector must match exactly one feature.
- Holes in included countries become `exclude` rings, unless another included feature fills
  them (an enclave).

## The Ukraine–IPS operating extent

`crates/infrastructure/authoring/ukraine_ips_operating_extent.ron` is the canonical exclusion
that Continental Europe–Maghreb and IPS/UPS share. It holds Natural Earth 1:10m admin-1 "Crimea"
and "Sevastopol", dissolved into one outline. It is a schematic operating extent per the research
notes and takes no sovereignty position.

The research map marks Crimea's connection with a violet point and draws no wider occupied
extent, so mainland Ukraine, Donetsk and Luhansk included, stays in Continental Europe–Maghreb.
The file is regenerated with:

```sh
python3 -I tools/region_authoring/natural_earth.py \
    --admin0 target/natural-earth/ne_50m_admin_0_countries.geojson \
    --admin1 target/natural-earth/ne_10m_admin_1_states_provinces.geojson \
    --exclude-admin1 "UKR:Crimea,UKR:Sevastopol" \
    --bounds=32.0,37.2,44.1,46.5 --grid 13,8 --dissolve --extent \
    --comment "..." > crates/infrastructure/authoring/ukraine_ips_operating_extent.ron
```

The file's header gives the full `--comment` text. The 13 × 8 grid gives the finer of the two
regions' cells (Continental Europe–Maghreb, about 0.4° × 0.3°). The tolerance is half of that
cell (0.15°), as for land. At IPS/UPS's 0.25° half cell, Kerch would fall outside the outline.

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

This prints the `cells: [...]` member of the region's `economic_zones` block, with a comment for
each row and 20 values per line. The tool samples cell centres in the terrain the loader builds,
using the loader's even-odd test. A cell that is not playable, or is in no zone, gets 0. A
playable cell that two zones claim is an `InvalidShape { field: "economic_zones.cells" }` error.

## Tests

```sh
python3 -I tools/region_authoring/test_region_authoring.py
cargo nextest run -p infrastructure -E 'test(zone_cells) | test(claimed_by_two_zones) | test(generated_zone_cells)'
```
