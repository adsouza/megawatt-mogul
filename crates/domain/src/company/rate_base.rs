//! The rate base: the book value of the company's capital, which the
//! regulator lets it recover through straight-line depreciation and earn a
//! return on.
//!
//! An annual reserve review can hold excess capacity as "plant held for
//! future use" (spec §2.5). Holding suspends the return on the held portion,
//! not its depreciation. A held portion is re-tested at the next review:
//! released back to the eligible base if it became useful, or permanently
//! written off if it is still excess at that second consecutive review. A
//! write-off removes only the remaining book value and its future
//! depreciation. Every rate-base change is a non-cash adjustment: nothing
//! here touches the cash ledger.
//!
//! Held and written-off portions are exact integer shares of an asset's
//! whole capacity; their money is derived from the asset's remaining book
//! value, so `eligible + held == remaining` holds by construction.
//!
//! The reserve review creates at most one held slice per asset (new holds
//! target only assets without one), so a slice is identified by its asset.
//! When only part of a slice stays excess the slice shrinks to that part,
//! keeping its history, and the rest is released.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::ledger::AssetRef;
use crate::events::DomainEvent;
use crate::ids::{NodeId, PlantId};
use crate::time::SimMonth;
use crate::units::{Megawatts, Money};

/// Units in an asset's whole capacity. Held and written-off portions are
/// counted in these units so that splitting and summing them is exact.
const SHARE_UNITS: u64 = 1_000_000_000_000;

/// A held portion still excess at this many consecutive reviews is written off.
const WRITE_OFF_AT_REVIEW: u8 = 2;

/// What a reserve review needs to know about the physical asset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssetProfile {
    /// A plant: its firm capacity (`capacity × capacity_credit`) and its node.
    Plant { firm: Megawatts, node: NodeId },
    /// A line and the nodes it joins.
    Line { from: NodeId, to: NodeId },
}

/// The part of an asset currently held for future use, with its review history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldSlice {
    /// Units of the asset's whole capacity (out of [`SHARE_UNITS`]).
    share: u64,
    /// For associated line capital: the plant whose slice this one follows.
    follows: Option<PlantId>,
    last_excess_review: i32,
    consecutive_excess_reviews: u8,
}

impl HeldSlice {
    fn new(share: u64, follows: Option<PlantId>, year: i32) -> Self {
        Self {
            share,
            follows,
            last_excess_review: year,
            consecutive_excess_reviews: 1,
        }
    }

    /// The review year that last found this slice excess.
    pub fn last_excess_review(&self) -> i32 {
        self.last_excess_review
    }

    /// Consecutive reviews that have found this slice excess.
    pub fn consecutive_excess_reviews(&self) -> u8 {
        self.consecutive_excess_reviews
    }

    /// For associated line capital, the plant whose held fraction it carries.
    pub fn follows(&self) -> Option<PlantId> {
        self.follows
    }

    /// The held fraction of the asset's whole capacity.
    pub fn fraction(&self) -> f64 {
        share_fraction(self.share)
    }
}

/// One capital asset's book value.
///
/// `remaining` is the book value of the part not written off; it splits into
/// [`eligible`](Self::eligible) and [`held`](Self::held).
#[derive(Debug, Clone, PartialEq)]
pub struct RateBaseAsset {
    pub asset: AssetRef,
    pub original: Money,
    pub remaining: Money,
    pub monthly_depreciation: Money,
    pub added_on: SimMonth,
    pub profile: AssetProfile,
    /// Starting fleet and lines: depreciate normally, but are never held or
    /// written off and never count as new capital.
    pub legacy: bool,
    months_left: u32,
    depreciated: Money,
    written_off: Money,
    /// Units of capacity not written off.
    live_share: u64,
    held: Option<HeldSlice>,
}

impl RateBaseAsset {
    fn new(
        asset: AssetRef,
        cost: Money,
        book_life_months: u32,
        month: SimMonth,
        profile: AssetProfile,
        legacy: bool,
    ) -> Self {
        Self {
            asset,
            original: cost,
            remaining: cost,
            monthly_depreciation: Money::from_cents(cost.cents() / i64::from(book_life_months)),
            added_on: month,
            profile,
            legacy,
            months_left: book_life_months,
            depreciated: Money::ZERO,
            written_off: Money::ZERO,
            live_share: SHARE_UNITS,
            held: None,
        }
    }

