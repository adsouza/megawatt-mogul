//! The fixed-step driver: `FixedUpdate` runs `AdvanceTicks { n: speed }` (spec §8.1).
//!
//! At 4 Hz and 1×, a 1,440-tick day takes 6 real minutes (spec §2.2).

use application::Command;
use bevy::prelude::*;
use domain::session::{DayPhase, SessionStatus};

use super::{PlayerCommand, Session, SimSpeed};

/// Fixed steps per real second.
pub const FIXED_STEP_HZ: f64 = 4.0;

/// Whether the driver's last `AdvanceTicks` is still waiting for `apply_player_commands`. The
/// driver sets it, the command funnel clears it, so at most one batch is ever in flight.
#[derive(Resource, Debug, Default)]
pub struct TicksInFlight(pub(super) bool);

/// The ticks to ask for this fixed step, if any. Only while the day is being operated, the game
/// is still active, the speed isn't paused and no earlier batch is unapplied. The phase and
/// status are the domain's own, so they are never a frame behind.
///
/// A finished game never returns to `Active`, so once it is over the driver sends nothing again:
/// no stream of `GameOver` refusals (Review Focus 2).
fn ticks_to_request(
    phase: DayPhase,
    status: SessionStatus,
    speed: SimSpeed,
    in_flight: bool,
) -> Option<u32> {
    let running = phase == DayPhase::Operating && status == SessionStatus::Active && !in_flight;
    let n = speed.ticks();
    (running && n > 0).then_some(n)
}

/// Sends one `AdvanceTicks { n: speed }` per fixed step while the day runs.
pub fn drive_ticks(
    session: Res<Session>,
    speed: Res<SimSpeed>,
    mut in_flight: ResMut<TicksInFlight>,
    mut commands: MessageWriter<PlayerCommand>,
) {
    let game = session.0.session();
    if let Some(n) = ticks_to_request(game.phase(), game.status(), *speed, in_flight.0) {
        commands.write(PlayerCommand(Command::AdvanceTicks { n }));
        in_flight.0 = true;
    }
}

#[cfg(test)]
mod tests {
    use domain::session::GameOverReason;

    use super::*;

    const ACTIVE: SessionStatus = SessionStatus::Active;

    #[test]
    fn operating_at_speed_asks_for_the_speed_in_ticks() {
        for speed in [SimSpeed::X1, SimSpeed::X2, SimSpeed::X4, SimSpeed::X8] {
            assert_eq!(
                ticks_to_request(DayPhase::Operating, ACTIVE, speed, false),
                Some(speed.ticks())
            );
        }
    }

    #[test]
    fn paused_asks_for_nothing() {
        assert_eq!(
            ticks_to_request(DayPhase::Operating, ACTIVE, SimSpeed::Paused, false),
            None
        );
    }

    #[test]
    fn only_the_operating_phase_runs() {
        for phase in [DayPhase::Planning, DayPhase::Review] {
            assert_eq!(ticks_to_request(phase, ACTIVE, SimSpeed::X8, false), None);
        }
    }

    #[test]
    fn one_batch_in_flight_at_most() {
        assert_eq!(
            ticks_to_request(DayPhase::Operating, ACTIVE, SimSpeed::X1, true),
            None
        );
    }

    #[test]
    fn a_finished_game_asks_for_nothing_even_while_operating() {
        for reason in [GameOverReason::Bankrupt, GameOverReason::LicenseRevoked] {
            let over = SessionStatus::GameOver(reason);
            assert_eq!(
                ticks_to_request(DayPhase::Operating, over, SimSpeed::X8, false),
                None
            );
        }
    }
}
