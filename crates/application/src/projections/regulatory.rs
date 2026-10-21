//! What the regulator did when the latest month closed: strikes (with their
//! reasons), formal notices, lapsed strikes and a revoked licence.
//!
//! The report's `frequency_notice` flag and the `FrequencyPerformance`
//! lesson can't say why a strike was issued (a month that failed both tests
//! draws one strike, for unserved energy), so the review reads the events.

use domain::events::DomainEvent;
use domain::session::GameSession;

use super::Projection;

/// The regulator's events of the latest closed month, in the order issued.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegulatoryProjection {
    latest: Vec<DomainEvent>,
    /// Issued since the last month closed: they belong to the month closing.
    pending: Vec<DomainEvent>,
}

impl RegulatoryProjection {
    /// Empty until a month closes, and for a month the regulator let pass.
    pub fn latest(&self) -> &[DomainEvent] {
        &self.latest
    }
}

/// Whether `event` is the regulator acting on the company.
fn is_regulatory(event: &DomainEvent) -> bool {
    matches!(
        event,
        DomainEvent::StrikeIssued { .. }
            | DomainEvent::FrequencyNoticeIssued { .. }
            | DomainEvent::StrikeExpired { .. }
            | DomainEvent::LicenseRevoked
    )
}

impl Projection for RegulatoryProjection {
    fn apply(&mut self, event: &DomainEvent, _session: &GameSession) {
        match event {
            DomainEvent::MonthClosed(_) => self.latest = std::mem::take(&mut self.pending),
            event if is_regulatory(event) => self.pending.push(event.clone()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use domain::regulation::StrikeReason;
    use domain::time::SimMonth;

    use super::*;
    use crate::projections::test_support::{report, session};

    fn close(projection: &mut RegulatoryProjection, month: u8) {
        let event = DomainEvent::MonthClosed(Box::new(report(2027, month, 100.0)));
        projection.apply(&event, &session());
    }

    fn strike(reason: StrikeReason) -> DomainEvent {
        DomainEvent::StrikeIssued { strikes: 1, reason }
    }

    #[test]
    fn the_regulators_events_belong_to_the_month_that_closes_after_them() {
        let session = session();
        let mut projection = RegulatoryProjection::default();
        let notice = DomainEvent::FrequencyNoticeIssued {
            month: SimMonth::new(2027, 3).unwrap(),
        };
        let strike = strike(StrikeReason::UnservedEnergy);
        for event in [&notice, &DomainEvent::Blackout, &strike] {
            projection.apply(event, &session);
        }
        assert_eq!(projection.latest(), [], "nothing until the month closes");

        close(&mut projection, 3);

        assert_eq!(projection.latest(), [notice, strike]);
    }

    #[test]
    fn a_clean_month_clears_the_last_months_events() {
        let session = session();
        let mut projection = RegulatoryProjection::default();
        for event in [
            strike(StrikeReason::FrequencyPerformance),
            DomainEvent::StrikeExpired { strikes: 0 },
            DomainEvent::LicenseRevoked,
        ] {
            projection.apply(&event, &session);
        }
        close(&mut projection, 3);
        assert_eq!(projection.latest().len(), 3);

        close(&mut projection, 4);

        assert_eq!(projection.latest(), []);
    }
}
