//! Career time progression: daytime free roam and nighttime racing run on a time budget.
//!
//! The career day is split into two playable **shifts**:
//!
//! - [`Shift::Day`] — free roam, sanctioned events, shopping (default 08:00–18:00).
//! - [`Shift::Night`] — street racing and meets (default 20:00–04:00).
//!
//! Each shift has a budget of in-game minutes. Driving spends it continuously
//! ([`TimeProgression::step`], at [`TimeRules::drive_ratio`] in-game seconds per simulated
//! second), and activities spend it in lumps ([`TimeProgression::spend`]): a garage visit, a
//! repair, a mod install, a race entry, an impound retrieval. The environment clock follows the
//! budget — half the budget used means the sun is halfway through the shift's window — so the
//! light the player sees always matches how much time they have left.
//!
//! When the budget runs out the shift ends and the world plays a short transition (dusk after
//! the day, dawn after the night) into the next shift. Time spent past the end of a shift (a
//! long repair started late in the day) is carried over as **debt** into the next shift; debt
//! larger than a whole shift skips that shift entirely (an overnight repair job means you miss
//! the night's racing).
//!
//! The progression drives the world clock instead of the clock's own ratio: after
//! [`crate::World::step`] call [`TimeProgression::apply`] with `&mut world.clock` (or set
//! `world.clock.ratio = 0.0` and apply once per tick). Everything is simulated time only (no
//! wall clock) and order-independent of anything but the calls made, so it is deterministic and
//! can be saved with the career profile (`Serialize`/`Deserialize`).
//!
//! Minimal wiring:
//!
//! ```
//! use wp_world::progression::{Activity, TimeProgression, TimeRules};
//! use wp_world::WorldClock;
//!
//! let mut time = TimeProgression::new(TimeRules::default());
//! let mut clock = WorldClock::default();
//! let mut events = Vec::new();
//! // every physics tick while the player is driving
//! time.step(1.0 / 240.0, &mut events);
//! time.apply(&mut clock);
//! // the garage charges a repair (labour hours from the repair estimate)
//! let receipt = time.spend(Activity::Repair { labour_hours: 2.5 }, &mut events);
//! assert_eq!(receipt.minutes, 113); // 2.5 h × 45 min per labour hour
//! ```

use serde::{Deserialize, Serialize};
use wp_contracts::{DayPhase, TimeOfDay};

use crate::clock::WorldClock;

/// In-game seconds per in-game minute.
const MIN_S: f32 = 60.0;

/// One of the two playable parts of a career day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Shift {
    /// Daytime free roam.
    Day,
    /// Nighttime racing.
    Night,
}

impl Shift {
    /// The shift that follows this one.
    pub fn next(self) -> Shift {
        match self {
            Shift::Day => Shift::Night,
            Shift::Night => Shift::Day,
        }
    }
}

/// The clock window and time budget of one shift.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShiftWindow {
    /// Clock time when the shift starts, hours since midnight (`0..24`).
    pub start_h: f32,
    /// Clock time when the shift ends, hours; may exceed 24 for a shift that runs past
    /// midnight (`28.0` = 04:00 the next morning). Must be greater than `start_h`.
    pub end_h: f32,
    /// In-game minutes the player can spend in this shift.
    pub budget_min: u32,
}

impl ShiftWindow {
    /// Budget in in-game seconds.
    pub fn budget_s(&self) -> f32 {
        self.budget_min as f32 * MIN_S
    }
}

/// How many in-game minutes each discrete activity costs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityCosts {
    /// Pulling into the garage (parking, menus, coffee).
    pub garage_visit_min: u32,
    /// Repair time per labour hour of the repair estimate, minutes. `60.0` = the shop works
    /// the estimate's labour hours exactly; lower values model a fast crew.
    pub repair_min_per_labour_hour: f32,
    /// Shortest repair, minutes (a scratch still takes the car off the road for a while).
    pub repair_min_floor: u32,
    /// Mod install time per labour hour, minutes.
    pub mod_min_per_labour_hour: f32,
    /// Shortest mod install, minutes.
    pub mod_min_floor: u32,
    /// Overhead of entering a race (finding the meet, lining up, the countdown). The race
    /// itself is driven, so [`TimeProgression::step`] charges it.
    pub race_entry_min: u32,
    /// Getting a car out of the impound lot.
    pub impound_retrieval_min: u32,
}

