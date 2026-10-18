//! Value objects: typed physical quantities and exact money.
//!
//! Every quantity is a newtype with a private field; the raw `f64` or `i64`
//! only escapes through an explicit accessor.

use std::iter::Sum;
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use crate::error::DomainError;
use crate::time::TICKS_PER_HOUR;

/// Instantaneous power in megawatts. Signed, so it can express imbalances.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Megawatts(f64);

impl Megawatts {
    pub const ZERO: Self = Self(0.0);

    pub fn new(value: f64) -> Self {
        debug_assert!(value.is_finite(), "Megawatts must be finite, got {value}");
        Self(value)
    }

    pub fn value(self) -> f64 {
        self.0
    }

    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    pub fn clamp(self, lo: Self, hi: Self) -> Self {
        Self(self.0.clamp(lo.0, hi.0))
    }

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// Energy produced by holding this power for `ticks` game minutes.
    pub fn energy_over_ticks(self, ticks: u32) -> MegawattHours {
        MegawattHours(self.0 * f64::from(ticks) / f64::from(TICKS_PER_HOUR))
    }
}

impl Add for Megawatts {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.0 + rhs.0)
    }
}

impl Sub for Megawatts {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.0 - rhs.0)
    }
}

impl Neg for Megawatts {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl AddAssign for Megawatts {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl SubAssign for Megawatts {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl Mul<f64> for Megawatts {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.0 * rhs)
    }
}

impl Div<Megawatts> for Megawatts {
    type Output = f64;
    fn div(self, rhs: Megawatts) -> f64 {
        self.0 / rhs.0
    }
}

impl Sum for Megawatts {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, |acc, x| acc + x)
    }
}

/// Energy in megawatt-hours.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct MegawattHours(f64);

impl MegawattHours {
    pub const ZERO: Self = Self(0.0);

    pub fn new(value: f64) -> Self {
        Self(value)
    }

    pub fn value(self) -> f64 {
        self.0
    }
}

impl Add for MegawattHours {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for MegawattHours {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl AddAssign for MegawattHours {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Mul<f64> for MegawattHours {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self(self.0 * rhs)
    }
}

impl Sum for MegawattHours {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, |acc, x| acc + x)
    }
}

/// Grid frequency (or a deviation from nominal) in hertz.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Hertz(f64);

impl Hertz {
    pub const ZERO: Self = Self(0.0);

    pub fn new(value: f64) -> Self {
        Self(value)
    }

    pub fn value(self) -> f64 {
        self.0
    }
}

impl Add for Hertz {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Hertz {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl Neg for Hertz {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

/// Exact money in whole cents. Integer arithmetic throughout: rounding to
/// cents happens only in [`Price::cost_of`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Self = Self(0);

    pub const fn from_cents(cents: i64) -> Self {
        Self(cents)
    }

    pub const fn from_dollars(dollars: i64) -> Self {
        Self(dollars * 100)
    }

    pub const fn cents(self) -> i64 {
        self.0
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Splits into `parts` equal amounts, with the remainder on the last part,
    /// so the parts always sum exactly to `self`.
    ///
    /// # Panics
    ///
    /// Panics if `parts == 0`.
    pub fn split_even(self, parts: u32) -> Vec<Money> {
        assert!(parts > 0, "cannot split money into zero parts");
        let n = i64::from(parts);
        let base = self.0 / n;
        let last = self.0 - base * (n - 1);
        let mut out = vec![Money(base); parts as usize - 1];
        out.push(Money(last));
        out
    }
}

impl Add for Money {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Money {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl Neg for Money {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul<i64> for Money {
    type Output = Self;
    fn mul(self, rhs: i64) -> Self {
        Self(self.0 * rhs)
    }
}

impl Sum for Money {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, |acc, x| acc + x)
    }
}

/// A price in dollars per megawatt-hour.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Price(f64);

impl Price {
    pub fn per_mwh(dollars: f64) -> Self {
        Self(dollars)
    }

    pub fn value(self) -> f64 {
        self.0
    }

