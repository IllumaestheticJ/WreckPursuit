//! The garage workshop: physical parts on a car, a parts inventory and the parts catalog.
//!
//! The mod tree ([`crate::mods`]) says *what* is installed on a car; the workshop says *where*.
//! A car has **slots** (`data/economy/parts.toml`, [`PartsTable`]): places such as the front
//! bumper, the radiator or the front-left wheel. Each slot holds one part or is empty. A part is
//! either the stock part of its car model or an aftermarket part, which is a mod of one of the
//! slot's mod lines (its **grade**). Taking a part off puts it in the account's inventory as a
//! [`LoosePart`]; fitting takes it from there. Parts are bought into the inventory, never
//! straight onto the car.
//!
//! Rules:
//! - **Removal order.** A slot can only be worked on (removed or fitted) while every slot in
//!   its `blocked_by` list is empty: the bumper comes off before the radiator.
//! - **Mods come from parts.** After every workshop operation the car's mod list is rebuilt
//!   from its fitted parts ([`Economy::derive_mods`]): a fitted grade is an installed mod; a
//!   line on a set of slots (the four wheels) is installed only when every slot of the set
//!   holds that grade. Mod lines that are in no slot (paint, gadgets...) are kept as they are.
//!   Everything that reads `OwnedCar::mods` (preview, class index, the sim) keeps working.
//! - **Condition comes from damage.** A fitted part's condition is `1 - severity` of the
//!   outstanding repair line of its slot (`repair`), or `1 - puncture` for a tyre. Taking the
//!   part off moves that line onto the loose part; fitting a damaged part moves it back. Fitting
//!   a part in perfect condition returns the slot's repair item in [`PartFit::restore`], which
//!   the damage system applies to the stored lattice (`DamageSim::apply_repair`).
//! - **Stock parts fit their model only;** aftermarket parts fit any car the mod allows
//!   (class whitelist, damage mode, requirements).
//! - Cars saved before the workshop have no parts state: [`Economy::car_parts`] derives one
//!   from the mod list (stock parts everywhere, optional slots empty), and the first workshop
//!   operation stores it.
//!
//! Every operation is all-or-nothing, like the rest of the economy API. Cash events reuse the
//! existing reasons until the contract gains part reasons (`docs/integration/garage.md`,
//! contract change request #1): buying an aftermarket part posts `ModInstalled`, a stock part
//! `RepairPaid { labour_multiplier: 0 }`, selling a part `ModRemoved`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wp_contracts::{DamageMode, RepairItem, RepairLine};

use crate::data::Economy;
use crate::error::{invalid, DataError, EconomyError};
use crate::ledger::{transact, CarLocation, CarUid, EconResult, EventReason, Money, OwnedCar};
use crate::mods::ModCatalog;
use crate::PlayerAccount;

// ---------------------------------------------------------------------------------------------
// Data: slots
// ---------------------------------------------------------------------------------------------

/// UI grouping of a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PartGroup {
    /// Bolt-on body panels, lights, kits.
    Body,
    /// Windshield and windows.
    Glass,
    /// Splitters, wings, diffusers.
    Aero,
    /// Radiator, coolers.
    Cooling,
    /// Engine and its bolt-ons.
    Engine,
    /// Battery, engine computer, nitrous.
    Electronics,
    /// Gearbox, clutch, driveshaft, differential.
    Drivetrain,
    /// Fuel tank.
    Fuel,
    /// Wheels and tyres.
    Wheels,
    /// Brakes.
    Brakes,
    /// Springs, dampers, anti-roll bars.
    Suspension,
    /// Steering rack.
    Steering,
    /// Crash beams, bars, plates.
    Protection,
    /// Seats, cage.
    Interior,
}

/// One slot of `data/economy/parts.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotDef {
    /// Unique id (`front_bumper`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// UI grouping.
    pub group: PartGroup,
    /// Set the slot belongs to (`wheels`): every slot of a set takes the same kind of part.
    #[serde(default)]
    pub set: Option<String>,
    /// Repair item that gives the fitted part its condition and that a fresh part restores.
    #[serde(default)]
    pub repair: Option<RepairItem>,
    /// Corner (0–3: FL, FR, RL, RR) whose tyre puncture gives the condition.
    #[serde(default)]
    pub puncture: Option<u8>,
    /// Mod lines whose parts fit here.
    #[serde(default)]
    pub lines: Vec<String>,
    /// The car cannot leave the garage while this slot is empty.
    #[serde(default)]
    pub required: bool,
    /// Slots that must be empty before this one can be worked on.
    #[serde(default)]
    pub blocked_by: Vec<String>,
    /// Price of a stock part when the slot has no `repair` item.
    #[serde(default)]
    pub stock_price: Option<u32>,
    /// No stock part: empty on a stock car, takes aftermarket parts only.
    #[serde(default)]
    pub optional: bool,
}

