//! Display formatting for en-US: pure functions from typed values to short text.
//!
//! Fluent's own `NUMBER` needs ICU data this crate set doesn't ship, so numbers are formatted
//! here and passed to messages as ready-made text (`FluentArg::Text`).

use domain::time::{SimDuration, SimMonth, TICKS_PER_HOUR};
use domain::units::{Hertz, MegawattHours, Megawatts, Money, Price, RampRate, Ratio};

/// Localized currency affixes, resolved once for the active region and font set.
#[derive(bevy::prelude::Resource, Debug, Clone, PartialEq, Eq)]
pub struct MoneyStyle {
    pub prefix: String,
    pub suffix: String,
}

impl MoneyStyle {
    pub fn resolve(
        currency: domain::units::Currency,
        l10n: &super::Localizer,
        has_glyph: impl Fn(char) -> bool,
    ) -> Self {
        use domain::units::Currency::*;
        let (symbol_key, prefix_key, suffix_key) = match currency {
            Usd => ("currency-usd", "currency-usd-prefix", "currency-usd-suffix"),
            Eur => ("currency-eur", "currency-eur-prefix", "currency-eur-suffix"),
            Cny => ("currency-cny", "currency-cny-prefix", "currency-cny-suffix"),
            Inr => ("currency-inr", "currency-inr-prefix", "currency-inr-suffix"),
            Rub => ("currency-rub", "currency-rub-prefix", "currency-rub-suffix"),
            Brl => ("currency-brl", "currency-brl-prefix", "currency-brl-suffix"),
        };
        let symbol = l10n.text(symbol_key);
        if !symbol.chars().all(has_glyph) {
            return Self {
                prefix: format!("{} ", currency.iso_code()),
                suffix: String::new(),
            };
        }
        Self {
            prefix: l10n.text(prefix_key),
            suffix: l10n.text(suffix_key),
        }
    }

    /// Checks the same proportional font family used by the game's money labels.
    pub fn resolve_egui(
        currency: domain::units::Currency,
        l10n: &super::Localizer,
        ctx: &bevy_egui::egui::Context,
    ) -> Self {
        Self::resolve(currency, l10n, |c| {
            ctx.fonts_mut(|fonts| fonts.has_glyph(&bevy_egui::egui::FontId::proportional(14.0), c))
        })
    }

    pub fn symbol(&self) -> String {
        format!("{}{}", self.prefix, self.suffix).trim().to_owned()
    }
}

/// U+2212, the typographic minus: it lines up with the plus sign and cannot be mistaken for a
/// hyphen.
const MINUS: &str = "\u{2212}";

/// From this magnitude, power and energy switch from MW(h) to GW(h).
const GIGA_THRESHOLD: f64 = 10_000.0;

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `$1.23M`, `$45.6k`, `$845.20`, `−$2.00B`: three significant digits from $1,000 up.
pub fn money(amount: Money, style: &MoneyStyle) -> String {
    let MoneyStyle { prefix, suffix } = style;
    let sign = if amount.is_negative() { MINUS } else { "" };
    let cents = amount.cents().unsigned_abs();
    if cents < 100_000 {
        return format!("{sign}{prefix}{}.{:02}{suffix}", cents / 100, cents % 100);
    }
    let mut value = cents as f64 / 100.0 / 1e3;
    let mut shown = three_significant(value);
    let mut unit = 0;
    while shown.value >= 1_000.0 && unit < 2 {
        value /= 1e3;
        shown = three_significant(value);
        unit += 1;
    }
    format!(
        "{sign}{prefix}{}{}{suffix}",
        shown.render(),
        ["k", "M", "B"][unit]
    )
}

/// `1,234 MW`, or `12.3 GW` from 10,000 MW.
pub fn megawatts(power: Megawatts) -> String {
    scaled(power.value(), "MW", "GW")
}

/// `power` in the unit `like` would be shown in, so two figures side by side compare at a glance:
/// `0.0 GW of 50.0 GW`, not `0.0 MW of 50.0 GW`.
pub fn megawatts_like(power: Megawatts, like: Megawatts) -> String {
    scaled_like(power.value(), like.value(), "MW", "GW")
}