impl Default for ActivityCosts {
    fn default() -> Self {
        Self {
            garage_visit_min: 20,
            repair_min_per_labour_hour: 45.0,
            repair_min_floor: 15,
            mod_min_per_labour_hour: 45.0,
            mod_min_floor: 15,
            race_entry_min: 15,
            impound_retrieval_min: 90,
        }
    }
}

/// Everything that tunes the time progression.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeRules {
    /// The daytime free-roam shift.
    pub day: ShiftWindow,
    /// The nighttime racing shift.
    pub night: ShiftWindow,
    /// In-game seconds of budget spent per simulated second of driving (default 10, the same
    /// as [`crate::DEFAULT_TIME_RATIO`]: a 600-minute day lasts one real hour of driving).
    pub drive_ratio: f32,
    /// Length of the dusk / dawn transition between shifts, simulated seconds. `0` = instant.
    pub transition_s: f32,
    /// A [`TimeEvent::LowTime`] fires once per shift when the remaining budget drops to this
    /// many minutes.
    pub low_time_warning_min: u32,
    /// Activity costs.
    pub costs: ActivityCosts,
}

impl Default for TimeRules {
    fn default() -> Self {
        Self {
            day: ShiftWindow {
                start_h: 8.0,
                end_h: 18.0,
                budget_min: 600,
            },
            night: ShiftWindow {
                start_h: 20.0,
                end_h: 28.0,
                budget_min: 480,
            },
            drive_ratio: 10.0,
            transition_s: 20.0,
            low_time_warning_min: 60,
            costs: ActivityCosts::default(),
        }
    }
}

impl TimeRules {
    /// The window of `shift`.
    pub fn window(&self, shift: Shift) -> &ShiftWindow {
        match shift {
            Shift::Day => &self.day,
            Shift::Night => &self.night,
        }
    }

    /// In-game minutes `activity` costs under these rules (always at least 0, rounded up to
    /// whole minutes).
    pub fn cost_minutes(&self, activity: Activity) -> u32 {
        let c = &self.costs;
        let labour = |hours: f32, per_hour: f32, floor: u32| -> u32 {
            let m = (hours.max(0.0) * per_hour).ceil();
            (m as u32).max(floor)
        };
        match activity {
            Activity::GarageVisit => c.garage_visit_min,
            Activity::Repair { labour_hours } => labour(
                labour_hours,
                c.repair_min_per_labour_hour,
                c.repair_min_floor,
            ),
            Activity::ModInstall { labour_hours } => {
                labour(labour_hours, c.mod_min_per_labour_hour, c.mod_min_floor)
            }
            Activity::RaceEntry => c.race_entry_min,
            Activity::ImpoundRetrieval => c.impound_retrieval_min,
            Activity::Custom { minutes } => minutes,
        }
    }

    /// Check the windows make sense (positive budgets, `end_h > start_h`, start in `0..24`).
    pub fn validate(&self) -> Result<(), String> {
        for (name, w) in [("day", &self.day), ("night", &self.night)] {
            if !(0.0..24.0).contains(&w.start_h) {
                return Err(format!("{name}.start_h must be in 0..24"));
            }
            if w.end_h <= w.start_h || w.end_h - w.start_h > 24.0 {
                return Err(format!("{name}.end_h must be after start_h, within a day"));
            }
            if w.budget_min == 0 {
                return Err(format!("{name}.budget_min must be positive"));
            }
        }
        let bad = |v: f32| v.is_nan() || v < 0.0;
        if bad(self.drive_ratio) || bad(self.transition_s) {
            return Err("drive_ratio and transition_s must be non-negative".into());
        }
        Ok(())
    }
}

/// Something that costs a lump of in-game time. Costs come from [`ActivityCosts`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Activity {
    /// Pulling into the garage.
    GarageVisit,
    /// Repairing the car; `labour_hours` is the repair estimate's total (unscaled) labour
    /// hours (`RepairLine::labour_hours` summed over the lines being fixed).
    Repair {
        /// Total labour hours of the work being done.
        labour_hours: f32,
    },
    /// Installing mods; `labour_hours` as for [`Activity::Repair`].
    ModInstall {
        /// Total labour hours of the install.
        labour_hours: f32,
    },
    /// Entering a race (overhead only; the race is driven).
    RaceEntry,
    /// Getting a car out of the impound lot.
    ImpoundRetrieval,
    /// A fixed number of in-game minutes (scripted events, cutscenes, "wait an hour").
    Custom {
        /// Minutes to spend.
        minutes: u32,
    },
}

