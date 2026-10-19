//! Error types.

use thiserror::Error;

use crate::kind::RacerAccount;

/// A data file under `data/events/` failed to parse or validate.
#[derive(Debug, Error, PartialEq)]
pub enum RaceDataError {
    /// I/O error reading a file.
    #[error("{file}: {msg}")]
    Io {
        /// File name.
        file: String,
        /// Message.
        msg: String,
    },
    /// TOML syntax / shape error.
    #[error("{file}: parse error: {msg}")]
    Parse {
        /// File name.
        file: String,
        /// Message.
        msg: String,
    },
    /// A value is out of range or inconsistent.
    #[error("{file}: {what}: {why}")]
    Invalid {
        /// File name.
        file: String,
        /// Field path.
        what: String,
        /// Reason.
        why: String,
    },
}

impl RaceDataError {
    pub(crate) fn parse(file: &str, e: impl std::fmt::Display) -> Self {
        RaceDataError::Parse {
            file: file.into(),
            msg: e.to_string(),
        }
    }
    pub(crate) fn invalid(file: &str, what: &str, why: &str) -> Self {
        RaceDataError::Invalid {
            file: file.into(),
            what: what.into(),
            why: why.into(),
        }
    }
}

/// Why a racer may not enter an event.
#[derive(Debug, Clone, Error, PartialEq)]
pub enum EntryDenied {
    /// The car's bracket is outside the event's brackets.
    #[error("car is class {car} but the event takes {min}..={max}")]
    WrongBracket {
        /// Car bracket letter.
        car: &'static str,
        /// Lowest allowed.
        min: &'static str,
        /// Highest allowed.
        max: &'static str,
    },
    /// Licence tier too low.
    #[error("needs licence tier {needed}, holds tier {has}")]
    Licence {
        /// Needed tier index (see `ProgressionTable::licence_name`).
        needed: u8,
        /// Held tier index.
        has: u8,
    },
    /// Not enough rep.
    #[error("needs {needed} rep, has {has}")]
    Rep {
        /// Needed.
        needed: i64,
        /// Held.
        has: i64,
    },
    /// Crew-only event and the racer has no crew (or the wrong one).
    #[error("crew event: racer is not in an eligible crew")]
    Crew,
    /// The event is full.
    #[error("event is full")]
    Full,
    /// The lobby is not open.
    #[error("lobby is not open")]
    LobbyClosed,
    /// The racer is already entered.
    #[error("already entered")]
    AlreadyEntered,
}

/// Crew / territory operation errors.
#[derive(Debug, Clone, Error, PartialEq)]
pub enum CrewError {
    /// Unknown crew id.
    #[error("unknown crew")]
    UnknownCrew,
    /// Racer already in a crew.
    #[error("racer {0:?} is already in a crew")]
    AlreadyInCrew(RacerAccount),
    /// Racer is not in that crew.
    #[error("racer {0:?} is not a member")]
    NotMember(RacerAccount),
    /// The crew is full.
    #[error("crew is full")]
    Full,
    /// Name empty, too long or taken.
    #[error("invalid crew name: {0}")]
    BadName(String),
    /// Only the leader may do this.
    #[error("only the crew leader may do this")]
    NotLeader,
    /// Challenge state error.
    #[error("challenge: {0}")]
    Challenge(String),
}
