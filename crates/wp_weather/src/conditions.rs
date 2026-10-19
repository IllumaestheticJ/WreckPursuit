//! `LocalConditions`: everything other systems need to know about the weather at one point.
//!
//! Since contract v0.3.0 (W-1) the type lives in `wp_contracts` and is the answer of
//! `WorldQuery::weather_at`; re-exported here so `wp_weather::conditions::LocalConditions`
//! keeps working.

pub use wp_contracts::LocalConditions;