/// Something the progression did that the game should react to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TimeEvent {
    /// The remaining budget dropped to [`TimeRules::low_time_warning_min`] or below.
    LowTime {
        /// Career day.
        day: u32,
        /// Shift.
        shift: Shift,
        /// Whole minutes left.
        remaining_min: u32,
    },
    /// A shift's budget ran out (or [`TimeProgression::end_shift`] was called); the
    /// transition into `next` starts now. End the race meet / free roam session here.
    ShiftEnded {
        /// Career day of the shift that ended.
        day: u32,
        /// The shift that ended.
        shift: Shift,
        /// The shift coming next.
        next: Shift,
    },
    /// A shift was skipped entirely because carried-over time covered its whole budget.
    ShiftSkipped {
        /// Career day of the skipped shift.
        day: u32,
        /// The skipped shift.
        shift: Shift,
    },
    /// A shift started (transition finished). Re-roll weather, traffic density, the race
    /// calendar etc. here.
    ShiftStarted {
        /// Career day (increments when a day shift starts).
        day: u32,
        /// The shift that started.
        shift: Shift,
        /// Minutes already used by carried-over time.
        carried_min: u32,
    },
}

/// What a [`TimeProgression::spend`] call cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpendReceipt {
    /// In-game minutes charged.
    pub minutes: u32,
    /// The charge used up the current shift (or landed during a transition) and carries into
    /// the next one.
    pub ends_shift: bool,
}

/// Whether the progression is inside a shift or between two.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Stage {
    /// A shift is running; its budget is being spent.
    Active,
    /// Dusk / dawn transition from the previous shift into [`TimeProgression::shift`]'s next.
    Transition {
        /// Simulated seconds into the transition.
        elapsed_s: f32,
    },
}

/// Career time state: which day and shift it is and how much of the shift is used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeProgression {
    rules: TimeRules,
    day: u32,
    shift: Shift,
    /// Budget used in the current shift, in-game seconds (`0..=budget_s`).
    used_s: f32,
    /// Time to charge to the next shift(s), in-game seconds.
    carry_s: f32,
    stage: Stage,
    warned: bool,
}

impl TimeProgression {
    /// Day 1, start of the day shift.
    pub fn new(rules: TimeRules) -> Self {
        Self::starting_at(rules, 1, Shift::Day)
    }

    /// Start of `shift` on career day `day`.
    pub fn starting_at(rules: TimeRules, day: u32, shift: Shift) -> Self {
        Self {
            rules,
            day,
            shift,
            used_s: 0.0,
            carry_s: 0.0,
            stage: Stage::Active,
            warned: false,
        }
    }

    /// The rules in use.
    pub fn rules(&self) -> &TimeRules {
        &self.rules
    }

    /// Career day (starts at 1, increments when a day shift starts).
    pub fn day(&self) -> u32 {
        self.day
    }

    /// The current shift. During a transition this is the shift that just ended.
    pub fn shift(&self) -> Shift {
        self.shift
    }

    /// Active shift or transition.
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// `true` between shifts (dusk / dawn playing).
    pub fn is_transitioning(&self) -> bool {
        matches!(self.stage, Stage::Transition { .. })
    }

    /// In-game minutes left in the current shift (`0` during a transition).
    pub fn remaining_min(&self) -> f32 {
        match self.stage {
            Stage::Active => (self.budget_s() - self.used_s).max(0.0) / MIN_S,
            Stage::Transition { .. } => 0.0,
        }
    }

    /// Fraction of the current shift's budget used, `0..=1`.
    pub fn fraction_used(&self) -> f32 {
        match self.stage {
            Stage::Active => (self.used_s / self.budget_s()).clamp(0.0, 1.0),
            Stage::Transition { .. } => 1.0,
        }
    }

    /// In-game minutes already owed to the next shift.
    pub fn carried_min(&self) -> f32 {
        self.carry_s / MIN_S
    }

    /// Minutes `activity` would cost (for "this repair takes 2 h 15 min" previews).
    pub fn cost_minutes(&self, activity: Activity) -> u32 {
        self.rules.cost_minutes(activity)
    }

