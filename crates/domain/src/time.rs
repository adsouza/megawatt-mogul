//! Calendar and simulation clock.
//!
//! One tick is one game minute. A simulated day is 1,440 ticks; the calendar
//! is Gregorian so leap years and month lengths are real.

use crate::error::DomainError;

/// Ticks (game minutes) in one hour.
pub const TICKS_PER_HOUR: u32 = 60;
/// Ticks (game minutes) in one day.
pub const TICKS_PER_DAY: u32 = 1_440;

/// A span of simulated time, stored as whole ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimDuration(u32);

impl SimDuration {
    pub fn from_ticks(ticks: u32) -> Self {
        Self(ticks)
    }

    pub fn from_minutes(minutes: u32) -> Self {
        Self(minutes)
    }

    pub fn from_hours(hours: u32) -> Self {
        Self(hours * TICKS_PER_HOUR)
    }

    pub fn ticks(self) -> u32 {
        self.0
    }
}

/// Seasonal label. Calendar months retain the default northern meteorological seasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Season {
    Winter,
    Spring,
    Summer,
    Autumn,
    Monsoon,
    PostMonsoon,
}

/// A calendar month, always valid (`month` is `1..=12`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimMonth {
    year: i32,
    month: u8,
}

impl SimMonth {
    pub fn new(year: i32, month: u8) -> Result<Self, DomainError> {
        if (1..=12).contains(&month) {
            Ok(Self { year, month })
        } else {
            Err(DomainError::InvalidMonth { month })
        }
    }

    pub fn year(self) -> i32 {
        self.year
    }

    /// Month number, `1..=12`.
    pub fn month(self) -> u8 {
        self.month
    }

    /// Constant-time checked calendar arithmetic for construction quotes and displays.
    pub fn checked_add_months(self, months: u32) -> Result<Self, DomainError> {
        let absolute = i64::from(self.year) * 12 + i64::from(self.month - 1) + i64::from(months);
        let year = i32::try_from(absolute.div_euclid(12))
            .map_err(|_| DomainError::ConstructionOverflow)?;
        Self::new(year, (absolute.rem_euclid(12) + 1) as u8)
    }

    pub fn next(self) -> Self {
        if self.month == 12 {
            Self {
                year: self.year + 1,
                month: 1,
            }
        } else {
            Self {
                year: self.year,
                month: self.month + 1,
            }
        }
    }

    /// Gregorian month length, including leap-year February.
    pub fn days_in_month(self) -> u32 {
        match self.month {
            4 | 6 | 9 | 11 => 30,
            2 if is_leap_year(self.year) => 29,
            2 => 28,
            _ => 31,
        }
    }

    pub fn season(self) -> Season {
        match self.month {
            12 | 1 | 2 => Season::Winter,
            3..=5 => Season::Spring,
            6..=8 => Season::Summer,
            _ => Season::Autumn,
        }
    }

    /// Zero-based month, `0..=11`, for indexing per-month tables.
    pub fn index0(self) -> usize {
        usize::from(self.month - 1)
    }