impl SlotDef {
    /// Kind of part the slot takes: the set id, or the slot id.
    pub fn kind(&self) -> &str {
        self.set.as_deref().unwrap_or(&self.id)
    }

    /// True when `item` is this slot's repair line (a detached panel counts for its region).
    pub fn owns_line(&self, item: &RepairItem) -> bool {
        match (self.repair, item) {
            (Some(own), it) if own == *it => true,
            (Some(RepairItem::Region(r)), RepairItem::DetachedPart(d)) => r == *d,
            _ => false,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PartsFile {
    slot: Vec<SlotDef>,
}

/// All slots, in file order (`data/economy/parts.toml`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PartsTable {
    /// Slots in display order.
    pub slots: Vec<SlotDef>,
}

impl PartsTable {
    /// Parse `parts.toml` (structure only; [`PartsTable::validate`] checks it).
    pub fn parse(text: &str) -> Result<Self, DataError> {
        let f: PartsFile = toml::from_str(text).map_err(|source| DataError::Toml {
            name: "economy/parts.toml".into(),
            source,
        })?;
        Ok(Self { slots: f.slot })
    }

    /// Slot by id.
    pub fn slot(&self, id: &str) -> Option<&SlotDef> {
        self.slots.iter().find(|s| s.id == id)
    }

    /// Slots taking parts of `kind` (one slot, or every slot of a set).
    pub fn slots_of_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a SlotDef> + 'a {
        self.slots.iter().filter(move |s| s.kind() == kind)
    }

    /// The first slot of the kind a mod line fits, if any.
    pub fn slot_for_line(&self, line: &str) -> Option<&SlotDef> {
        self.slots
            .iter()
            .find(|s| s.lines.iter().any(|l| l == line))
    }

    /// Every mod line that lives in a slot.
    pub fn slotted_lines(&self) -> BTreeSet<&str> {
        self.slots
            .iter()
            .flat_map(|s| s.lines.iter().map(String::as_str))
            .collect()
    }

    /// Check ids, references, the removal order and the mod lines against the catalog.
    pub fn validate(&self, catalog: &ModCatalog) -> Result<(), DataError> {
        let mut ids = BTreeSet::new();
        for s in &self.slots {
            let w = format!("economy/parts.toml slot {}", s.id);
            if !ids.insert(s.id.as_str()) {
                return Err(invalid(w, "duplicate id"));
            }
            if s.repair.is_some() && s.puncture.is_some() {
                return Err(invalid(w, "repair and puncture are exclusive"));
            }
            if s.puncture.is_some_and(|c| c > 3) {
                return Err(invalid(w, "puncture corner must be 0..=3"));
            }
            if s.optional {
                if s.required || s.repair.is_some() || s.stock_price.is_some() {
                    return Err(invalid(
                        w,
                        "an optional slot has no stock part (no repair, stock_price, required)",
                    ));
                }
                if s.lines.is_empty() {
                    return Err(invalid(w, "an optional slot needs a mod line"));
                }
            } else if s.repair.is_none() && s.stock_price.is_none() {
                return Err(invalid(
                    w,
                    "a stock part needs a repair item or a stock_price",
                ));
            }
        }
        let mut line_kind: BTreeMap<&str, &str> = BTreeMap::new();
        for s in &self.slots {
            let w = format!("economy/parts.toml slot {}", s.id);
            for b in &s.blocked_by {
                if b == &s.id || !ids.contains(b.as_str()) {
                    return Err(invalid(&w, format!("blocked_by {b:?} is not another slot")));
                }
            }
            for l in &s.lines {
                if !catalog.iter().any(|m| &m.line == l) {
                    return Err(invalid(&w, format!("unknown mod line {l:?}")));
                }
                if let Some(k) = line_kind.insert(l, s.kind()) {
                    if k != s.kind() {
                        return Err(invalid(&w, format!("line {l:?} is also in {k:?}")));
                    }
                }
            }
            if let Some(set) = &s.set {
                let first = self.slots_of_kind(set).next().expect("s is in its set");
                if first.lines != s.lines || first.group != s.group || first.optional != s.optional
                {
                    return Err(invalid(&w, format!("differs from the rest of set {set:?}")));
                }
            }
        }
        // The removal order must be a DAG, or some slots could never be worked on.
        let mut state: BTreeMap<&str, u8> = BTreeMap::new();
        fn visit<'a>(
            t: &'a PartsTable,
            id: &'a str,
            state: &mut BTreeMap<&'a str, u8>,
        ) -> Result<(), DataError> {
            match state.get(id) {
                Some(2) => return Ok(()),
                Some(1) => {
                    return Err(invalid(
                        "economy/parts.toml",
                        format!("blocked_by has a cycle through {id:?}"),
                    ))
                }
                _ => {}
            }
            state.insert(id, 1);
            for b in &t.slot(id).expect("ids checked").blocked_by {
                visit(t, b, state)?;
            }
            state.insert(id, 2);
            Ok(())
        }
        for s in &self.slots {
            visit(self, &s.id, &mut state)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Account state
// ---------------------------------------------------------------------------------------------

/// A part fitted in a slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FittedPart {
    /// Mod id of an aftermarket part; `None` = the car's stock part.
    pub grade: Option<String>,
}

impl FittedPart {
    /// The stock part.
    pub fn stock() -> Self {
        Self { grade: None }
    }
}

/// What is in each slot of one car. A slot missing from the map has its default content
/// (derived from the mod list, see [`Economy::car_parts`]).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CarParts {
    /// Slot id → fitted part (`None` = empty).
    pub slots: BTreeMap<String, Option<FittedPart>>,
}

impl CarParts {
    /// The part in `slot`, if one is fitted.
    pub fn fitted(&self, slot: &str) -> Option<&FittedPart> {
        self.slots.get(slot).and_then(Option::as_ref)
    }
}

/// A part in the inventory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoosePart {
    /// Account-unique id.
    pub uid: u32,
    /// Kind of slot it fits ([`SlotDef::kind`]).
    pub kind: String,
    /// Mod id of an aftermarket part; `None` = a stock part.
    pub grade: Option<String>,
    /// Car model (`VehicleDef::id`) a stock part belongs to; `None` for aftermarket parts.
    pub model: Option<String>,
    /// Condition 0..=1 (1 = perfect).
    pub condition: f32,
    /// Repair lines the part carries from the car it came off (put back when it is fitted).
    #[serde(default)]
    pub damage: Vec<RepairLine>,
    /// What the part is worth new (resale is a fraction of it, scaled by condition).
    pub value: Money,
}