    /// Remaining book value held out of the eligible rate base.
    pub fn held(&self) -> Money {
        self.held
            .as_ref()
            .map_or(Money::ZERO, |slice| self.book_of(slice.share))
    }

    /// Remaining book value that earns a return.
    pub fn eligible(&self) -> Money {
        self.remaining - self.held()
    }

    /// Book value permanently written off.
    pub fn written_off(&self) -> Money {
        self.written_off
    }

    /// Book value recovered through depreciation so far.
    pub fn depreciated(&self) -> Money {
        self.depreciated
    }

    /// Months of book life still to depreciate.
    pub fn months_left(&self) -> u32 {
        self.months_left
    }

    pub fn held_slice(&self) -> Option<&HeldSlice> {
        self.held.as_ref()
    }

    /// Firm capacity behind the held slice; `None` for lines or when nothing is held.
    pub fn held_firm(&self) -> Option<Megawatts> {
        let firm = self.firm()?;
        self.held
            .as_ref()
            .map(|slice| firm * share_fraction(slice.share))
    }

    fn firm(&self) -> Option<Megawatts> {
        match self.profile {
            AssetProfile::Plant { firm, .. } => Some(firm),
            AssetProfile::Line { .. } => None,
        }
    }

    /// Firm capacity of the part not written off (zero for lines).
    fn live_firm(&self) -> f64 {
        self.firm()
            .map_or(0.0, |firm| firm.value() * share_fraction(self.live_share))
    }

    /// New capital for the review of `year`.
    fn is_new_in(&self, year: i32) -> bool {
        !self.legacy && self.added_on.year() == year
    }

    /// Remaining book value of `share` units of the live capacity, rounded
    /// half-even to whole cents.
    fn book_of(&self, share: u64) -> Money {
        prorate(self.remaining, share, self.live_share)
    }

    /// Units of capacity carrying `mw` of this plant's firm capacity, capped
    /// by the live capacity. Zero for lines and plants without firm capacity.
    fn share_for_firm(&self, mw: f64) -> u64 {
        let Some(firm) = self.firm().map(Megawatts::value) else {
            return 0;
        };
        if firm <= 0.0 || mw.is_nan() || mw <= 0.0 {
            return 0;
        }
        let units = (mw / firm * SHARE_UNITS as f64).round_ties_even();
        units.min(self.live_share as f64) as u64
    }

    fn depreciate(&mut self) -> Money {
        if self.months_left == 0 {
            return Money::ZERO;
        }
        let amount = if self.months_left == 1 {
            self.remaining
        } else {
            self.monthly_depreciation.min(self.remaining)
        };
        self.remaining -= amount;
        self.depreciated += amount;
        self.months_left -= 1;
        amount
    }

    /// Permanently removes `share` units and their `amount` of book value,
    /// spreading the rest straight-line over the remaining life.
    fn write_off(&mut self, share: u64, amount: Money) {
        self.remaining -= amount;
        self.written_off += amount;
        self.live_share -= share;
        if self.months_left > 0 {
            self.monthly_depreciation =
                Money::from_cents(self.remaining.cents() / i64::from(self.months_left));
        }
    }
}

/// What the annual reserve review is told about the system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReserveReviewInputs {
    /// The review year: capital added in this year is new capital.
    pub year: i32,
    /// Physical firm capacity above the allowed reserve margin.
    pub excess: Megawatts,
}

/// One non-cash adjustment a reserve review makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapitalChange {
    Held { asset: AssetRef, amount: Money },
    Released { asset: AssetRef, amount: Money },
    Disallowed { asset: AssetRef, amount: Money },
}

impl CapitalChange {
    pub fn into_event(self) -> DomainEvent {
        match self {
            Self::Held { asset, amount } => DomainEvent::CapitalHeld { asset, amount },
            Self::Released { asset, amount } => DomainEvent::CapitalReleased { asset, amount },
            Self::Disallowed { asset, amount } => DomainEvent::CapitalDisallowed { asset, amount },
        }
    }
}

/// The outcome of a reserve review: its adjustments in order and the rate
/// base it leaves behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapitalAssessment {
    pub year: i32,
    pub changes: Vec<CapitalChange>,
    pub eligible_total: Money,
    pub held_total: Money,
}