/// `1,234 MWh`, or `12.3 GWh` from 10,000 MWh.
pub fn megawatt_hours(energy: MegawattHours) -> String {
    scaled(energy.value(), "MWh", "GWh")
}

/// `12 MW/min`: how fast a plant can change its output, per game minute.
pub fn ramp_rate(rate: RampRate) -> String {
    scaled(rate.value(), "MW/min", "GW/min")
}

/// `+0.12 Hz`: always signed, so a deviation reads as a deviation. Zero has no sign.
pub fn hertz_deviation(deviation: Hertz) -> String {
    let hundredths = (deviation.value() * 100.0).round();
    let sign = if hundredths > 0.0 {
        "+"
    } else if hundredths < 0.0 {
        MINUS
    } else {
        ""
    };
    format!("{sign}{:.2} Hz", hundredths.abs() / 100.0)
}

/// `99.4%`.
pub fn percent(share: Ratio) -> String {
    format!("{:.1}%", share.value() * 100.0)
}

/// A margin that can be negative: `+18.0%`, `−5.2%`. A margin that rounds to zero shows no sign.
pub fn signed_percent(fraction: f64) -> String {
    let tenths = (fraction * 1000.0).round();
    let sign = if tenths > 0.0 {
        "+"
    } else if tenths < 0.0 {
        MINUS
    } else {
        ""
    };
    format!("{sign}{:.1}%", tenths.abs() / 10.0)
}

/// A return adjustment in percentage points, with no sign on rounded zero.
pub fn signed_points(fraction: f64) -> String {
    let hundredths = (fraction * 10_000.0).round();
    let sign = if hundredths > 0.0 {
        "+"
    } else if hundredths < 0.0 {
        MINUS
    } else {
        ""
    };
    format!("{sign}{:.2} pp", hundredths.abs() / 100.0)
}

/// A power as the bare number an edit field holds: `300`, or `301.5` when it has a fraction. No
/// unit and no separators, so what is shown can be typed back.
pub fn plain_megawatts(power: Megawatts) -> String {
    let value = power.value();
    if (value - value.round()).abs() < 0.05 {
        format!("{:.0}", value.round())
    } else {
        format!("{value:.1}")
    }
}

/// A 0–100 score such as satisfaction, to the nearest whole point: `72`.
pub fn score(value: f64) -> String {
    format!("{:.0}", value.clamp(0.0, 100.0))
}

/// `$45.00/MWh`.
pub fn price(price: Price, style: &MoneyStyle) -> String {
    let MoneyStyle { prefix, suffix } = style;
    let cents = (price.value().abs() * 100.0).round() as u64;
    let sign = if price.value() < 0.0 && cents > 0 {
        MINUS
    } else {
        ""
    };
    format!(
        "{sign}{prefix}{}.{:02}{suffix}/MWh",
        group_thousands(cents / 100),
        cents % 100
    )
}

/// The time of day of a tick (game minute): `17:40`. The day's last tick is `24:00`.
pub fn clock(tick: u32) -> String {
    format!("{:02}:{:02}", tick / TICKS_PER_HOUR, tick % TICKS_PER_HOUR)
}

/// `March 2027`.
pub fn month_year(month: SimMonth) -> String {
    format!("{} {}", MONTH_NAMES[month.index0()], month.year())
}

/// `Mar 2027`: the month's first three letters, for labels that must fit a narrow slot.
pub fn month_year_short(month: SimMonth) -> String {
    let name = MONTH_NAMES[month.index0()];
    format!("{} {}", &name[..3], month.year())
}