    /// Cost of `energy` repeated `weight` times (e.g. days in a month).
    /// Weighting happens before rounding; this is the single rounding step,
    /// to whole cents with ties to even.
    pub fn cost_of(self, energy: MegawattHours, weight: u32) -> Money {
        let cents = self.0 * energy.value() * f64::from(weight) * 100.0;
        Money(cents.round_ties_even() as i64)
    }
}

/// How fast a plant can change output, in megawatts per game minute.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct RampRate(f64);

impl RampRate {
    pub fn mw_per_minute(mw: f64) -> Self {
        Self(mw)
    }

    /// Change achievable in one tick (one game minute).
    pub fn per_tick(self) -> Megawatts {
        Megawatts::new(self.0)
    }

    pub fn over_ticks(self, ticks: u32) -> Megawatts {
        self.per_tick() * f64::from(ticks)
    }
}

/// A fraction in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Ratio(f64);

impl Ratio {
    pub const ZERO: Self = Self(0.0);
    pub const ONE: Self = Self(1.0);

    /// Rejects NaN and anything outside `0.0..=1.0`.
    pub fn new(value: f64) -> Result<Self, DomainError> {
        if (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(DomainError::InvalidRatio { value })
        }
    }

    pub fn value(self) -> f64 {
        self.0
    }