    /// Whole months from `earlier` to `self` (negative if `self` is earlier).
    pub fn months_since(self, earlier: SimMonth) -> i32 {
        (self.year - earlier.year) * 12 + i32::from(self.month) - i32::from(earlier.month)
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The simulation clock: the current month and the tick within the current day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimClock {
    month: SimMonth,
    tick: u32,
}

impl SimClock {
    pub fn new(month: SimMonth) -> Self {
        Self { month, tick: 0 }
    }

    pub fn month(&self) -> SimMonth {
        self.month
    }

    /// Tick within the current day, `0..=TICKS_PER_DAY`.
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// Hour of the day for the current tick.
    pub fn hour(&self) -> u8 {
        u8::try_from(self.tick / TICKS_PER_HOUR).unwrap_or(u8::MAX)
    }

    /// Advances one tick. Returns true exactly when the tick reaches
    /// `TICKS_PER_DAY`, i.e. the day is complete.
    pub fn advance(&mut self) -> bool {
        self.tick += 1;
        self.tick == TICKS_PER_DAY
    }

    /// Moves to the next month and restarts the day at tick 0.
    pub fn next_month(&mut self) {
        self.month = self.month.next();
        self.tick = 0;
    }
}

// Portable state schema 1: explicit field order and enum tags.
impl crate::state::State for SimClock {
    fn write(&self, out: &mut Vec<u8>) {
        crate::state::State::write(&self.month, out);
        crate::state::State::write(&self.tick, out);
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        let month = crate::state::State::read(input)?;
        let tick = <u32 as crate::state::State>::read(input)?;
        if tick > TICKS_PER_DAY {
            return Err(crate::state::StateError::Invalid);
        }
        Ok(Self { month, tick })
    }
}
impl crate::state::State for SimMonth {
    fn write(&self, out: &mut Vec<u8>) {
        crate::state::State::write(&self.year, out);
        crate::state::State::write(&self.month, out);
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Self::new(
            crate::state::State::read(input)?,
            crate::state::State::read(input)?,
        )
        .map_err(|_| crate::state::StateError::Invalid)
    }
}
impl crate::state::State for Season {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Winter => {
                crate::state::State::write(&0u8, out);
            }
            Self::Spring => {
                crate::state::State::write(&1u8, out);
            }
            Self::Summer => {
                crate::state::State::write(&2u8, out);
            }
            Self::Autumn => {
                crate::state::State::write(&3u8, out);
            }
            Self::Monsoon => {
                crate::state::State::write(&4u8, out);
            }
            Self::PostMonsoon => {
                crate::state::State::write(&5u8, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Winter,
            1 => Self::Spring,
            2 => Self::Summer,
            3 => Self::Autumn,
            4 if input.schema >= 3 => Self::Monsoon,
            5 if input.schema >= 3 => Self::PostMonsoon,
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
crate::state_tuple!(SimDuration);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DomainError;
    use proptest::prelude::*;

    #[test]
    fn season_tags_four_and_five_are_schema_three_only() {
        use crate::state::{State, StateError};
        for (season, tag) in [(Season::Monsoon, 4u8), (Season::PostMonsoon, 5u8)] {
            let bytes = season.state_bytes_schema(3);
            assert_eq!(bytes, vec![tag]);
            assert_eq!(Season::from_state_bytes_schema(&bytes, 3), Ok(season));
            assert_eq!(Season::from_state_bytes(&season.state_bytes()), Ok(season));
            for schema in [1, 2] {
                assert_eq!(
                    Season::from_state_bytes_schema(&bytes, schema),
                    Err(StateError::Invalid)
                );
            }
        }
    }

    proptest! {
        #[test]
        fn season_tag_gate_preserves_legacy_tags(schema in 1u32..=3, tag in any::<u8>()) {
            use crate::state::{State, StateError};
            let expected = match tag {
                0 => Ok(Season::Winter),
                1 => Ok(Season::Spring),
                2 => Ok(Season::Summer),
                3 => Ok(Season::Autumn),
                4 if schema == 3 => Ok(Season::Monsoon),
                5 if schema == 3 => Ok(Season::PostMonsoon),
                _ => Err(StateError::Invalid),
            };
            prop_assert_eq!(Season::from_state_bytes_schema(&[tag], schema), expected);
        }
    }

    #[test]
    fn leap_years_follow_gregorian_rules() {
        let feb = |y| SimMonth::new(y, 2).unwrap().days_in_month();
        assert_eq!(
            (feb(2028), feb(2100), feb(2000), feb(2027)),
            (29, 28, 29, 28)
        );
        assert_eq!(SimMonth::new(2027, 4).unwrap().days_in_month(), 30);
    }

    #[test]
    fn december_rolls_into_january() {
        assert_eq!(
            SimMonth::new(2027, 12).unwrap().next(),
            SimMonth::new(2028, 1).unwrap()
        );
    }

    #[test]
    fn seasons_follow_meteorological_months() {
        let s = |m| SimMonth::new(2027, m).unwrap().season();
        assert_eq!(
            [s(12), s(2), s(3), s(6), s(9)],
            [
                Season::Winter,
                Season::Winter,
                Season::Spring,
                Season::Summer,
                Season::Autumn
            ]
        );
    }

    #[test]
    fn rejects_month_13() {
        assert_eq!(
            SimMonth::new(2027, 13),
            Err(DomainError::InvalidMonth { month: 13 })
        );
    }

    #[test]
    fn rejects_month_0() {
        assert_eq!(
            SimMonth::new(2027, 0),
            Err(DomainError::InvalidMonth { month: 0 })
        );
    }

    #[test]
    fn months_since_counts_across_years() {
        assert_eq!(
            SimMonth::new(2029, 1)
                .unwrap()
                .months_since(SimMonth::new(2027, 3).unwrap()),
            22
        );
    }

    #[test]
    fn index0_is_zero_based_month() {
        assert_eq!(SimMonth::new(2027, 1).unwrap().index0(), 0);
        assert_eq!(SimMonth::new(2027, 12).unwrap().index0(), 11);
    }

    #[test]
    fn clock_reports_day_end_on_tick_1440() {
        let mut c = SimClock::new(SimMonth::new(2027, 1).unwrap());
        let ends: Vec<bool> = (0..TICKS_PER_DAY).map(|_| c.advance()).collect();
        assert!(ends[..1439].iter().all(|e| !e) && ends[1439]);
    }

    #[test]
    fn clock_hour_and_next_month() {
        let mut c = SimClock::new(SimMonth::new(2027, 12).unwrap());
        for _ in 0..125 {
            c.advance();
        }
        assert_eq!((c.tick(), c.hour()), (125, 2));
        c.next_month();
        assert_eq!((c.month(), c.tick()), (SimMonth::new(2028, 1).unwrap(), 0));
    }

    #[test]
    fn durations_convert_to_ticks() {
        assert_eq!(SimDuration::from_hours(2).ticks(), 120);
        assert_eq!(SimDuration::from_minutes(45).ticks(), 45);
        assert_eq!(SimDuration::from_ticks(7).ticks(), 7);
    }

    proptest! {
        #[test]
        fn year_has_365_or_366_days(y in 1900i32..2400) {
            let total: u32 = (1..=12).map(|m| SimMonth::new(y, m).unwrap().days_in_month()).sum();
            let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
            prop_assert_eq!(total, if leap { 366 } else { 365 });
        }
    }
}