impl CapitalAssessment {
    /// Book value this review writes off.
    pub fn disallowed(&self) -> Money {
        self.changes
            .iter()
            .map(|change| match change {
                CapitalChange::Disallowed { amount, .. } => *amount,
                _ => Money::ZERO,
            })
            .sum()
    }
}

/// Every capital asset's book value, and the reserve-review history.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RateBase {
    assets: BTreeMap<AssetRef, RateBaseAsset>,
    last_review_year: Option<i32>,
}

impl RateBase {
    /// Adds new capital, recovered straight-line over `book_life_months`.
    pub fn add(
        &mut self,
        asset: AssetRef,
        cost: Money,
        book_life_months: u32,
        month: SimMonth,
        profile: AssetProfile,
    ) {
        self.insert(asset, cost, book_life_months, month, profile, false);
    }

    /// Adds starting capital: it depreciates normally but is never held or
    /// written off and never counts as new capital.
    pub fn add_legacy(
        &mut self,
        asset: AssetRef,
        cost: Money,
        book_life_months: u32,
        month: SimMonth,
        profile: AssetProfile,
    ) {
        self.insert(asset, cost, book_life_months, month, profile, true);
    }

    fn insert(
        &mut self,
        asset: AssetRef,
        cost: Money,
        book_life_months: u32,
        month: SimMonth,
        profile: AssetProfile,
        legacy: bool,
    ) {
        // Region validation rules these out, so reaching one is a bug.
        assert!(!cost.is_negative(), "negative capital cost {cost:?}");
        assert!(book_life_months >= 1, "zero book life for {asset:?}");
        assert!(
            !self.assets.contains_key(&asset),
            "{asset:?} already in the rate base"
        );
        let entry = RateBaseAsset::new(asset, cost, book_life_months, month, profile, legacy);
        self.assets.insert(asset, entry);
    }

    /// Books one month of straight-line depreciation on every asset, held
    /// portions included, and returns the month's total. An asset's final
    /// month takes its exact remainder.
    pub fn depreciate_month(&mut self) -> Money {
        self.assets
            .values_mut()
            .map(RateBaseAsset::depreciate)
            .sum()
    }

    /// Book value still recoverable: eligible plus held.
    pub fn total(&self) -> Money {
        self.assets.values().map(|a| a.remaining).sum()
    }

    /// Book value that earns a return: excludes held and written-off capital.
    pub fn eligible_total(&self) -> Money {
        self.assets.values().map(RateBaseAsset::eligible).sum()
    }

    pub fn held_total(&self) -> Money {
        self.assets.values().map(RateBaseAsset::held).sum()
    }

    pub fn written_off_total(&self) -> Money {
        self.assets.values().map(RateBaseAsset::written_off).sum()
    }

    pub fn asset(&self, asset: AssetRef) -> Option<&RateBaseAsset> {
        self.assets.get(&asset)
    }

    /// Every asset, plants then lines, each in id order.
    pub fn assets(&self) -> impl Iterator<Item = &RateBaseAsset> {
        self.assets.values()
    }

    /// The year of the last applied reserve review.
    pub fn last_review_year(&self) -> Option<i32> {
        self.last_review_year
    }

    /// What [`apply_reserve_review`](Self::apply_reserve_review) would do,
    /// without changing anything.
    pub fn preview_reserve_review(&self, inputs: &ReserveReviewInputs) -> CapitalAssessment {
        let mut scratch = self.clone();
        let changes = scratch.review(inputs);
        CapitalAssessment {
            year: inputs.year,
            changes,
            eligible_total: scratch.eligible_total(),
            held_total: scratch.held_total(),
        }
    }

    /// Applies the reserve review for `inputs.year`. A review for a year at
    /// or before the last applied one is a no-op, so holdings cannot age twice.
    pub fn apply_reserve_review(&mut self, inputs: &ReserveReviewInputs) -> Vec<DomainEvent> {
        self.review(inputs)
            .into_iter()
            .map(CapitalChange::into_event)
            .collect()
    }