    /// `true` if `activity` fits in what is left of the current shift (doing it anyway is
    /// allowed; the excess carries over into the next shift).
    pub fn fits(&self, activity: Activity) -> bool {
        !self.is_transitioning() && self.cost_minutes(activity) as f32 <= self.remaining_min()
    }

    /// Clock time the environment should show: within a shift it moves through the shift's
    /// window in proportion to the budget used; during a transition it sweeps from the end of
    /// the old shift to the start of the next.
    pub fn time_of_day(&self) -> TimeOfDay {
        let w = self.rules.window(self.shift);
        match self.stage {
            Stage::Active => {
                let h = w.start_h + (w.end_h - w.start_h) * self.fraction_used();
                TimeOfDay::from_hours(h)
            }
            Stage::Transition { elapsed_s } => {
                let next = self.rules.window(self.shift.next());
                let from = w.end_h.rem_euclid(24.0);
                let mut to = next.start_h;
                if to < from {
                    to += 24.0;
                }
                let k = if self.rules.transition_s > 0.0 {
                    (elapsed_s / self.rules.transition_s).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                TimeOfDay::from_hours(from + (to - from) * k)
            }
        }
    }

    /// Coarse day phase of [`Self::time_of_day`].
    pub fn phase(&self) -> DayPhase {
        self.time_of_day().phase()
    }

    /// Write [`Self::time_of_day`] into the world clock. Call after `World::step` each tick
    /// (it overrides the clock's own ratio-based advance).
    pub fn apply(&self, clock: &mut WorldClock) {
        clock.time = self.time_of_day();
    }

    /// Advance by `dt` simulated seconds of driving: spends `dt × drive_ratio` in-game seconds
    /// of budget, or plays the transition. Do not call while the player is in a menu or the
    /// garage (those cost time through [`Self::spend`] instead).
    pub fn step(&mut self, dt: f32, events: &mut Vec<TimeEvent>) {
        let dt = dt.max(0.0);
        match self.stage {
            Stage::Active => self.charge(dt * self.rules.drive_ratio, events),
            Stage::Transition { elapsed_s } => {
                let elapsed_s = elapsed_s + dt;
                if elapsed_s >= self.rules.transition_s {
                    self.begin_next(events);
                } else {
                    self.stage = Stage::Transition { elapsed_s };
                }
            }
        }
    }

    /// Charge a discrete activity. Excess over the current shift (or anything charged during a
    /// transition) carries into the next shift. This is the API the garage calls for repairs
    /// and mod installs.
    pub fn spend(&mut self, activity: Activity, events: &mut Vec<TimeEvent>) -> SpendReceipt {
        let minutes = self.cost_minutes(activity);
        let ends_shift = self.spend_minutes(minutes, events);
        SpendReceipt {
            minutes,
            ends_shift,
        }
    }

    /// Charge `minutes` in-game minutes. Returns `true` if the charge ended the shift (or
    /// landed during a transition and carries over).
    pub fn spend_minutes(&mut self, minutes: u32, events: &mut Vec<TimeEvent>) -> bool {
        if self.is_transitioning() {
            self.carry_s += minutes as f32 * MIN_S;
            return true;
        }
        self.charge(minutes as f32 * MIN_S, events);
        self.is_transitioning()
    }

    /// End the current shift now ("skip to night", "go home and sleep"). The unused budget is
    /// lost. Does nothing during a transition.
    pub fn end_shift(&mut self, events: &mut Vec<TimeEvent>) {
        if !self.is_transitioning() {
            self.used_s = self.budget_s();
            self.finish_shift(events);
        }
    }

    /// Jump to the end of a running transition (loading screen, "skip" button).
    pub fn finish_transition(&mut self, events: &mut Vec<TimeEvent>) {
        if self.is_transitioning() {
            self.begin_next(events);
        }
    }

    fn budget_s(&self) -> f32 {
        self.rules.window(self.shift).budget_s()
    }

    /// Spend `s` in-game seconds of the active shift's budget.
    fn charge(&mut self, s: f32, events: &mut Vec<TimeEvent>) {
        let budget = self.budget_s();
        self.used_s += s;
        if self.used_s >= budget {
            self.carry_s += self.used_s - budget;
            self.used_s = budget;
            self.finish_shift(events);
            return;
        }
        let remaining_min = (budget - self.used_s) / MIN_S;
        if !self.warned && remaining_min <= self.rules.low_time_warning_min as f32 {
            self.warned = true;
            events.push(TimeEvent::LowTime {
                day: self.day,
                shift: self.shift,
                remaining_min: remaining_min as u32,
            });
        }
    }

    fn finish_shift(&mut self, events: &mut Vec<TimeEvent>) {
        events.push(TimeEvent::ShiftEnded {
            day: self.day,
            shift: self.shift,
            next: self.shift.next(),
        });
        if self.rules.transition_s > 0.0 {
            self.stage = Stage::Transition { elapsed_s: 0.0 };
        } else {
            self.begin_next(events);
        }
    }

    /// Enter the next shift, skipping whole shifts the carried time covers.
    fn begin_next(&mut self, events: &mut Vec<TimeEvent>) {
        self.advance_shift();
        while self.carry_s >= self.budget_s() {
            self.carry_s -= self.budget_s();
            events.push(TimeEvent::ShiftSkipped {
                day: self.day,
                shift: self.shift,
            });
            self.advance_shift();
        }
        self.used_s = self.carry_s;
        self.carry_s = 0.0;
        self.stage = Stage::Active;
        self.warned = false;
        events.push(TimeEvent::ShiftStarted {
            day: self.day,
            shift: self.shift,
            carried_min: (self.used_s / MIN_S) as u32,
        });
        // Carried time can already put the new shift into its warning window.
        self.charge(0.0, events);
    }

    fn advance_shift(&mut self) {
        self.shift = self.shift.next();
        if self.shift == Shift::Day {
            self.day += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(t: &mut TimeProgression, seconds: f32, events: &mut Vec<TimeEvent>) {
        let dt = 1.0 / 240.0;
        for _ in 0..(seconds * 240.0).round() as u32 {
            t.step(dt, events);
        }
    }

    #[test]
    fn default_rules_validate() {
        assert!(TimeRules::default().validate().is_ok());
        let mut bad = TimeRules::default();
        bad.night.end_h = 19.0;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn clock_follows_budget_through_the_day() {
        let mut t = TimeProgression::new(TimeRules::default());
        let mut ev = Vec::new();
        assert!((t.time_of_day().hours() - 8.0).abs() < 1e-3);
        assert_eq!(t.phase(), DayPhase::Day);
        // 30 real minutes at ratio 10 = 300 game minutes = half the day budget.
        drive(&mut t, 1800.0, &mut ev);
        assert!((t.fraction_used() - 0.5).abs() < 1e-2);
        assert!((t.time_of_day().hours() - 13.0).abs() < 0.05);
        assert!(ev.is_empty());
    }

    #[test]
    fn running_out_plays_dusk_then_starts_night() {
        let mut t = TimeProgression::new(TimeRules::default());
        let mut ev = Vec::new();
        t.spend_minutes(590, &mut ev);
        assert!(matches!(
            ev[..],
            [TimeEvent::LowTime {
                remaining_min: 10,
                ..
            }]
        ));
        ev.clear();
        assert!(t.spend_minutes(10, &mut ev));
        assert!(t.is_transitioning());
        assert_eq!(
            ev,
            vec![TimeEvent::ShiftEnded {
                day: 1,
                shift: Shift::Day,
                next: Shift::Night
            }]
        );
        // Halfway through the 20 s transition the sky is at 19:00 (dusk).
        drive(&mut t, 10.0, &mut ev);
        assert!((t.time_of_day().hours() - 19.0).abs() < 0.05);
        assert_eq!(t.phase(), DayPhase::Dusk);
        ev.clear();
        drive(&mut t, 10.5, &mut ev);
        assert_eq!(t.shift(), Shift::Night);
        assert_eq!(t.day(), 1);
        assert!(matches!(
            ev[0],
            TimeEvent::ShiftStarted {
                shift: Shift::Night,
                ..
            }
        ));
        assert_eq!(t.phase(), DayPhase::Night);
    }

    #[test]
    fn night_wraps_past_midnight_and_dawn_increments_the_day() {
        let mut t = TimeProgression::starting_at(TimeRules::default(), 3, Shift::Night);
        let mut ev = Vec::new();
        t.spend_minutes(300, &mut ev);
        // 20:00 + 5 h = 01:00.
        assert!((t.time_of_day().hours() - 1.0).abs() < 1e-3);
        t.end_shift(&mut ev);
        // Dawn sweeps 04:00 → 08:00.
        drive(&mut t, 10.0, &mut ev);
        assert!((t.time_of_day().hours() - 6.0).abs() < 0.05);
        t.finish_transition(&mut ev);
        assert_eq!((t.day(), t.shift()), (4, Shift::Day));
        assert!((t.time_of_day().hours() - 8.0).abs() < 1e-3);
    }

    #[test]
    fn repair_cost_from_labour_hours() {
        let r = TimeRules::default();
        assert_eq!(r.cost_minutes(Activity::Repair { labour_hours: 2.0 }), 90);
        assert_eq!(r.cost_minutes(Activity::Repair { labour_hours: 0.1 }), 15);
        assert_eq!(r.cost_minutes(Activity::Repair { labour_hours: -1.0 }), 15);
        assert_eq!(r.cost_minutes(Activity::GarageVisit), 20);
        assert_eq!(r.cost_minutes(Activity::Custom { minutes: 7 }), 7);
    }

    #[test]
    fn late_repair_carries_into_next_shift() {
        let mut t = TimeProgression::new(TimeRules::default());
        let mut ev = Vec::new();
        t.spend_minutes(560, &mut ev);
        assert!(!t.fits(Activity::Repair { labour_hours: 2.0 }));
        let r = t.spend(Activity::Repair { labour_hours: 2.0 }, &mut ev);
        assert_eq!(r.minutes, 90);
        assert!(r.ends_shift);
        assert!((t.carried_min() - 50.0).abs() < 1e-3);
        ev.clear();
        t.finish_transition(&mut ev);
        assert_eq!(t.shift(), Shift::Night);
        assert!((t.remaining_min() - 430.0).abs() < 1e-2);
        assert!(matches!(
            ev[0],
            TimeEvent::ShiftStarted {
                carried_min: 50,
                ..
            }
        ));
    }

    #[test]
    fn huge_job_skips_the_night() {
        let mut t = TimeProgression::new(TimeRules::default());
        let mut ev = Vec::new();
        t.spend_minutes(500, &mut ev);
        // 100 min left in the day, 480 night, 30 more into day 2.
        t.spend_minutes(610, &mut ev);
        t.finish_transition(&mut ev);
        assert_eq!((t.day(), t.shift()), (2, Shift::Day));
        assert!((t.remaining_min() - 570.0).abs() < 1e-2);
        assert!(ev.contains(&TimeEvent::ShiftSkipped {
            day: 1,
            shift: Shift::Night
        }));
    }

    #[test]
    fn spending_during_transition_is_carried() {
        let mut t = TimeProgression::new(TimeRules::default());
        let mut ev = Vec::new();
        t.end_shift(&mut ev);
        assert!(t.spend(Activity::GarageVisit, &mut ev).ends_shift);
        t.finish_transition(&mut ev);
        assert!((t.remaining_min() - 460.0).abs() < 1e-2);
    }

    #[test]
    fn instant_transition() {
        let rules = TimeRules {
            transition_s: 0.0,
            ..TimeRules::default()
        };
        let mut t = TimeProgression::new(rules);
        let mut ev = Vec::new();
        t.end_shift(&mut ev);
        assert!(!t.is_transitioning());
        assert_eq!(t.shift(), Shift::Night);
    }

    #[test]
    fn apply_drives_world_clock_and_is_deterministic() {
        let run = || {
            let mut t = TimeProgression::new(TimeRules::default());
            let mut clock = WorldClock::default();
            let mut ev = Vec::new();
            for i in 0..20_000u32 {
                t.step(1.0 / 60.0, &mut ev);
                if i % 997 == 0 {
                    t.spend(Activity::Repair { labour_hours: 1.3 }, &mut ev);
                }
            }
            t.apply(&mut clock);
            (clock.time.seconds_since_midnight.to_bits(), ev)
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn serde_round_trip() {
        let mut t = TimeProgression::new(TimeRules::default());
        t.spend_minutes(123, &mut Vec::new());
        let s = serde_json::to_string(&t).unwrap();
        let back: TimeProgression = serde_json::from_str(&s).unwrap();
        assert_eq!(t, back);
    }
}
