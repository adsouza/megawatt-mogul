//! Inspect each shipped region's simultaneous, noise-free demand before tuning milestones.
use application::RegionCatalog;
use domain::{
    ids::RegionId,
    time::{SimMonth, TICKS_PER_DAY},
};
use infrastructure::region::RonRegionCatalog;

fn main() {
    let catalog = RonRegionCatalog::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"),
    );
    for id in [
        "eastern_interconnection",
        "western_interconnection",
        "brazil_sin",
        "continental_europe_maghreb",
        "india_nepal_bhutan",
        "ips_ups",
        "sgcc",
        "southern_china",
    ] {
        let loaded = catalog.load(&RegionId::new(id)).expect("shipped region");
        let region = &loaded.definition;
        let mut all_peak = 0.0f64;
        let mut start_peak = 0.0f64;
        for m in 1..=12 {
            let month = SimMonth::new(region.start().month.year(), m).expect("calendar month");
            for tick in 0..TICKS_PER_DAY {
                let mut all = 0.0;
                let mut start = 0.0;
                for city in region.cities() {
                    let mw = city.base_load.value() * region.load_factor(city.id, month, tick);
                    all += mw;
                    if region.start().connected_cities.contains(&city.id) {
                        start += mw;
                    }
                }
                all_peak = all_peak.max(all);
                start_peak = start_peak.max(start);
            }
        }
        println!(
            "{id}: cities {} → {}; peak {:.3} → {:.3} GW",
            region.start().connected_cities.len(),
            region.cities().len(),
            start_peak / 1000.0,
            all_peak / 1000.0
        );
        println!(
            "  targets: {:?}",
            application::projections::regional_milestones(region)
        );
    }
}
