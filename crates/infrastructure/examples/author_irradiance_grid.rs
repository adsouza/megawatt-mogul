//! Offline authoring only: emit a v4 irradiance cells sidecar from immutable legacy potential.
//! Usage: author_irradiance_grid <legacy.ron> <schema4.ron> <potential-scale>
//! Inputs are read-only; stdout is the `(cells: [...])` sidecar the region's
//! `irradiance.cells_file` names (spec §28.11.1), normally
//! `assets/regions/<region>/irradiance_cells.ron`. The schema-4 geography includes its geography
//! sidecars. The source and destination playable cells must match.
use std::io::Write as _;
use std::path::Path;

use domain::ids::{CellCoord, RegionId};
use infrastructure::region::{
    Field, IrradianceV4, ParsedRegionFile, RegionFileV4, RonRegionCatalog, format_cells_sidecar,
    load_region_bytes, map_region_v4,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: author_irradiance_grid <legacy.ron> <schema4.ron> <potential-scale>".into(),
        );
    }
    let scale: f64 = args[2].parse()?;
    if !scale.is_finite() || scale <= 0.0 {
        return Err("potential scale must be finite and positive".into());
    }
    let path = Path::new(&args[1]);
    let mut file: RegionFileV4 = ron::from_str(&std::fs::read_to_string(path)?)?;
    let legacy = load_region_bytes(std::fs::read(&args[0])?, &RegionId::new(&file.id))?;
    // Reproduce the historical Plan-6 migration; A16 calibration is a separate authoring step.
    file.solar.calibration = infrastructure::region::SolarCalibrationDto::Legacy;
    file.irradiance = IrradianceV4::Gaussian(Field {
        base: 1.0,
        blobs: Vec::new(),
    });
    let catalog = RonRegionCatalog::new(path.parent().unwrap_or(Path::new(".")));
    let ParsedRegionFile::V4(mut file) =
        catalog.resolve(&RegionId::new(&file.id), ParsedRegionFile::V4(file))?
    else {
        return Err("a schema-4 region resolved to another schema".into());
    };
    let unit = map_region_v4(file.clone())?;
    let before = &legacy.definition;
    let (width, height) = (unit.terrain().width(), unit.terrain().height());
    if unit.geo() != before.geo()
        || (width, height) != (before.terrain().width(), before.terrain().height())
    {
        return Err("source and destination geography must match".into());
    }
    if !matches!(
        before.solar().model,
        domain::region::SolarModel::DaylightTable(_)
    ) {
        return Err("source must retain legacy daylight-table semantics".into());
    }
    let mut values = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let cell = CellCoord { x, y };
            if unit.terrain().get(cell).unwrap().land != before.terrain().get(cell).unwrap().land {
                return Err("source and destination playable cells must match".into());
            }
            // A unit-irradiance v4 region exposes the actual domain row factor.
            let factor = unit.solar_potential(cell).value();
            let value = before.solar_potential(cell).value() * scale / factor;
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(
                    format!("scale {scale} cannot fit cell {cell:?}: irradiance={value}").into(),
                );
            }
            values.push(value);
        }
    }
    file.irradiance = IrradianceV4::Cells(values.clone());
    // Round-trip the authored f64 values through the real RON loader before emitting.
    let loaded = load_region_bytes(
        ron::to_string(&file)?.into_bytes(),
        &RegionId::new(&file.id),
    )?;
    let mut ranked = Vec::new();
    let mut maximum_error = 0.0_f64;
    for y in 0..height {
        for x in 0..width {
            let cell = CellCoord { x, y };
            let old = before.solar_potential(cell).value();
            let new = loaded.definition.solar_potential(cell).value();
            maximum_error = maximum_error.max((new - old * scale).abs());
            if before.terrain().get(cell).unwrap().land {
                ranked.push((old, new));
            }
        }
    }
    if maximum_error > f64::EPSILON * scale.max(1.0) * 4.0 {
        return Err(format!("authored target error too large: {maximum_error}").into());
    }
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let groups: Vec<_> = ranked.chunk_by(|a, b| a.0 == b.0).collect();
    let extrema: Vec<_> = groups
        .iter()
        .map(|g| {
            g.iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), v| {
                    (min.min(v.1), max.max(v.1))
                })
        })
        .collect();
    if extrema.windows(2).any(|g| g[0].1 >= g[1].0) {
        return Err("authored grid does not preserve every strict playable-site order".into());
    }
    let tie_spread = extrema.iter().map(|g| g.1 - g.0).fold(0.0_f64, f64::max);
    eprintln!(
        "{}: {} cells, {} playable, {} rank groups, scale {scale}, maximum target error {maximum_error:e}, maximum old-tie spread {tie_spread:e}; all strict ranks preserved",
        file.id,
        values.len(),
        ranked.len(),
        groups.len()
    );
    let fingerprint: String = legacy
        .fingerprint
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let header = [
        format!(
            "Offline author_irradiance_grid: immutable {}-v3.ron",
            file.id
        ),
        format!("Source SHA256: {fingerprint}"),
        format!("Annual potential target = legacy irradiance * {scale}; inverse latitude factor"),
        "is baked into site quality. No runtime calibration. Row-major, north first.".to_owned(),
    ];
    let output = format_cells_sidecar(&header, &values, width, 5);
    std::io::stdout().write_all(output.as_bytes())?;
    Ok(())
}