    fn review(&mut self, inputs: &ReserveReviewInputs) -> Vec<CapitalChange> {
        let year = inputs.year;
        if self.last_review_year.is_some_and(|last| year <= last) {
            return Vec::new();
        }
        self.last_review_year = Some(year);
        let excess = inputs.excess.value().max(0.0);
        let mut changes = Vec::new();

        // 1. Re-test held plant slices first, against the whole excess.
        let held: Vec<(PlantId, f64)> = self
            .assets
            .values()
            .filter_map(|a| match (a.asset, a.held_firm()) {
                (AssetRef::Plant(id), Some(mw)) => Some((id, mw.value())),
                _ => None,
            })
            .collect();
        let held_mw: f64 = held.iter().map(|(_, mw)| mw).sum();
        let weights: Vec<f64> = held.iter().map(|(_, mw)| *mw).collect();
        for ((plant, _), keep_mw) in held.iter().zip(allocate(excess, &weights)) {
            self.retest_plant(*plant, keep_mw, year, &mut changes);
        }

        // 2. Spread what excess is left across this year's new plant capital.
        let new: Vec<(PlantId, f64)> = self
            .assets
            .values()
            .filter(|a| a.is_new_in(year) && a.held.is_none())
            .filter_map(|a| match a.asset {
                AssetRef::Plant(id) => Some((id, a.live_firm())),
                AssetRef::Line(_) => None,
            })
            .filter(|(_, mw)| *mw > 0.0)
            .collect();
        let weights: Vec<f64> = new.iter().map(|(_, mw)| *mw).collect();
        for ((plant, _), mw) in new.iter().zip(allocate(excess - held_mw, &weights)) {
            self.hold_new(*plant, mw, year, &mut changes);
        }
        changes
    }

    /// Re-tests a plant's held slice, keeping `keep_mw` of it held, and
    /// carries the same split to the line slices that follow it.
    fn retest_plant(
        &mut self,
        plant: PlantId,
        keep_mw: f64,
        year: i32,
        changes: &mut Vec<CapitalChange>,
    ) {
        let key = AssetRef::Plant(plant);
        let Some(asset) = self.assets.get(&key) else {
            return;
        };
        let old = asset.held.as_ref().map_or(0, |s| s.share);
        let keep = asset.share_for_firm(keep_mw).min(old);
        let written_off = self.retest_slice(key, keep, year, None, changes);

        let lines: Vec<(AssetRef, u64)> = self
            .assets
            .values()
            .filter_map(|a| {
                let slice = a.held.as_ref()?;
                (slice.follows == Some(plant)).then_some((a.asset, slice.share))
            })
            .collect();
        for (line, share) in lines {
            let line_keep = scale_share(share, keep, old);
            self.retest_slice(line, line_keep, year, Some(written_off), changes);
        }
    }

    /// Shrinks an asset's held slice to `keep` units, releasing the rest, then
    /// ages what is still held. The still-held part is written off at its
    /// second consecutive review, or as `write_off` dictates for line slices
    /// that follow a plant. Returns whether it was written off.
    fn retest_slice(
        &mut self,
        key: AssetRef,
        keep: u64,
        year: i32,
        write_off: Option<bool>,
        changes: &mut Vec<CapitalChange>,
    ) -> bool {
        let Some(asset) = self.assets.get_mut(&key) else {
            return false;
        };
        let Some(mut slice) = asset.held.take() else {
            return false;
        };
        let keep = keep.min(slice.share);
        if keep < slice.share {
            let amount = asset.book_of(slice.share) - asset.book_of(keep);
            changes.push(CapitalChange::Released { asset: key, amount });
        }
        if keep == 0 {
            return false;
        }
        slice.share = keep;
        if slice.last_excess_review != year {
            slice.consecutive_excess_reviews = slice.consecutive_excess_reviews.saturating_add(1);
            slice.last_excess_review = year;
        }
        if write_off.unwrap_or(slice.consecutive_excess_reviews >= WRITE_OFF_AT_REVIEW) {
            let amount = asset.book_of(keep);
            asset.write_off(keep, amount);
            changes.push(CapitalChange::Disallowed { asset: key, amount });
            true
        } else {
            asset.held = Some(slice);
            false
        }
    }

    /// Holds `mw` of a new plant's firm capacity and the same fraction of
    /// its associated line capital.
    fn hold_new(&mut self, plant: PlantId, mw: f64, year: i32, changes: &mut Vec<CapitalChange>) {
        let key = AssetRef::Plant(plant);
        let Some(asset) = self.assets.get_mut(&key) else {
            return;
        };
        let share = asset.share_for_firm(mw);
        if share == 0 {
            return;
        }
        let plant_live = asset.live_share;
        asset.held = Some(HeldSlice::new(share, None, year));
        changes.push(CapitalChange::Held {
            asset: key,
            amount: asset.held(),
        });

        for line in self.associated_lines(plant, year) {
            let Some(asset) = self.assets.get_mut(&line) else {
                continue;
            };
            let line_share = scale_share(asset.live_share, share, plant_live);
            if line_share == 0 {
                continue;
            }
            asset.held = Some(HeldSlice::new(line_share, Some(plant), year));
            changes.push(CapitalChange::Held {
                asset: line,
                amount: asset.held(),
            });
        }
    }

