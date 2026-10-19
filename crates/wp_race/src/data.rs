//! Loading `data/events/*.toml` into one validated [`RaceData`].
//!
//! The files are embedded at compile time ([`RaceData::embedded`]) so the browser build needs no
//! file access; native servers may load a modded copy from disk with [`RaceData::load_dir`].

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::crew::CrewConfig;
use crate::error::RaceDataError;
use crate::progression::ProgressionTable;
use crate::rules::RulesTable;
use crate::schedule::ScheduleRules;
use crate::stakes::StakeTable;
use crate::territory::TerritoryConfig;

/// `crews.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrewsFile {
    /// Crew settings.
    pub crew: CrewConfig,
    /// Territory settings.
    pub territory: TerritoryConfig,
}

/// Every `data/events/` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceData {
    /// `rules.toml`.
    pub rules: RulesTable,
    /// `stakes.toml`.
    pub stakes: StakeTable,
    /// `schedule.toml`.
    pub schedule: ScheduleRules,
    /// `progression.toml`.
    pub progression: ProgressionTable,
    /// `crews.toml` `[crew]`.
    pub crew: CrewConfig,
    /// `crews.toml` `[territory]`.
    pub territory: TerritoryConfig,
}

/// Raw file contents, one per table.
#[derive(Debug, Clone, Copy)]
pub struct RaceDataSources<'a> {
    /// `rules.toml`.
    pub rules: &'a str,
    /// `stakes.toml`.
    pub stakes: &'a str,
    /// `schedule.toml`.
    pub schedule: &'a str,
    /// `progression.toml`.
    pub progression: &'a str,
    /// `crews.toml`.
    pub crews: &'a str,
}

impl RaceDataSources<'static> {
    /// The repository's `data/events/` files, embedded at compile time.
    pub const EMBEDDED: RaceDataSources<'static> = RaceDataSources {
        rules: include_str!("../../../data/events/rules.toml"),
        stakes: include_str!("../../../data/events/stakes.toml"),
        schedule: include_str!("../../../data/events/schedule.toml"),
        progression: include_str!("../../../data/events/progression.toml"),
        crews: include_str!("../../../data/events/crews.toml"),
    };
}

impl RaceData {
    /// Parse and validate every table.
    pub fn parse(src: RaceDataSources<'_>) -> Result<RaceData, RaceDataError> {
        let crews: CrewsFile =
            toml::from_str(src.crews).map_err(|e| RaceDataError::parse("crews.toml", e))?;
        crews.crew.validate()?;
        crews.territory.validate()?;
        Ok(RaceData {
            rules: RulesTable::from_toml(src.rules)?,
            stakes: StakeTable::from_toml(src.stakes)?,
            schedule: ScheduleRules::from_toml(src.schedule)?,
            progression: ProgressionTable::from_toml(src.progression)?,
            crew: crews.crew,
            territory: crews.territory,
        })
    }

    /// The embedded repository data (panics only if the shipped data is invalid, which the
    /// crate's tests rule out).
    pub fn embedded() -> RaceData {
        RaceData::parse(RaceDataSources::EMBEDDED).expect("data/events/*.toml is valid")
    }

    /// Load from a directory containing the five files.
    pub fn load_dir(dir: impl AsRef<Path>) -> Result<RaceData, RaceDataError> {
        let dir = dir.as_ref();
        let read = |name: &str| {
            std::fs::read_to_string(dir.join(name)).map_err(|e| RaceDataError::Io {
                file: name.into(),
                msg: e.to_string(),
            })
        };
        let (rules, stakes, schedule, progression, crews) = (
            read("rules.toml")?,
            read("stakes.toml")?,
            read("schedule.toml")?,
            read("progression.toml")?,
            read("crews.toml")?,
        );
        RaceData::parse(RaceDataSources {
            rules: &rules,
            stakes: &stakes,
            schedule: &schedule,
            progression: &progression,
            crews: &crews,
        })
    }
}