/// Result of [`Economy::fit_part`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PartFit {
    /// Repair items the damage system must restore on the stored lattice (a fresh part).
    pub restore: Vec<RepairItem>,
}

// ---------------------------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------------------------

/// One slot as the workshop shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotView {
    /// Slot id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// UI grouping.
    pub group: PartGroup,
    /// Kind of part it takes.
    pub kind: String,
    /// Fitted part (`None` = empty).
    pub fitted: Option<PartView>,
    /// The car cannot leave while the slot is empty.
    pub required: bool,
    /// Blocking slots that still hold a part (empty = the slot can be worked on).
    pub blocked_by: Vec<String>,
    /// Why the fitted part cannot come off now (`None` = it can).
    pub remove_blocker: Option<String>,
}

/// A part as the workshop shows it (fitted or loose).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartView {
    /// Mod id, `None` for stock.
    pub grade: Option<String>,
    /// Display name ("Stock" or the mod's name).
    pub name: String,
    /// Mod tier (0 = stock).
    pub tier: u8,
    /// Condition 0..=1.
    pub condition: f32,
}

/// A loose part, with the empty slots of the car it would fit now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InventoryView {
    /// The part.
    pub part: LoosePart,
    /// Display name.
    pub name: String,
    /// Empty slots of the car that take it with nothing in the way.
    pub fits: Vec<String>,
    /// Why it fits nowhere on this car (`None` when `fits` is not empty).
    pub reason: Option<String>,
    /// Dealer price for it now.
    pub sale_price: Money,
}

/// One line of the parts catalog for a car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// Kind of part.
    pub kind: String,
    /// Slot (or first slot of the set) name.
    pub slot_name: String,
    /// Group.
    pub group: PartGroup,
    /// Mod id, `None` for the stock replacement.
    pub grade: Option<String>,
    /// Display name.
    pub name: String,
    /// Tier (0 = stock).
    pub tier: u8,
    /// Price of what one purchase brings.
    pub price: Money,
    /// Parts one purchase brings (4 for a set of wheels bought as an upgrade).
    pub count: u32,
    /// `None` = can be bought for this car, else why not.
    pub reason: Option<String>,
}

