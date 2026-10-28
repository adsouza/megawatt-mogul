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
    pub fn thresholds(&mut self) -> Vec<u8> {
        let relative = self.field.value() / self.initial_field.value();
        let mut crossed = Vec::new();
        for n in 0..9u8 {
            let threshold = 90 - 10 * n;
            let ratio = f64::from(threshold) / 100.0;
            if relative > ratio {
                self.crossed &= !(1 << n);
            }
            if relative < ratio && self.crossed & (1 << n) == 0 {
                self.crossed |= 1 << n;
                self.notice = Some(threshold);
                crossed.push(threshold);
            }
        }
        crossed
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
    }
}