    pub fn of(self, power: Megawatts) -> Megawatts {
        power * self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn money_from_dollars_is_cents() {
        assert_eq!(Money::from_dollars(3).cents(), 300);
    }

    #[test]
    fn cost_rounds_half_even_once() {
        let one = MegawattHours::new(1.0);
        assert_eq!(Price::per_mwh(0.125).cost_of(one, 1), Money::from_cents(12));
        assert_eq!(Price::per_mwh(0.375).cost_of(one, 1), Money::from_cents(38));
    }

    #[test]
    fn cost_weights_before_rounding() {
        // per-day rounding would give 0 × 31 = 0; weighting first gives 3.1 → 3 cents
        assert_eq!(
            Price::per_mwh(0.001).cost_of(MegawattHours::new(1.0), 31),
            Money::from_cents(3)
        );
    }

    #[test]
    fn energy_over_ticks_is_minutes() {
        assert_eq!(
            Megawatts::new(120.0).energy_over_ticks(30),
            MegawattHours::new(60.0)
        );
    }

    #[test]
    fn ramp_per_tick_equals_per_minute() {
        assert_eq!(RampRate::mw_per_minute(5.0).per_tick(), Megawatts::new(5.0));
    }

    proptest! {
        #[test]
        fn split_even_sums_exactly(cents in 0i64..1_000_000_000_000, parts in 1u32..=120) {
            let split = Money::from_cents(cents).split_even(parts);
            prop_assert_eq!(split.len() as u32, parts);
            prop_assert_eq!(split.iter().copied().sum::<Money>(), Money::from_cents(cents));
            prop_assert!(split[..split.len() - 1].iter().all(|p| *p == split[0]));
        }

        #[test]
        fn ratio_accepts_exactly_unit_interval(v in -10.0f64..10.0) {
            prop_assert_eq!(Ratio::new(v).is_ok(), (0.0..=1.0).contains(&v));
        }
    }

    #[test]
    fn ratio_rejects_nan() {
        assert!(Ratio::new(f64::NAN).is_err());
    }

    #[test]
    #[should_panic(expected = "zero parts")]
    fn split_even_zero_parts_panics() {
        let _ = Money::from_cents(100).split_even(0);
    }

    #[test]
    fn split_even_puts_remainder_on_last_part() {
        assert_eq!(
            Money::from_cents(10).split_even(3),
            vec![
                Money::from_cents(3),
                Money::from_cents(3),
                Money::from_cents(4)
            ]
        );
    }

    #[test]
    fn ratio_accepts_both_bounds_and_reports_rejected_value() {
        assert_eq!(Ratio::new(0.0), Ok(Ratio::ZERO));
        assert_eq!(Ratio::new(1.0), Ok(Ratio::ONE));
        assert_eq!(
            Ratio::new(1.5),
            Err(DomainError::InvalidRatio { value: 1.5 })
        );
    }

    #[test]
    fn ratio_of_scales_power() {
        assert_eq!(
            Ratio::new(0.25).unwrap().of(Megawatts::new(80.0)),
            Megawatts::new(20.0)
        );
    }

    #[test]
    fn megawatts_min_max_clamp_abs() {
        let (a, b) = (Megawatts::new(-3.0), Megawatts::new(5.0));
        assert_eq!(a.min(b), a);
        assert_eq!(a.max(b), b);
        assert_eq!(Megawatts::new(9.0).clamp(a, b), b);
        assert_eq!(a.abs(), Megawatts::new(3.0));
        assert_eq!(b / Megawatts::new(2.0), 2.5);
    }

    #[test]
    fn ramp_over_ticks_scales_linearly() {
        assert_eq!(
            RampRate::mw_per_minute(5.0).over_ticks(4),
            Megawatts::new(20.0)
        );
    }

    #[test]
    fn megawatt_operators_follow_arithmetic() {
        let (a, b) = (Megawatts::new(7.0), Megawatts::new(2.5));
        assert_eq!(a + b, Megawatts::new(9.5));
        assert_eq!(a - b, Megawatts::new(4.5));
        assert_eq!(-a, Megawatts::new(-7.0));
        assert_eq!(a * 3.0, Megawatts::new(21.0));
        let mut c = a;
        c += b;
        assert_eq!(c, Megawatts::new(9.5));
        c -= Megawatts::new(4.0);
        assert_eq!(c, Megawatts::new(5.5));
        let total: Megawatts = [a, b, Megawatts::new(0.5)].into_iter().sum();
        assert_eq!(total, Megawatts::new(10.0));
        assert_eq!(a.value(), 7.0);
    }

    #[test]
    fn energy_operators_follow_arithmetic() {
        let (a, b) = (MegawattHours::new(7.0), MegawattHours::new(2.5));
        assert_eq!(a + b, MegawattHours::new(9.5));
        assert_eq!(a - b, MegawattHours::new(4.5));
        assert_eq!(a * 2.0, MegawattHours::new(14.0));
        let mut c = a;
        c += b;
        assert_eq!(c, MegawattHours::new(9.5));
        let total: MegawattHours = [a, b].into_iter().sum();
        assert_eq!(total, MegawattHours::new(9.5));
        assert_eq!(a.value(), 7.0);
    }

    #[test]
    fn hertz_operators_follow_arithmetic() {
        let (a, b) = (Hertz::new(60.0), Hertz::new(0.25));
        assert_eq!(a + b, Hertz::new(60.25));
        assert_eq!(a - b, Hertz::new(59.75));
        assert_eq!(-b, Hertz::new(-0.25));
        assert_eq!(b.value(), 0.25);
        assert_eq!(Hertz::ZERO, Hertz::new(0.0));
    }

    #[test]
    fn money_operators_follow_arithmetic() {
        let (a, b) = (Money::from_cents(700), Money::from_cents(250));
        assert_eq!(a + b, Money::from_cents(950));
        assert_eq!(a - b, Money::from_cents(450));
        assert_eq!(-a, Money::from_cents(-700));
        assert_eq!(a * 3, Money::from_cents(2_100));
        let mut c = a;
        c += b;
        assert_eq!(c, Money::from_cents(950));
        c -= Money::from_cents(400);
        assert_eq!(c, Money::from_cents(550));
        let total: Money = [a, b, Money::from_cents(50)].into_iter().sum();
        assert_eq!(total, Money::from_cents(1_000));
    }

    #[test]
    fn money_is_negative_only_below_zero() {
        assert!(Money::from_cents(-1).is_negative());
        assert!(!Money::ZERO.is_negative());
        assert!(!Money::from_cents(1).is_negative());
    }

    #[test]
    fn price_value_is_dollars_per_mwh() {
        assert_eq!(Price::per_mwh(42.5).value(), 42.5);
    }

    #[test]
    fn split_even_rounds_toward_zero_with_remainder_last_for_negatives() {
        assert_eq!(
            Money::from_cents(-10).split_even(3),
            vec![
                Money::from_cents(-3),
                Money::from_cents(-3),
                Money::from_cents(-4)
            ]
        );
        assert_eq!(
            Money::from_cents(-7).split_even(1),
            vec![Money::from_cents(-7)]
        );
    }

    proptest! {
        #[test]
        fn split_even_sums_exactly_for_any_sign(cents in -1_000_000_000_000i64..1_000_000_000_000, parts in 1u32..=120) {
            let split = Money::from_cents(cents).split_even(parts);
            prop_assert_eq!(split.len() as u32, parts);
            prop_assert_eq!(split.iter().copied().sum::<Money>(), Money::from_cents(cents));
        }
    }
}