/// The whole workshop screen for one car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkshopView {
    /// Car.
    pub car: CarUid,
    /// Every slot, in table order.
    pub slots: Vec<SlotView>,
    /// Names of required slots that are empty (the car cannot leave).
    pub missing: Vec<String>,
    /// Installed mods that are not parts (paint, gadgets...).
    pub upgrades: Vec<String>,
    /// The account's inventory, with where each part fits on this car.
    pub inventory: Vec<InventoryView>,
}

// ---------------------------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------------------------

fn refuse(slot: &str, reason: impl Into<String>) -> EconomyError {
    EconomyError::PartRefused {
        slot: slot.to_string(),
        reason: reason.into(),
    }
}

fn round(v: f32) -> Money {
    v.round().max(0.0) as Money
}

impl Economy {
    /// What is in each slot of `car`: its stored parts state, with any slot it does not
    /// mention derived from the mod list (a grade if a mod of the slot's lines is installed,
    /// else the stock part, or nothing for an optional slot).
    pub fn car_parts(&self, car: &OwnedCar) -> CarParts {
        let mut out = CarParts::default();
        for s in &self.parts.slots {
            let stored = car.parts.as_ref().and_then(|p| p.slots.get(&s.id));
            let content = match stored {
                Some(c) => c.clone(),
                None => self.default_part(s, &car.mods),
            };
            out.slots.insert(s.id.clone(), content);
        }
        out
    }

    fn default_part(&self, s: &SlotDef, mods: &[String]) -> Option<FittedPart> {
        let grade = mods.iter().find(|m| {
            self.catalog
                .get(m)
                .is_some_and(|d| s.lines.contains(&d.line))
        });
        match grade {
            Some(g) => Some(FittedPart {
                grade: Some(g.clone()),
            }),
            None if s.optional => None,
            None => Some(FittedPart::stock()),
        }
    }

    /// The mod list a car with `parts` has: `mods` minus every slotted line, plus the
    /// fitted grades (a set's line only when every slot of the set holds the same grade).
    pub fn derive_mods(&self, mods: &[String], parts: &CarParts) -> Vec<String> {
        let slotted = self.parts.slotted_lines();
        let mut out: BTreeSet<String> = mods
            .iter()
            .filter(|m| {
                self.catalog
                    .get(m)
                    .is_none_or(|d| !slotted.contains(d.line.as_str()))
            })
            .cloned()
            .collect();
        let mut sets: BTreeMap<&str, Vec<Option<&str>>> = BTreeMap::new();
        for s in &self.parts.slots {
            let grade = parts.fitted(&s.id).and_then(|f| f.grade.as_deref());
            match &s.set {
                Some(set) => sets.entry(set).or_default().push(grade),
                None => {
                    if let Some(g) = grade {
                        out.insert(g.to_string());
                    }
                }
            }
        }
        for grades in sets.values() {
            if let Some(Some(g)) = grades.first() {
                if grades.iter().all(|x| *x == Some(*g)) {
                    out.insert(g.to_string());
                }
            }
        }
        out.into_iter().collect()
    }

    /// Condition 0..=1 of whatever is fitted in `slot` on `car`, read from its damage.
    pub fn slot_condition(&self, car: &OwnedCar, slot: &SlotDef) -> f32 {
        if let Some(c) = slot.puncture {
            return 1.0 - car.damage.handling.tire_puncture[c as usize].clamp(0.0, 1.0);
        }
        let worst = car
            .damage
            .outstanding
            .lines
            .iter()
            .filter(|l| slot.owns_line(&l.item))
            .map(|l| match l.item {
                RepairItem::DetachedPart(_) => 1.0,
                _ => l.severity,
            })
            .fold(0.0f32, f32::max);
        1.0 - worst.clamp(0.0, 1.0)
    }

    /// Price of a new part of `kind` with `grade` for `car` (one part; an upgrade bought for a
    /// set costs the mod price for all of them, see [`Economy::buy_part`]).
    fn new_part_value(&self, car: &OwnedCar, slot: &SlotDef, grade: Option<&str>) -> Money {
        match grade {
            Some(g) => {
                let price = self.catalog.get(g).map_or(0, |m| m.price) as Money;
                let n = self.parts.slots_of_kind(slot.kind()).count().max(1) as Money;
                price / n
            }
            None => match slot.repair {
                Some(item) => {
                    let t = self.price_table_for(car);
                    let v = match item {
                        RepairItem::Region(r) | RepairItem::DetachedPart(r) => {
                            t.region_replace_cost.get(&r).copied()
                        }
                        RepairItem::Component(c) => t.component_replace_cost.get(&c).copied(),
                    };
                    round(v.unwrap_or(0.0))
                }
                None => slot.stock_price.unwrap_or(0) as Money,
            },
        }
    }

