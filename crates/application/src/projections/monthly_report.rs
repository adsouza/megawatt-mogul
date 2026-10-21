//! Every closed month's report, oldest first.

use domain::events::DomainEvent;
use domain::session::{GameSession, MonthlyReport};

use super::Projection;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MonthlyReportProjection {
    reports: Vec<MonthlyReport>,
}

impl MonthlyReportProjection {
    pub fn latest(&self) -> Option<&MonthlyReport> {
        self.reports.last()
    }

    pub fn history(&self) -> &[MonthlyReport] {
        &self.reports
    }
}

impl Projection for MonthlyReportProjection {
    fn apply(&mut self, event: &DomainEvent, _session: &GameSession) {
        if let DomainEvent::MonthClosed(report) = event {
            self.reports.push(report.as_ref().clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projections::test_support::{report, session};

    #[test]
    fn monthly_report_is_stored_on_close() {
        let session = session();
        let mut projection = MonthlyReportProjection::default();
        assert_eq!(projection.latest(), None);
        let march = report(2027, 3, 100.0);
        projection.apply(&DomainEvent::MonthClosed(Box::new(march.clone())), &session);
        assert_eq!(projection.latest(), Some(&march));
        assert_eq!(projection.history(), std::slice::from_ref(&march));
    }

    #[test]
    fn history_is_oldest_first_and_latest_is_the_newest() {
        let session = session();
        let mut projection = MonthlyReportProjection::default();
        let march = report(2027, 3, 100.0);
        let april = report(2027, 4, 200.0);
        for r in [&march, &april] {
            projection.apply(&DomainEvent::MonthClosed(Box::new(r.clone())), &session);
        }
        assert_eq!(projection.history(), [march, april.clone()]);
        assert_eq!(projection.latest(), Some(&april));
    }
}
