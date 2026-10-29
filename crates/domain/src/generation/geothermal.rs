//! Persistent geothermal project and reservoir state. No random draws or I/O here.
use crate::{
    region::GeothermalParams,
    units::{Megawatts, Money, Ratio},
};
use std::collections::VecDeque;
#[derive(Debug, Clone, PartialEq)]
pub struct MakeupProject {
    pub months_left: u32,
    pub payments: VecDeque<Money>,
    pub capital: Money,
    pub ceiling: Ratio,
}
#[derive(Debug, Clone, PartialEq)]
pub struct GeothermalState {
    pub exploration_left: u32,
    pub odds: Ratio,
    pub exploration_capital: Money,
    pub initial_field: Ratio,
    pub field: Ratio,
    pub rounds: u32,
    pub makeup: Option<MakeupProject>,
    /// Bit n represents the (90 - 10*n)% threshold, n in 0..9.
    pub crossed: u16,
    pub notice: Option<u8>,
    pub taught: bool,
}
/// How close to a notice line counts as on it, as a share of commissioned output.
const LINE_TOLERANCE: f64 = 1e-9;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MakeupQuote {
    pub capital: Money,
    pub months: u32,
    pub from: Megawatts,
    pub to: Megawatts,
    pub ceiling: Ratio,
}
impl GeothermalState {
    pub fn decline(&mut self, params: &GeothermalParams) {
        self.field = Ratio::new(
            (self.field.value() * (1.0 - params.monthly_decline.value())).max(f64::from_bits(1)),
        )
        .expect("positive bounded field");
    }
    /// Notices for the lines the field has newly fallen below. A field within [`LINE_TOLERANCE`]
    /// of a line is on it: makeup ceilings land exactly on lines (round 2 at 90%), and a restore
    /// onto a line must neither re-arm it nor fire it, whichever way the site's rounding falls.
    pub fn thresholds(&mut self) -> Vec<u8> {
        let relative = self.field.value() / self.initial_field.value();
        let mut crossed = Vec::new();
        for n in 0..9u8 {
            let threshold = 90 - 10 * n;
            let ratio = f64::from(threshold) / 100.0;
            if relative > ratio + LINE_TOLERANCE {
                self.crossed &= !(1 << n);
            }
            if relative < ratio - LINE_TOLERANCE && self.crossed & (1 << n) == 0 {
                self.crossed |= 1 << n;
                self.notice = Some(threshold);
                crossed.push(threshold);
            }
        }
        crossed
    }
    /// The field that makeup round `round` (1-based) restores to: `initial × (1 − round × step)`,
    /// floored at zero. Each round's ceiling is strictly lower until it reaches zero.
    pub fn ceiling(&self, round: u32, params: &GeothermalParams) -> f64 {
        (self.initial_field.value() * (1.0 - f64::from(round) * params.ceiling_step.value()))
            .max(0.0)
    }
    pub fn valid(&self) -> bool {
        self.initial_field > Ratio::ZERO
            && self.field > Ratio::ZERO
            && self.field <= self.initial_field
            && self.crossed < 512
            && self
                .notice
                .is_none_or(|t| (10..=90).contains(&t) && t % 10 == 0)
            && self.makeup.as_ref().is_none_or(|m| {
                m.months_left > 0
                    && m.payments.len() == m.months_left as usize
                    && !m.capital.is_negative()
                    && m.ceiling > Ratio::ZERO
                    && m.ceiling <= self.initial_field
                    && m.payments.iter().all(|p| !p.is_negative())
            })
    }
}
crate::state_record!(MakeupProject {
    months_left,
    payments,
    capital,
    ceiling
});
crate::state_record!(GeothermalState {
    exploration_left,
    odds,
    exploration_capital,
    initial_field,
    field,
    rounds,
    makeup,
    crossed,
    notice,
    taught
});

#[cfg(test)]
mod tests {
    use super::*;
    proptest::proptest! {
        #[test]
        fn geothermal_decline_stays_positive_and_never_increases_even_for_subnormal_fields(bits in 1u64..=1.0f64.to_bits()) {
            let field=Ratio::new(f64::from_bits(bits)).unwrap();
            let mut g=GeothermalState {exploration_left:0,odds:Ratio::ONE,exploration_capital:Money::ZERO,initial_field:field,field,rounds:0,makeup:None,crossed:0,notice:None,taught:false};
            g.decline(&crate::region::fixtures::geothermal_params());
            proptest::prop_assert!(g.field>Ratio::ZERO && g.field<=field);
        }
        /// A makeup ceiling that lands on a notice line (round 2 at 90%, round 4 at 80%) is a tie,
        /// not a restoration above it: whatever the site's float rounding, the crossed notice stays
        /// spent and does not fire again as the field resumes declining.
        #[test]
        fn geothermal_restore_landing_on_a_notice_line_never_rearms_it(initial in 0.15f64..=1.0, round in 1u32..=4) {
            let step = 0.05;
            let initial_field = Ratio::new(initial).unwrap();
            let field = Ratio::new(initial * (1.0 - f64::from(2 * round) * step)).unwrap();
            let line = 90 - 10 * (round as u8 - 1);
            let spent = (0..round as usize).fold(0u16, |bits, n| bits | (1 << n));
            let mut g = GeothermalState {exploration_left:0,odds:Ratio::ONE,exploration_capital:Money::ZERO,initial_field,field,rounds:2 * round,makeup:None,crossed:spent,notice:None,taught:true};
            proptest::prop_assert!(g.thresholds().is_empty(), "restored onto the {}% line", line);
            proptest::prop_assert_eq!(g.crossed, spent);
            g.decline(&crate::region::fixtures::geothermal_params());
            proptest::prop_assert!(g.thresholds().is_empty(), "{}% fired again after the restore", line);
        }
        #[test]
        fn geothermal_makeup_ceilings_fall_strictly_and_never_exceed_the_commissioned_field(
            initial in 0.15f64..=1.0, step in 0.001f64..=0.5, round in 0u32..40,
        ) {
            let mut params = crate::region::fixtures::geothermal_params();
            params.ceiling_step = Ratio::new(step).unwrap();
            let initial_field = Ratio::new(initial).unwrap();
            let g = GeothermalState {exploration_left:0,odds:Ratio::ONE,exploration_capital:Money::ZERO,initial_field,field:initial_field,rounds:round,makeup:None,crossed:0,notice:None,taught:false};
            let (this, next) = (g.ceiling(round, &params), g.ceiling(round + 1, &params));
            proptest::prop_assert!(this <= initial && next >= 0.0);
            proptest::prop_assert!(next < this || this == 0.0, "round {} ceiling {} then {}", round, this, next);
        }
        /// Restoring clearly above a line re-arms it, and the next fall below fires it once.
        #[test]
        fn geothermal_restore_above_a_notice_line_rearms_it_once(initial in 0.15f64..=1.0) {
            let initial_field = Ratio::new(initial).unwrap();
            let mut g = GeothermalState {exploration_left:0,odds:Ratio::ONE,exploration_capital:Money::ZERO,initial_field,field:Ratio::new(initial * 0.95).unwrap(),rounds:1,makeup:None,crossed:1,notice:None,taught:true};
            proptest::prop_assert!(g.thresholds().is_empty());
            proptest::prop_assert_eq!(g.crossed, 0);
            g.field = Ratio::new(initial * 0.89).unwrap();
            proptest::prop_assert_eq!(g.thresholds(), vec![90]);
            proptest::prop_assert!(g.thresholds().is_empty());
        }
    }
}