/// `2 h 30 min`, `48 h`, `40 min`.
pub fn duration(span: SimDuration) -> String {
    let (hours, minutes) = (span.ticks() / TICKS_PER_HOUR, span.ticks() % TICKS_PER_HOUR);
    match (hours, minutes) {
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

/// A value rounded to three significant digits, with the decimals that took.
struct Rounded {
    value: f64,
    decimals: usize,
}

impl Rounded {
    fn render(&self) -> String {
        if self.decimals == 0 {
            group_thousands(self.value as u64)
        } else {
            format!("{:.*}", self.decimals, self.value)
        }
    }
}

/// Rounds to three significant digits: 2 decimals below 10, 1 below 100, none from 100. A value
/// that rounds up into the next digit count (9.996 to 10.0) is shown with that count.
fn three_significant(value: f64) -> Rounded {
    for (decimals, limit) in [(2_i32, 10.0), (1, 100.0)] {
        let scale = 10f64.powi(decimals);
        let rounded = (value * scale).round() / scale;
        if rounded < limit {
            return Rounded {
                value: rounded,
                decimals: decimals as usize,
            };
        }
    }
    Rounded {
        value: value.round(),
        decimals: 0,
    }
}

/// Power or energy: whole units with thousands separators, one decimal below 10, and the big
/// unit with one decimal from [`GIGA_THRESHOLD`].
fn scaled(value: f64, small: &str, big: &str) -> String {
    scaled_like(value, value, small, big)
}

/// [`scaled`] in the unit `like` would take.
fn scaled_like(value: f64, like: f64, small: &str, big: &str) -> String {
    let magnitude = value.abs();
    let in_big = like.abs() >= GIGA_THRESHOLD;
    let shown = if in_big {
        magnitude / 1_000.0
    } else {
        magnitude
    };
    // No sign on a figure that rounds to zero.
    let sign = if value < 0.0 && shown >= 0.05 {
        MINUS
    } else {
        ""
    };
    if in_big {
        format!("{sign}{shown:.1} {big}")
    } else if magnitude < 10.0 {
        format!("{sign}{magnitude:.1} {small}")
    } else {
        format!(
            "{sign}{} {small}",
            group_thousands(magnitude.round() as u64)
        )
    }
}

/// `1234567` as `1,234,567`.
fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::hud::test_support::usd_style;
    use domain::time::{SimDuration, SimMonth};
    use domain::units::{Hertz, MegawattHours, Megawatts, Money, Price, Ratio};

    use super::*;

    fn check<T: Copy>(format: impl Fn(T) -> String, cases: &[(T, &str)]) {
        for &(value, expected) in cases {
            assert_eq!(format(value), expected);
        }
    }

    #[test]
    fn ramp_rates_are_per_game_minute() {
        check(
            |mw| ramp_rate(RampRate::mw_per_minute(mw)),
            &[
                (5.0, "5.0 MW/min"),
                (12.0, "12 MW/min"),
                (1_250.0, "1,250 MW/min"),
            ],
        );
    }

    #[test]
    fn scores_are_whole_points_within_zero_to_a_hundred() {
        check(
            score,
            &[
                (0.0, "0"),
                (72.4, "72"),
                (72.6, "73"),
                (100.0, "100"),
                (100.2, "100"),
                (-0.3, "0"),
            ],
        );
    }

    fn currency_style(currency: domain::units::Currency) -> MoneyStyle {
        MoneyStyle::resolve(currency, &crate::hud::test_support::shipped_l10n(), |_| {
            true
        })
    }

    #[test]
    fn every_currency_formats_compactly() {
        use domain::units::Currency::*;
        for (currency, symbol) in [
            (Usd, "$"),
            (Eur, "€"),
            (Cny, "¥"),
            (Inr, "₹"),
            (Rub, "₽"),
            (Brl, "R$"),
        ] {
            let style = currency_style(currency);
            assert_eq!(
                money(Money::from_dollars(1_234_000), &style),
                format!("{symbol}1.23M")
            );
            assert_eq!(
                money(Money::from_dollars(-2_000_000_000), &style),
                format!("−{symbol}2.00B")
            );
            assert_eq!(
                price(Price::per_mwh(45.0), &style),
                format!("{symbol}45.00/MWh")
            );
        }
    }

    #[test]
    fn a_missing_glyph_falls_back_to_the_iso_code() {
        let style = MoneyStyle::resolve(
            domain::units::Currency::Inr,
            &crate::hud::test_support::shipped_l10n(),
            |c| c != '₹',
        );
        assert_eq!(money(Money::from_dollars(1_200_000), &style), "INR 1.20M");
        assert_eq!(price(Price::per_mwh(-45.0), &style), "−INR 45.00/MWh");
    }

    #[test]
    fn money_placement_comes_from_fluent() {
        let l10n = crate::l10n::Localizer::from_sources([(
            "test.ftl".into(),
            "currency-eur = €\ncurrency-eur-prefix = { \"\" }\ncurrency-eur-suffix = { \" €\" }\n"
                .into(),
        )])
        .unwrap();
        let style = MoneyStyle::resolve(domain::units::Currency::Eur, &l10n, |_| true);
        assert_eq!(money(Money::from_cents(-123), &style), "−1.23 €");
        assert_eq!(price(Price::per_mwh(45.0), &style), "45.00 €/MWh");
    }

    #[test]
    fn usd_money_text_is_unchanged() {
        let cents = |dollars: f64| Money::from_cents((dollars * 100.0).round() as i64);
        check(
            |amount| money(amount, &currency_style(domain::units::Currency::Usd)),
            &[
                (Money::ZERO, "$0.00"),
                (Money::from_cents(5), "$0.05"),
                (cents(845.20), "$845.20"),
                (cents(999.99), "$999.99"),
                (cents(1_000.0), "$1.00k"),
                (cents(1_234.56), "$1.23k"),
                (cents(45_600.0), "$45.6k"),
                (cents(150_000.0), "$150k"),
                (cents(999_999.0), "$1.00M"),
                (cents(1_230_000.0), "$1.23M"),
                (cents(600_000_000.0), "$600M"),
                (cents(6_000_000_000.0), "$6.00B"),
                (cents(1_500_000_000_000.0), "$1,500B"),
            ],
        );
    }

    #[test]
    fn money_is_signed_with_a_true_minus() {
        let cents = |dollars: f64| Money::from_cents((dollars * 100.0).round() as i64);
        check(
            |amount| money(amount, &currency_style(domain::units::Currency::Usd)),
            &[
                (cents(-2_000_000_000.0), "\u{2212}$2.00B"),
                (cents(-45_600.0), "\u{2212}$45.6k"),
                (Money::from_cents(-1), "\u{2212}$0.01"),
            ],
        );
    }

    #[test]
    fn money_handles_the_extremes_without_panicking() {
        assert!(money(Money::from_cents(i64::MAX), &usd_style()).starts_with('$'));
        assert!(money(Money::from_cents(i64::MIN), &usd_style()).starts_with('\u{2212}'));
    }

    #[test]
    fn money_rounds_up_into_the_next_band_and_digit_count() {
        let cents = |dollars: f64| Money::from_cents((dollars * 100.0).round() as i64);
        check(
            |amount| money(amount, &currency_style(domain::units::Currency::Usd)),
            &[
                (cents(9_996.0), "$10.0k"),
                (cents(99_960.0), "$100k"),
                (cents(999_600.0), "$1.00M"),
            ],
        );
    }

    #[test]
    fn megawatts_formats_scale() {
        check(
            |v| megawatts(Megawatts::new(v)),
            &[
                (0.0, "0.0 MW"),
                (4.56, "4.6 MW"),
                (40.0, "40 MW"),
                (1_234.0, "1,234 MW"),
                (9_999.0, "9,999 MW"),
                (10_000.0, "10.0 GW"),
                (12_345.0, "12.3 GW"),
                (-250.0, "\u{2212}250 MW"),
                (-12_500.0, "\u{2212}12.5 GW"),
            ],
        );
    }

    #[test]
    fn megawatts_like_takes_the_unit_of_the_reference() {
        let in_gigawatts = Megawatts::new(50_000.0);
        check(
            |v| megawatts_like(Megawatts::new(v), in_gigawatts),
            &[
                (0.0, "0.0 GW"),
                (450.0, "0.5 GW"),
                (8_400.0, "8.4 GW"),
                (31_234.0, "31.2 GW"),
                (50_000.0, "50.0 GW"),
            ],
        );
        // A reference in megawatts keeps megawatts, even for a figure past it.
        let in_megawatts = Megawatts::new(5_000.0);
        check(
            |v| megawatts_like(Megawatts::new(v), in_megawatts),
            &[
                (0.0, "0.0 MW"),
                (1_234.0, "1,234 MW"),
                (12_345.0, "12,345 MW"),
            ],
        );
    }

    #[test]
    fn megawatt_hours_formats_scale() {
        check(
            |v| megawatt_hours(MegawattHours::new(v)),
            &[
                (0.0, "0.0 MWh"),
                (850.0, "850 MWh"),
                (1_234.0, "1,234 MWh"),
                (10_000.0, "10.0 GWh"),
                (123_456.0, "123.5 GWh"),
            ],
        );
    }

    #[test]
    fn hertz_deviation_is_always_signed() {
        check(
            |v| hertz_deviation(Hertz::new(v)),
            &[
                (0.12, "+0.12 Hz"),
                (-0.5, "\u{2212}0.50 Hz"),
                (0.0, "0.00 Hz"),
                (0.004, "0.00 Hz"),
                (-0.004, "0.00 Hz"),
            ],
        );
    }

    #[test]
    fn signed_percent_shows_the_direction() {
        check(
            signed_percent,
            &[
                (0.18, "+18.0%"),
                (-0.052, "\u{2212}5.2%"),
                (0.0, "0.0%"),
                (0.0004, "0.0%"),
                (-0.0004, "0.0%"),
                (1.5, "+150.0%"),
            ],
        );
    }

    #[test]
    fn plain_megawatts_can_be_typed_back() {
        check(
            |v| plain_megawatts(Megawatts::new(v)),
            &[
                (300.0, "300"),
                (240.04, "240"),
                (301.5, "301.5"),
                (1_234.0, "1234"),
                (0.0, "0"),
            ],
        );
    }

    #[test]
    fn percent_has_one_decimal() {
        check(
            |v| percent(Ratio::new(v).unwrap()),
            &[
                (0.994, "99.4%"),
                (1.0, "100.0%"),
                (0.0, "0.0%"),
                (0.15, "15.0%"),
            ],
        );
    }

    #[test]
    fn price_has_cents_and_groups_thousands() {
        check(
            |v| price(Price::per_mwh(v), &usd_style()),
            &[
                (45.0, "$45.00/MWh"),
                (0.5, "$0.50/MWh"),
                (1_500.0, "$1,500.00/MWh"),
                (-5.0, "\u{2212}$5.00/MWh"),
            ],
        );
    }

    #[test]
    fn clock_formats_ticks() {
        check(
            clock,
            &[
                (0, "00:00"),
                (5, "00:05"),
                (1_060, "17:40"),
                (1_439, "23:59"),
                (1_440, "24:00"),
            ],
        );
    }

    #[test]
    fn month_year_short_abbreviates_every_month_to_three_letters() {
        let short = |m| month_year_short(SimMonth::new(2027, m).unwrap());
        assert_eq!(short(1), "Jan 2027");
        assert_eq!(short(5), "May 2027");
        assert_eq!(short(9), "Sep 2027");
        assert_eq!(short(12), "Dec 2027");
        for m in 1..=12 {
            let month = SimMonth::new(2027, m).unwrap();
            assert!(month_year(month).starts_with(&short(m)[..3]), "{m}");
            assert_eq!(short(m).len(), "Mmm 2027".len(), "{m}");
        }
    }

    #[test]
    fn month_year_names_the_month() {
        let month = |y, m| month_year(SimMonth::new(y, m).unwrap());
        assert_eq!(month(2027, 3), "March 2027");
        assert_eq!(month(2030, 12), "December 2030");
        assert_eq!(month(2026, 1), "January 2026");
    }

    #[test]
    fn duration_formats_hours_and_minutes() {
        check(
            |ticks| duration(SimDuration::from_ticks(ticks)),
            &[
                (0, "0 min"),
                (40, "40 min"),
                (120, "2 h"),
                (150, "2 h 30 min"),
                (2_880, "48 h"),
            ],
        );
    }
}

#[cfg(test)]
mod points_tests {
    #[test]
    fn signed_points_formats_percentage_points() {
        for (input, expected) in [
            (0.005, "+0.50 pp"),
            (-0.01, "−1.00 pp"),
            (-0.000001, "0.00 pp"),
            (0.000001, "0.00 pp"),
        ] {
            assert_eq!(super::signed_points(input), expected);
        }
    }
}