    fn part_name(&self, grade: Option<&str>) -> (String, u8) {
        match grade.and_then(|g| self.catalog.get(g)) {
            Some(m) => (m.name.clone(), m.tier),
            None => ("Stock".into(), 0),
        }
    }

    /// Fitted slots in `slot`'s `blocked_by` list.
    fn blockers(&self, parts: &CarParts, slot: &SlotDef) -> Vec<String> {
        slot.blocked_by
            .iter()
            .filter(|b| parts.fitted(b).is_some())
            .cloned()
            .collect()
    }

    fn blocker_reason(&self, blockers: &[String]) -> String {
        let names: Vec<&str> = blockers
            .iter()
            .filter_map(|b| self.parts.slot(b).map(|s| s.name.as_str()))
            .collect();
        format!("take off first: {}", names.join(", "))
    }

    fn workshop_car<'a>(
        &self,
        acct: &'a PlayerAccount,
        car: CarUid,
    ) -> Result<&'a OwnedCar, EconomyError> {
        let c = acct.try_car(car)?;
        if c.location != CarLocation::Garage {
            return Err(EconomyError::CarUnavailable {
                car,
                reason: "in the pound".into(),
            });
        }
        Ok(c)
    }

    fn try_slot(&self, slot: &str) -> Result<&SlotDef, EconomyError> {
        self.parts
            .slot(slot)
            .ok_or_else(|| refuse(slot, "no such slot"))
    }

    /// Check a mod list a workshop change would leave, as the mod tree would.
    fn check_mods(
        &self,
        car: &OwnedCar,
        slot: &str,
        before: &[String],
        next: &[String],
    ) -> Result<(), EconomyError> {
        match self.catalog.validate_set(car.class, next) {
            Ok(_) => Ok(()),
            Err(EconomyError::MissingRequirement { mod_id, requires }) => {
                // A requirement that was already broken is not this change's fault.
                if self.catalog.validate_set(car.class, before).is_err() {
                    return Ok(());
                }
                Err(refuse(slot, format!("{mod_id} needs {requires}")))
            }
            Err(e) => Err(e),
        }
    }

    /// Names of the required slots that are empty on `car` (empty = ready to drive).
    pub fn missing_parts(&self, car: &OwnedCar) -> Vec<String> {
        let parts = self.car_parts(car);
        self.parts
            .slots
            .iter()
            .filter(|s| s.required && parts.fitted(&s.id).is_none())
            .map(|s| s.name.clone())
            .collect()
    }

    /// Why loose part `p` cannot go in `slot` on `car` with `parts` (`None` = it can).
    fn fit_blocker(
        &self,
        car: &OwnedCar,
        parts: &CarParts,
        slot: &SlotDef,
        p: &LoosePart,
        mode: DamageMode,
    ) -> Option<String> {
        if p.kind != slot.kind() {
            return Some("wrong kind of part".into());
        }
        if parts.fitted(&slot.id).is_some() {
            return Some("the slot is not empty".into());
        }
        let blockers = self.blockers(parts, slot);
        if !blockers.is_empty() {
            return Some(self.blocker_reason(&blockers));
        }
        match &p.grade {
            None => {
                if p.model.as_deref() != Some(car.def_id.as_str()) {
                    return Some("a stock part of another model".into());
                }
                if slot.optional {
                    return Some("no stock part goes here".into());
                }
            }
            Some(g) => {
                let Some(m) = self.catalog.get(g) else {
                    return Some(format!("unknown mod {g:?}"));
                };
                if !m.allows_class(car.class) {
                    return Some(format!("not for class {:?}", car.class));
                }
                if !m.allows_mode(mode) {
                    return Some(format!("not available in {mode:?} mode"));
                }
            }
        }
        None
    }

    /// The workshop screen for `car`: slots with condition and blockers, missing parts,
    /// upgrades, and the inventory with where each part would fit.
    pub fn workshop(
        &self,
        acct: &PlayerAccount,
        car: CarUid,
        mode: DamageMode,
    ) -> Result<WorkshopView, EconomyError> {
        let c = acct.try_car(car)?;
        let parts = self.car_parts(c);
        let slotted = self.parts.slotted_lines();
        let slots = self
            .parts
            .slots
            .iter()
            .map(|s| {
                let fitted = parts.fitted(&s.id).map(|f| {
                    let (name, tier) = self.part_name(f.grade.as_deref());
                    PartView {
                        grade: f.grade.clone(),
                        name,
                        tier,
                        condition: self.slot_condition(c, s),
                    }
                });
                let blocked_by = self.blockers(&parts, s);
                let remove_blocker = match &fitted {
                    None => Some("nothing fitted".into()),
                    Some(_) => match self.check_remove(c, &parts, s) {
                        Ok(()) => None,
                        Err(EconomyError::PartRefused { reason, .. }) => Some(reason),
                        Err(e) => Some(e.to_string()),
                    },
                };
                SlotView {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    group: s.group,
                    kind: s.kind().to_string(),
                    fitted,
                    required: s.required,
                    blocked_by,
                    remove_blocker,
                }
            })
            .collect();
        let upgrades = c
            .mods
            .iter()
            .filter(|m| {
                self.catalog
                    .get(m)
                    .is_none_or(|d| !slotted.contains(d.line.as_str()))
            })
            .cloned()
            .collect();
        let inventory = acct
            .inventory
            .iter()
            .map(|p| {
                let mut fits = Vec::new();
                let mut why = None;
                for s in self.parts.slots_of_kind(&p.kind) {
                    match self.fit_blocker(c, &parts, s, p, mode) {
                        None => fits.push(s.id.clone()),
                        Some(r) => {
                            why.get_or_insert(r);
                        }
                    }
                }
                let reason = if fits.is_empty() {
                    Some(why.unwrap_or_else(|| "fits no slot of this car".into()))
                } else {
                    None
                };
                let (name, _) = self.part_name(p.grade.as_deref());
                InventoryView {
                    part: p.clone(),
                    name,
                    fits,
                    reason,
                    sale_price: self.part_sale_price(p),
                }
            })
            .collect();
        Ok(WorkshopView {
            car,
            slots,
            missing: self.missing_parts(c),
            upgrades,
            inventory,
        })
    }

    fn check_remove(
        &self,
        car: &OwnedCar,
        parts: &CarParts,
        slot: &SlotDef,
    ) -> Result<(), EconomyError> {
        if parts.fitted(&slot.id).is_none() {
            return Err(refuse(&slot.id, "nothing fitted"));
        }
        let blockers = self.blockers(parts, slot);
        if !blockers.is_empty() {
            return Err(refuse(&slot.id, self.blocker_reason(&blockers)));
        }
        let mut next = parts.clone();
        next.slots.insert(slot.id.clone(), None);
        let next_mods = self.derive_mods(&car.mods, &next);
        self.check_mods(car, &slot.id, &car.mods, &next_mods)
    }

    /// Take the part in `slot` off `car` into the inventory. Its repair lines (or tyre
    /// puncture) go with it. Returns the loose part's uid. No cash changes.
    pub fn remove_part(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        slot: &str,
    ) -> EconResult<u32> {
        let c = self.workshop_car(acct, car)?;
        let s = self.try_slot(slot)?;
        let parts = self.car_parts(c);
        self.check_remove(c, &parts, s)?;
        let fitted = parts.fitted(slot).cloned().expect("checked");
        let condition = self.slot_condition(c, s);
        let value = self.new_part_value(c, s, fitted.grade.as_deref());
        transact(acct, |tx| {
            let uid = tx.acct.next_part_uid.max(1);
            tx.acct.next_part_uid = uid + 1;
            let c = tx.car_mut(car)?;
            let (damage, keep): (Vec<RepairLine>, Vec<RepairLine>) = c
                .damage
                .outstanding
                .lines
                .drain(..)
                .partition(|l| s.owns_line(&l.item));
            c.damage.outstanding.lines = keep;
            c.damage.outstanding.total = c.damage.outstanding.lines.iter().map(|l| l.total).sum();
            if let Some(corner) = s.puncture {
                c.damage.handling.tire_puncture[corner as usize] = 0.0;
            }
            let mut next = parts.clone();
            next.slots.insert(slot.to_string(), None);
            c.mods = self.derive_mods(&c.mods, &next);
            c.parts = Some(next);
            let model = fitted.grade.is_none().then(|| c.def_id.clone());
            tx.acct.inventory.push(LoosePart {
                uid,
                kind: s.kind().to_string(),
                grade: fitted.grade,
                model,
                condition,
                damage,
                value,
            });
            Ok(uid)
        })
    }

    /// Fit inventory part `part` into the empty `slot` of `car`. A damaged part brings its
    /// repair lines back onto the car; a part in perfect condition returns the slot's repair
    /// item in [`PartFit::restore`] for the stored lattice.
    pub fn fit_part(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        slot: &str,
        part: u32,
        mode: DamageMode,
    ) -> EconResult<PartFit> {
        let c = self.workshop_car(acct, car)?;
        let s = self.try_slot(slot)?;
        let p = acct
            .inventory
            .iter()
            .find(|p| p.uid == part)
            .ok_or(EconomyError::UnknownPart(part))?
            .clone();
        let parts = self.car_parts(c);
        if let Some(why) = self.fit_blocker(c, &parts, s, &p, mode) {
            return Err(refuse(slot, why));
        }
        let mut next = parts.clone();
        next.slots.insert(
            slot.to_string(),
            Some(FittedPart {
                grade: p.grade.clone(),
            }),
        );
        let next_mods = self.derive_mods(&c.mods, &next);
        self.check_mods(c, slot, &c.mods, &next_mods)?;
        transact(acct, |tx| {
            tx.acct.inventory.retain(|x| x.uid != part);
            let c = tx.car_mut(car)?;
            let fresh = p.damage.is_empty() && p.condition >= 1.0;
            let mut restore = Vec::new();
            if fresh {
                c.damage.outstanding.lines.retain(|l| !s.owns_line(&l.item));
                restore.extend(s.repair);
            } else {
                c.damage.outstanding.lines.extend(p.damage.iter().cloned());
            }
            c.damage.outstanding.total = c.damage.outstanding.lines.iter().map(|l| l.total).sum();
            if let Some(corner) = s.puncture {
                c.damage.handling.tire_puncture[corner as usize] =
                    (1.0 - p.condition).clamp(0.0, 1.0);
            }
            c.mods = next_mods;
            c.parts = Some(next);
            Ok(PartFit { restore })
        })
    }

    /// The parts catalog for `car`: a stock replacement for every non-optional kind, and every
    /// aftermarket grade of the kind's mod lines, with the reason it cannot be bought.
    pub fn part_catalog(
        &self,
        acct: &PlayerAccount,
        car: CarUid,
        mode: DamageMode,
    ) -> Result<Vec<CatalogEntry>, EconomyError> {
        let c = acct.try_car(car)?;
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for s in &self.parts.slots {
            if !seen.insert(s.kind()) {
                continue;
            }
            let count = self.parts.slots_of_kind(s.kind()).count() as u32;
            let slot_name = match &s.set {
                Some(set) => set.replace('_', " "),
                None => s.name.clone(),
            };
            if !s.optional {
                out.push(CatalogEntry {
                    kind: s.kind().to_string(),
                    slot_name: slot_name.clone(),
                    group: s.group,
                    grade: None,
                    name: format!("{} (stock)", s.name),
                    tier: 0,
                    price: self.new_part_value(c, s, None),
                    count: 1,
                    reason: None,
                });
            }
            let mut grades: Vec<_> = self
                .catalog
                .iter()
                .filter(|m| s.lines.contains(&m.line))
                .collect();
            grades.sort_by(|a, b| (&a.line, a.tier, &a.id).cmp(&(&b.line, b.tier, &b.id)));
            for m in grades {
                let reason = if !m.allows_class(c.class) {
                    Some(format!("not for class {:?}", c.class))
                } else if !m.allows_mode(mode) {
                    Some(format!("not available in {mode:?} mode"))
                } else {
                    None
                };
                out.push(CatalogEntry {
                    kind: s.kind().to_string(),
                    slot_name: slot_name.clone(),
                    group: s.group,
                    grade: Some(m.id.clone()),
                    name: m.name.clone(),
                    tier: m.tier,
                    price: m.price as Money,
                    count,
                    reason,
                });
            }
        }
        Ok(out)
    }

    /// Buy a new part of `kind` for `car` into the inventory: the stock replacement
    /// (`grade = None`, one part for the car's model) or an aftermarket grade (a whole set for
    /// a set kind, at the mod's price). Returns the new parts' uids.
    pub fn buy_part(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        kind: &str,
        grade: Option<&str>,
        mode: DamageMode,
    ) -> EconResult<Vec<u32>> {
        let c = acct.try_car(car)?;
        let s = self
            .parts
            .slots_of_kind(kind)
            .next()
            .ok_or_else(|| refuse(kind, "no such kind of part"))?;
        let (price, count, reason) = match grade {
            None => {
                if s.optional {
                    return Err(refuse(kind, "there is no stock part for this slot"));
                }
                (
                    self.new_part_value(c, s, None),
                    1,
                    EventReason::RepairPaid {
                        lines: 1,
                        labour_multiplier: 0.0,
                    },
                )
            }
            Some(g) => {
                let m = self.catalog.try_get(g)?;
                if !s.lines.contains(&m.line) {
                    return Err(refuse(kind, format!("{g} does not fit here")));
                }
                if !m.allows_class(c.class) || !m.allows_mode(mode) {
                    return Err(EconomyError::ModNotAllowed {
                        mod_id: g.to_string(),
                        reason: format!("not for this car in {mode:?} mode"),
                    });
                }
                (
                    m.price as Money,
                    self.parts.slots_of_kind(kind).count(),
                    EventReason::ModInstalled {
                        mod_id: g.to_string(),
                    },
                )
            }
        };
        let value = self.new_part_value(c, s, grade);
        let model = grade.is_none().then(|| c.def_id.clone());
        transact(acct, |tx| {
            tx.spend(price, reason, Some(car))?;
            let mut uids = Vec::new();
            for _ in 0..count {
                let uid = tx.acct.next_part_uid.max(1);
                tx.acct.next_part_uid = uid + 1;
                tx.acct.inventory.push(LoosePart {
                    uid,
                    kind: kind.to_string(),
                    grade: grade.map(str::to_string),
                    model: model.clone(),
                    condition: 1.0,
                    damage: Vec::new(),
                    value,
                });
                uids.push(uid);
            }
            Ok(uids)
        })
    }

    /// What the dealer pays for a loose part: `mod_resale_frac × value × condition`.
    pub fn part_sale_price(&self, p: &LoosePart) -> Money {
        round(self.market.mod_resale_frac * p.value as f32 * p.condition.clamp(0.0, 1.0))
    }

    /// Sell (or scrap, at condition 0) a part from the inventory. Returns the price.
    pub fn sell_part(&self, acct: &mut PlayerAccount, part: u32) -> EconResult<Money> {
        let p = acct
            .inventory
            .iter()
            .find(|p| p.uid == part)
            .ok_or(EconomyError::UnknownPart(part))?
            .clone();
        let price = self.part_sale_price(&p);
        transact(acct, |tx| {
            tx.acct.inventory.retain(|x| x.uid != part);
            let mod_id = p
                .grade
                .clone()
                .unwrap_or_else(|| format!("stock_{}", p.kind));
            tx.credit(price, EventReason::ModRemoved { mod_id }, None);
            Ok(price)
        })
    }

    /// Keep a car's stored parts in step after the mod tree changed its mods (the shop fits
    /// and removes the parts itself): slots of a line whose mod is installed get that grade;
    /// a grade that is no longer installed goes back to stock (empty if optional), except in
    /// a set whose slots hold different parts (a workshop state the mod list cannot express).
    pub(crate) fn sync_parts_with_mods(&self, car: &mut OwnedCar) {
        let Some(parts) = car.parts.as_ref() else {
            return;
        };
        let mut next = parts.clone();
        for s in &self.parts.slots {
            let want = self.default_part(s, &car.mods);
            let have = parts.slots.get(&s.id).cloned().flatten();
            let have_grade = have.as_ref().and_then(|f| f.grade.as_deref());
            let want_grade = want.as_ref().and_then(|f| f.grade.as_deref());
            let update = match (have_grade, want_grade) {
                (_, Some(w)) => have_grade != Some(w),
                (Some(h), None) => !car.mods.iter().any(|m| m == h) && !self.set_mixed(parts, s),
                (None, None) => false,
            };
            if update {
                next.slots.insert(s.id.clone(), want);
            }
        }
        car.parts = Some(next);
    }

    /// Drop the repair lines of empty slots from a freshly recorded estimate: the damaged
    /// part is in the inventory and carries them (the stored lattice still has its dents).
    pub(crate) fn strip_empty_slot_lines(&self, car: &mut OwnedCar) {
        let Some(parts) = car.parts.as_ref() else {
            return;
        };
        let empty: Vec<&SlotDef> = self
            .parts
            .slots
            .iter()
            .filter(|s| s.repair.is_some() && parts.fitted(&s.id).is_none())
            .collect();
        if empty.is_empty() {
            return;
        }
        let o = &mut car.damage.outstanding;
        o.lines
            .retain(|l| !empty.iter().any(|s| s.owns_line(&l.item)));
        o.total = o.lines.iter().map(|l| l.total).sum();
    }

    fn set_mixed(&self, parts: &CarParts, s: &SlotDef) -> bool {
        if s.set.is_none() {
            return false;
        }
        let grades: BTreeSet<Option<&str>> = self
            .parts
            .slots_of_kind(s.kind())
            .map(|x| parts.fitted(&x.id).and_then(|f| f.grade.as_deref()))
            .collect();
        grades.len() > 1
    }
}