    /// New, unheld lines of `year` whose associated plant is `plant`.
    fn associated_lines(&self, plant: PlantId, year: i32) -> Vec<AssetRef> {
        self.assets
            .values()
            .filter(|a| a.is_new_in(year) && a.held.is_none())
            .filter_map(|a| match a.profile {
                AssetProfile::Line { from, to }
                    if self.associated_plant(from, to, year) == Some(plant) =>
                {
                    Some(a.asset)
                }
                _ => None,
            })
            .collect()
    }

    /// The plant a new line of `year` serves: the lowest-id new plant of the
    /// same year at either endpoint. `None` means the line is never held.
    fn associated_plant(&self, from: NodeId, to: NodeId, year: i32) -> Option<PlantId> {
        self.assets
            .values()
            .find_map(|a| match (a.asset, a.profile) {
                (AssetRef::Plant(id), AssetProfile::Plant { node, firm })
                    if firm > Megawatts::ZERO
                        && a.is_new_in(year)
                        && (node == from || node == to) =>
                {
                    Some(id)
                }
                _ => None,
            })
    }
}

/// Splits `total` across `weights` in proportion, never giving an entry more
/// than its weight. The last (highest-id) entry takes the rounding remainder.
fn allocate(total: f64, weights: &[f64]) -> Vec<f64> {
    let sum: f64 = weights.iter().sum();
    if total >= sum {
        return weights.to_vec();
    }
    if total.is_nan() || total <= 0.0 {
        return vec![0.0; weights.len()];
    }
    let mut parts: Vec<f64> = weights.iter().map(|w| w * total / sum).collect();
    if let (Some((last, rest)), Some(cap)) = (parts.split_last_mut(), weights.last()) {
        let given: f64 = rest.iter().sum();
        *last = (total - given).min(*cap).max(0.0);
    }
    parts
}

fn share_fraction(share: u64) -> f64 {
    share as f64 / SHARE_UNITS as f64
}

/// `amount × num / den`, rounded half-even to whole cents.
fn prorate(amount: Money, num: u64, den: u64) -> Money {
    if den == 0 {
        return Money::ZERO;
    }
    let cents = div_round_half_even(
        i128::from(amount.cents()) * i128::from(num),
        i128::from(den),
    );
    Money::from_cents(cents as i64)
}

/// `share × num / den` in share units, rounded half-even.
fn scale_share(share: u64, num: u64, den: u64) -> u64 {
    if den == 0 {
        return 0;
    }
    div_round_half_even(i128::from(share) * i128::from(num), i128::from(den)) as u64
}

/// `n / d` rounded to the nearest integer, ties to even. `d` must be positive.
fn div_round_half_even(n: i128, d: i128) -> i128 {
    let (q, r) = (n.div_euclid(d), n.rem_euclid(d));
    match (2 * r).cmp(&d) {
        Ordering::Less => q,
        Ordering::Greater => q + 1,
        Ordering::Equal => q + (q & 1),
    }
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(RateBase {
    assets,
    last_review_year
});
crate::state_record!(RateBaseAsset {
    asset,
    original,
    remaining,
    monthly_depreciation,
    added_on,
    profile,
    legacy,
    months_left,
    depreciated,
    written_off,
    live_share,
    held
});
crate::state_record!(HeldSlice {
    share,
    follows,
    last_excess_review,
    consecutive_excess_reviews
});
impl crate::state::State for AssetProfile {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Plant { firm, node } => {
                crate::state::State::write(&0u8, out);
                crate::state::State::write(firm, out);
                crate::state::State::write(node, out);
            }
            Self::Line { from, to } => {
                crate::state::State::write(&1u8, out);
                crate::state::State::write(from, out);
                crate::state::State::write(to, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Plant {
                firm: crate::state::State::read(input)?,
                node: crate::state::State::read(input)?,
            },
            1 => Self::Line {
                from: crate::state::State::read(input)?,
                to: crate::state::State::read(input)?,
            },
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}

#[cfg(test)]
mod tests;
