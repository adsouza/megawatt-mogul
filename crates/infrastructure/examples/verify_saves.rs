//! Read-only validation of the saves visible to the shipped game; no recovery or publication.
use application::{InMemoryCommandLog, SaveStore};
use infrastructure::save::FileSaveStore;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("usage: verify_saves <save-directory>")?;
    let store = FileSaveStore::new(root.into());
    let mut verified = 0;
    for row in store.list()? {
        let entry = row.entry?;
        let id = entry.metadata.slot;
        let revision = store.revision(&id)?;
        let (bundle, region) = store.read(&id)?;
        let mut expected = bundle.checkpoint.clone();
        let game = bundle.restore(
            region,
            &store.compatibility(),
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )?;
        if expected.snapshot.is_some() {
            // A portable restore adopts today's rules for future commands only.
            // Every byte of saved state/history and every other header field must match.
            expected.header.sim_rules_version = domain::SIM_RULES_VERSION;
            assert_eq!(
                game.checkpoint(),
                expected,
                "snapshot boundary changed during restoration"
            );
        } else {
            // Current-rule command-only recordings still have a verified diagnostic reader.
            assert_eq!(game.session().state_hash(), expected.state_hash);
            assert_eq!(game.checkpoint().summary, expected.summary);
        }
        assert_eq!(
            store.revision(&id)?,
            revision,
            "source changed during verification"
        );
        println!(
            "VERIFIED {} {:?}; commands={}; state_hash={}; cash_cents={}; plants={}; connected_cities={}; reports={}",
            id.as_str(),
            expected.summary,
            expected.command_count,
            expected.state_hash,
            game.session().company_view().cash.cents(),
            game.session().plant_views().len(),
            game.session()
                .city_views()
                .iter()
                .filter(|c| c.connected)
                .count(),
            game.projections().monthly_reports.history().len()
        );
        verified += 1;
    }
    println!(
        "Verified {verified} visible snapshots; no save files changed (disposable metadata cache may refresh)"
    );
    Ok(())
}
