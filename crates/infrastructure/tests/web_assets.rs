use application::RegionCatalog;
use infrastructure::region::{BundleRegionCatalog, RonRegionCatalog};

#[test]
fn packaged_catalog_preserves_every_shipped_region_bundle() {
    let native = RonRegionCatalog::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"),
    );
    let listings = native.list().unwrap();
    let bundles = listings
        .iter()
        .map(|l| {
            let loaded = native.load(&l.id).unwrap();
            (l.id.clone(), loaded.source_bytes)
        })
        .collect();
    let browser = BundleRegionCatalog::new(bundles);
    assert_eq!(browser.list().unwrap(), listings);
    for listing in listings {
        let a = native.load(&listing.id).unwrap();
        let b = browser.load(&listing.id).unwrap();
        assert_eq!(a.source_bytes, b.source_bytes);
        assert_eq!(a.fingerprint, b.fingerprint);
    }
}
