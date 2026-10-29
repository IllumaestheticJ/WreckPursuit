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
//! - **Assemblies.** A slot with a `parent` lives inside that assembly slot (engine internals
//!   inside the engine). Removing the assembly takes its fitted children with it as the loose
//!   part's [`LoosePart::contents`]; fitting it puts them back. Children can be worked on in the
//!   car, in removal order, or on the **engine stand** ([`Economy::strip_part`],
//!   [`Economy::build_part`]) while the assembly is loose. Stock children belong to an engine
//!   family (the stock engine's car model, or the swap engine's mod id) and fit only it.
//! - **Mounted parts.** A slot with a `mount` holds a part bolted onto the part in that slot
//!   (a splitter on the front bumper, a wing on the trunk lid). It can be worked on while its
//!   host is on, needs the host on to be fitted, and comes off with the host (in the loose
//!   host's [`LoosePart::contents`]) and goes back on with it: nothing is left floating.
//! - **Wear.** Assembly members (the block and its children) carry wear 0..=1 instead of reading
//!   their condition from a repair line. When a damage report raises the assembly's repair-line
//!   severity by `d`, each member's wear rises by `d × damage_share`. A shop repair of that line
//!   rebuilds the assembly (all wear 0). Doing it yourself: once every member is within its
//!   `wear_limit`, the line is cleared and the lattice item restored ([`PartFit::restore`]).
//!   While no line is open, worn members cost torque ([`Economy::wear_torque_factor`]).
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
    /// Assembly slot this part lives inside (engine internals: `engine`).
    #[serde(default)]
    pub parent: Option<String>,
    /// Slot of the part this one bolts onto (`front_splitter` on `front_bumper`): it comes off
    /// and goes back on with that part (see the module docs).
    #[serde(default)]
    pub mount: Option<String>,
    /// Share of the assembly's damage this part takes as wear (see the module docs).
    #[serde(default)]
    pub damage_share: f32,
    /// Fraction of engine torque lost when the part is fully worn.
    #[serde(default)]
    pub torque_loss: f32,
    /// Most wear the part may have for its assembly to count as rebuilt (default 0.05).
    #[serde(default)]
    pub wear_limit: Option<f32>,
}

/// Default [`SlotDef::wear_limit`].
pub const DEFAULT_WEAR_LIMIT: f32 = 0.05;

impl SlotDef {
    /// Kind of part the slot takes: the set id, or the slot id.
    pub fn kind(&self) -> &str {
        self.set.as_deref().unwrap_or(&self.id)
    }

    /// Most wear the part may have for its assembly to count as rebuilt.
    pub fn wear_limit(&self) -> f32 {
        self.wear_limit.unwrap_or(DEFAULT_WEAR_LIMIT)
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
    #[serde(default)]
    mechanic: MechanicRates,
}

/// What the garage mechanic charges (`[mechanic]` in `parts.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MechanicRates {
    /// Once per job.
    pub call_out: u32,
    /// Per part taken off and put back (the part itself and everything in its way).
    pub per_part: u32,
}

impl Default for MechanicRates {
    fn default() -> Self {
        Self {
            call_out: 60,
            per_part: 30,
        }
    }
}

/// All slots, in file order (`data/economy/parts.toml`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PartsTable {
    /// Slots in display order.
    pub slots: Vec<SlotDef>,
    /// Mechanic labour rates.
    pub mechanic: MechanicRates,
}

impl PartsTable {
    /// Parse `parts.toml` (structure only; [`PartsTable::validate`] checks it).
    pub fn parse(text: &str) -> Result<Self, DataError> {
        let f: PartsFile = toml::from_str(text).map_err(|source| DataError::Toml {
            name: "economy/parts.toml".into(),
            source,
        })?;
        Ok(Self {
            slots: f.slot,
            mechanic: f.mechanic,
        })
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

    /// Slots inside assembly `parent`, in table order.
    pub fn children<'a>(&'a self, parent: &'a str) -> impl Iterator<Item = &'a SlotDef> + 'a {
        self.slots
            .iter()
            .filter(move |s| s.parent.as_deref() == Some(parent))
    }

    /// True when other slots live inside `id` (the engine).
    pub fn is_assembly(&self, id: &str) -> bool {
        self.children(id).next().is_some()
    }

    /// Slots bolted onto the part in `host`, in table order.
    pub fn mounted<'a>(&'a self, host: &'a str) -> impl Iterator<Item = &'a SlotDef> + 'a {
        self.slots
            .iter()
            .filter(move |s| s.mount.as_deref() == Some(host))
    }

    /// Slots whose parts leave the car with the part in `id`: an assembly's internals and the
    /// parts mounted on it.
    pub fn carried<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a SlotDef> + 'a {
        self.children(id).chain(self.mounted(id))
    }

    /// The assembly `slot` belongs to: its parent, itself if it is an assembly, else `None`.
    pub fn assembly_of<'a>(&'a self, slot: &'a SlotDef) -> Option<&'a SlotDef> {
        match &slot.parent {
            Some(p) => self.slot(p),
            None if self.is_assembly(&slot.id) => Some(slot),
            None => None,
        }
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
            for (name, v) in [
                ("damage_share", s.damage_share),
                ("torque_loss", s.torque_loss),
                ("wear_limit", s.wear_limit()),
            ] {
                if !(0.0..=2.0).contains(&v) || (name != "damage_share" && v > 1.0) {
                    return Err(invalid(&w, format!("{name} out of range")));
                }
            }
        }
        for s in &self.slots {
            let Some(p) = &s.parent else { continue };
            let w = format!("economy/parts.toml slot {}", s.id);
            let Some(parent) = self.slot(p) else {
                return Err(invalid(w, format!("parent {p:?} is not a slot")));
            };
            if parent.parent.is_some() || parent.set.is_some() {
                return Err(invalid(w, "the parent must be a plain top-level slot"));
            }
            if s.set.is_some() || s.optional || s.repair.is_some() {
                return Err(invalid(
                    w,
                    "a part inside an assembly has no set, repair item or optional flag",
                ));
            }
            if parent.blocked_by.contains(&s.id) {
                return Err(invalid(
                    w,
                    "the assembly cannot be blocked by its own parts (they come out with it)",
                ));
            }
        }
        for s in &self.slots {
            let Some(m) = &s.mount else { continue };
            let w = format!("economy/parts.toml slot {}", s.id);
            let Some(host) = self.slot(m) else {
                return Err(invalid(w, format!("mount {m:?} is not a slot")));
            };
            if host.parent.is_some() || host.mount.is_some() || host.set.is_some() {
                return Err(invalid(w, "the mount must be a plain top-level slot"));
            }
            if s.parent.is_some() || s.set.is_some() || s.repair.is_some() {
                return Err(invalid(
                    w,
                    "a mounted part has no parent, set or repair item",
                ));
            }
            if host.blocked_by.contains(&s.id) || s.blocked_by.contains(&host.id) {
                return Err(invalid(
                    w,
                    "a mounted part and its host do not block each other (they move together)",
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FittedPart {
    /// Mod id of an aftermarket part; `None` = the car's stock part.
    pub grade: Option<String>,
    /// Wear 0 (new) ..= 1 (destroyed); only assembly members (engine and internals) wear.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wear: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

impl FittedPart {
    /// The stock part.
    pub fn stock() -> Self {
        Self::grade(None)
    }

    /// A new part of `grade` (`None` = stock).
    pub fn grade(grade: Option<String>) -> Self {
        Self { grade, wear: 0.0 }
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
    /// Parts still inside an assembly (a loose engine's internals); each one's `kind` is its
    /// slot id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contents: Vec<LoosePart>,
}

impl LoosePart {
    /// The part inside this assembly in `slot`, if fitted.
    pub fn content(&self, slot: &str) -> Option<&LoosePart> {
        self.contents.iter().find(|c| c.kind == slot)
    }
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
    /// Assembly slot it lives inside (engine internals), if any.
    pub parent: Option<String>,
    /// Slot of the part it bolts onto (comes off and goes on with it), if any.
    #[serde(default)]
    pub mount: Option<String>,
    /// Fitted slots whose parts come off with this one (mounted parts; an assembly's internals
    /// are listed on the stand instead).
    #[serde(default)]
    pub takes_along: Vec<String>,
    /// Fitted part (`None` = empty).
    pub fitted: Option<PartView>,
    /// The car cannot leave while the slot is empty.
    pub required: bool,
    /// Blocking slots that still hold a part (empty = the slot can be worked on).
    pub blocked_by: Vec<String>,
    /// Why the fitted part cannot come off now (`None` = it can).
    pub remove_blocker: Option<String>,
    /// Fitted parts that have to come off first, all the way down (what "take off what's in
    /// the way" removes), in removal order.
    pub in_the_way: Vec<String>,
    /// What the mechanic charges to install one part here now.
    pub mechanic_fee: Money,
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
    /// Empty slots of loose assemblies on the stand that take it.
    pub stand_fits: Vec<StandFit>,
    /// Why it fits nowhere on this car (`None` when `fits` is not empty).
    pub reason: Option<String>,
    /// Dealer price for it now.
    pub sale_price: Money,
}

/// An empty slot of a loose assembly a part can go into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandFit {
    /// The assembly's inventory uid.
    pub assembly: u32,
    /// The slot inside it.
    pub slot: String,
}

/// A loose assembly (an engine) on the engine stand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StandView {
    /// The assembly's inventory uid.
    pub uid: u32,
    /// Display name.
    pub name: String,
    /// Assembly slot id (`engine`).
    pub slot: String,
    /// Its internals, in table order.
    pub slots: Vec<SlotView>,
    /// Names of internals it is missing.
    pub missing: Vec<String>,
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
    /// `None` = can be bought for this car, else why not (class, mode).
    pub reason: Option<String>,
    /// Name of a mod that must be fitted before this grade can go on (it can still be
    /// bought, or bought and fitted together with what it needs); `None` = nothing missing.
    pub needs: Option<String>,
    /// What the mechanic charges to fit what one purchase brings (every slot of a set).
    pub install_fee: Money,
    /// Names of installed mods that fitting this grade takes off because it does their job
    /// (coilovers replace separate dampers); refunded at resale value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replaces: Vec<String>,
}

/// Where the part the mechanic fits comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartSource {
    /// A part already in the inventory.
    Inventory(u32),
    /// Bought now: a mod id, or `None` for the stock part. An aftermarket set (wheels) is
    /// bought and fitted on every slot of the set.
    Buy(Option<String>),
}

/// Result of [`Economy::mechanic_install`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MechanicJob {
    /// Labour charged.
    pub fee: Money,
    /// Price of the new parts bought for the job.
    pub bought: Money,
    /// What the old parts sold for (0 when kept or there were none).
    pub sold: Money,
    /// Inventory uids of the old parts kept.
    pub kept: Vec<u32>,
    /// Parts taken off and put back.
    pub touched: u32,
    /// Repair items the damage system must restore on the stored lattice (new parts).
    pub restore: Vec<RepairItem>,
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
    /// Loose assemblies (engines) in the inventory, as the engine stand shows them.
    pub stands: Vec<StandView>,
    /// Engine torque left after wear (1 = none lost; see [`Economy::wear_torque_factor`]).
    pub torque_factor: f32,
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
    /// Replace every mod `car` carries that its class may not (an old save, or a lot variant
    /// sold before its data was fixed) with the allowed mod of the same line nearest in tier
    /// (the higher one on a tie), or drop it when the line has none; in the mod list and in
    /// the fitted parts alike. Returns `(old, new)` for each change, empty when the car was
    /// already legal.
    pub fn legalize_mods(&self, car: &mut crate::OwnedCar) -> Vec<(String, Option<String>)> {
        let class = car.class;
        let swap = |id: &str| -> Option<Option<String>> {
            let m = self.catalog.get(id)?;
            if m.allows_class(class) {
                return None;
            }
            let best = self
                .catalog
                .iter()
                .filter(|o| o.line == m.line && o.allows_class(class))
                .min_by_key(|o| ((o.tier as i32 - m.tier as i32).abs(), u8::MAX - o.tier))
                .map(|o| o.id.clone());
            Some(best)
        };
        let mut changes = Vec::new();
        let mut mods = Vec::new();
        for id in &car.mods {
            match swap(id) {
                None => mods.push(id.clone()),
                Some(new) => {
                    if let Some(n) = &new {
                        if !mods.contains(n) {
                            mods.push(n.clone());
                        }
                    }
                    changes.push((id.clone(), new));
                }
            }
        }
        car.mods = mods;
        if let Some(parts) = car.parts.as_mut() {
            for fitted in parts.slots.values_mut().flatten() {
                if let Some(g) = fitted.grade.clone() {
                    if let Some(new) = swap(&g) {
                        if !changes.iter().any(|(o, _)| *o == g) {
                            changes.push((g.clone(), new.clone()));
                        }
                        fitted.grade = new;
                    }
                }
            }
        }
        changes
    }

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
        // Parts inside an empty assembly are not on the car.
        for s in &self.parts.slots {
            if let Some(p) = &s.parent {
                if out.fitted(p).is_none() {
                    out.slots.insert(s.id.clone(), None);
                }
            }
        }
        // A car without a parts state has never had its assembly damage spread as wear.
        if car.parts.is_none() {
            for a in self
                .parts
                .slots
                .iter()
                .filter(|s| self.parts.is_assembly(&s.id))
            {
                let sev = self.line_severity(&car.damage.outstanding.lines, a);
                if sev > 0.0 {
                    self.add_wear(&mut out, a, sev);
                }
            }
        }
        out
    }

    /// Severity of `slot`'s outstanding repair line in `lines` (0 = none).
    fn line_severity(&self, lines: &[RepairLine], slot: &SlotDef) -> f32 {
        lines
            .iter()
            .filter(|l| slot.owns_line(&l.item))
            .map(|l| l.severity)
            .fold(0.0, f32::max)
            .clamp(0.0, 1.0)
    }

    /// Wear every fitted member of `assembly` by `rise × damage_share`.
    fn add_wear(&self, parts: &mut CarParts, assembly: &SlotDef, rise: f32) {
        let members = std::iter::once(assembly).chain(self.parts.children(&assembly.id));
        for m in members {
            if let Some(Some(f)) = parts.slots.get_mut(&m.id) {
                f.wear = (f.wear + rise * m.damage_share).clamp(0.0, 1.0);
            }
        }
    }

    /// The engine family stock internals of the assembly fitted in `assembly` belong to: the
    /// swap engine's mod id, or the car's model for the stock engine.
    fn family(&self, car: &OwnedCar, parts: &CarParts, assembly: &str) -> String {
        parts
            .fitted(assembly)
            .and_then(|f| f.grade.clone())
            .unwrap_or_else(|| car.def_id.clone())
    }

    /// Engine torque left after the wear of assembly members, `Π (1 − torque_loss × wear)`,
    /// at least 0.2. 1 while an assembly's repair line is open: the damage sim's component
    /// health already takes that power away.
    pub fn wear_torque_factor(&self, car: &OwnedCar) -> f32 {
        let parts = self.car_parts(car);
        let mut f = 1.0f32;
        for a in self
            .parts
            .slots
            .iter()
            .filter(|s| self.parts.is_assembly(&s.id))
        {
            if self.line_severity(&car.damage.outstanding.lines, a) > 0.0 {
                continue;
            }
            for m in std::iter::once(a).chain(self.parts.children(&a.id)) {
                if let Some(p) = parts.fitted(&m.id) {
                    f *= 1.0 - m.torque_loss * p.wear;
                }
            }
        }
        f.max(0.2)
    }

    fn default_part(&self, s: &SlotDef, mods: &[String]) -> Option<FittedPart> {
        let grade = mods.iter().find(|m| {
            self.catalog
                .get(m)
                .is_some_and(|d| s.lines.contains(&d.line))
        });
        match grade {
            Some(g) => Some(FittedPart::grade(Some(g.clone()))),
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
        if self.parts.assembly_of(slot).is_some() {
            let parts = self.car_parts(car);
            return parts.fitted(&slot.id).map_or(1.0, |f| 1.0 - f.wear);
        }
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
            .filter(|s| s.parent.as_ref().is_none_or(|p| parts.fitted(p).is_some()))
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
        if let Some(holder) = slot.parent.as_ref().or(slot.mount.as_ref()) {
            if parts.fitted(holder).is_none() {
                let name = self.parts.slot(holder).map_or(holder.as_str(), |s| &s.name);
                return Some(format!("no {} fitted", name.to_lowercase()));
            }
        }
        let blockers = self.blockers(parts, slot);
        if !blockers.is_empty() {
            return Some(self.blocker_reason(&blockers));
        }
        // What comes with the part goes back where it was: those slots must be free.
        let taken: Vec<String> = p
            .contents
            .iter()
            .filter(|q| parts.fitted(&q.kind).is_some())
            .map(|q| q.kind.clone())
            .collect();
        if !taken.is_empty() {
            return Some(self.blocker_reason(&taken));
        }
        if let Some(why) = self.underneath_reason(slot, |id| parts.fitted(id).is_some()) {
            return Some(why);
        }
        let family = match &slot.parent {
            Some(parent) => self.family(car, parts, parent),
            None => car.def_id.clone(),
        };
        std::iter::once(p)
            .chain(p.contents.iter())
            .find_map(|q| self.grade_blocker(q, slot, &family, car.class, mode))
    }

    /// Why loose part `p` cannot be fitted for its grade: a stock part of another model or
    /// engine family (`family`), or an aftermarket part the car's class or mode does not allow.
    /// Parts inside an assembly were checked when they went in; only their grade is checked.
    fn grade_blocker(
        &self,
        p: &LoosePart,
        slot: &SlotDef,
        family: &str,
        class: wp_contracts::VehicleClass,
        mode: DamageMode,
    ) -> Option<String> {
        match &p.grade {
            None => {
                let own = p.kind == slot.kind();
                if own && p.model.as_deref() != Some(family) {
                    return Some(if slot.parent.is_some() {
                        "a stock part of another engine".into()
                    } else {
                        "a stock part of another model".into()
                    });
                }
                if own && slot.optional {
                    return Some("no stock part goes here".into());
                }
            }
            Some(g) => {
                let Some(m) = self.catalog.get(g) else {
                    return Some(format!("unknown mod {g:?}"));
                };
                if !m.allows_class(class) {
                    return Some(format!("{} is not for class {class:?}", m.name));
                }
                if !m.allows_mode(mode) {
                    return Some(format!("{} is not available in {mode:?} mode", m.name));
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
                let in_the_way = self.in_the_way(&parts, s);
                SlotView {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    group: s.group,
                    kind: s.kind().to_string(),
                    parent: s.parent.clone(),
                    mount: s.mount.clone(),
                    takes_along: self
                        .parts
                        .mounted(&s.id)
                        .filter(|k| parts.fitted(&k.id).is_some())
                        .map(|k| k.id.clone())
                        .collect(),
                    fitted,
                    required: s.required,
                    blocked_by,
                    remove_blocker,
                    mechanic_fee: self.mechanic_fee(in_the_way.len()),
                    in_the_way,
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
                let stand_fits: Vec<StandFit> = acct
                    .inventory
                    .iter()
                    .filter(|a| a.uid != p.uid)
                    .filter(|a| self.stand_blocker(a, &p.kind, p).is_none())
                    .map(|a| StandFit {
                        assembly: a.uid,
                        slot: p.kind.clone(),
                    })
                    .collect();
                let reason = if fits.is_empty() && stand_fits.is_empty() {
                    reason
                } else {
                    None
                };
                InventoryView {
                    part: p.clone(),
                    name: self.loose_name(p),
                    fits,
                    stand_fits,
                    reason,
                    sale_price: self.part_sale_price(p),
                }
            })
            .collect();
        let stands = acct
            .inventory
            .iter()
            .filter(|p| self.parts.is_assembly(&p.kind))
            .map(|p| self.stand_view(p))
            .collect();
        Ok(WorkshopView {
            car,
            slots,
            missing: self.missing_parts(c),
            upgrades,
            inventory,
            stands,
            torque_factor: self.wear_torque_factor(c),
        })
    }

    /// Display name of a loose part: the mod's name, or "Stock <slot name>".
    fn loose_name(&self, p: &LoosePart) -> String {
        match p.grade.as_deref().and_then(|g| self.catalog.get(g)) {
            Some(m) => m.name.clone(),
            None => {
                let slot = self.parts.slots_of_kind(&p.kind).next();
                match slot {
                    Some(s) if s.set.is_none() => format!("Stock {}", s.name.to_lowercase()),
                    _ => "Stock".into(),
                }
            }
        }
    }

    /// Loose assembly `a` as the engine stand shows it.
    fn stand_view(&self, a: &LoosePart) -> StandView {
        let slots = self
            .parts
            .children(&a.kind)
            .map(|s| {
                let inner = a.content(&s.id);
                let fitted = inner.map(|q| {
                    let (name, tier) = self.part_name(q.grade.as_deref());
                    PartView {
                        grade: q.grade.clone(),
                        name,
                        tier,
                        condition: q.condition,
                    }
                });
                let blocked_by = self.stand_blockers(a, s);
                let remove_blocker = match inner {
                    None => Some("nothing fitted".into()),
                    Some(_) if !blocked_by.is_empty() => Some(self.blocker_reason(&blocked_by)),
                    Some(_) => None,
                };
                SlotView {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    group: s.group,
                    kind: s.kind().to_string(),
                    parent: s.parent.clone(),
                    mount: None,
                    takes_along: Vec::new(),
                    fitted,
                    required: s.required,
                    in_the_way: blocked_by.clone(),
                    blocked_by,
                    remove_blocker,
                    mechanic_fee: 0,
                }
            })
            .collect();
        StandView {
            uid: a.uid,
            name: self.loose_name(a),
            slot: a.kind.clone(),
            slots,
            missing: self
                .parts
                .children(&a.kind)
                .filter(|s| s.required && a.content(&s.id).is_none())
                .map(|s| s.name.clone())
                .collect(),
        }
    }

    /// Inside an assembly a part goes on top of the required parts its removal frees (the head
    /// on the head gasket): why `slot` cannot be fitted while one of them is missing.
    fn underneath_reason(&self, slot: &SlotDef, present: impl Fn(&str) -> bool) -> Option<String> {
        let parent = slot.parent.as_deref()?;
        let names: Vec<&str> = self
            .parts
            .children(parent)
            .filter(|k| k.required && k.blocked_by.contains(&slot.id) && !present(&k.id))
            .map(|k| k.name.as_str())
            .collect();
        (!names.is_empty()).then(|| format!("fit first: {}", names.join(", ")))
    }

    /// Siblings in `slot`'s `blocked_by` list still inside loose assembly `a` (slots outside
    /// the assembly, like the intake, are not on the stand).
    fn stand_blockers(&self, a: &LoosePart, slot: &SlotDef) -> Vec<String> {
        slot.blocked_by
            .iter()
            .filter(|b| a.content(b).is_some())
            .cloned()
            .collect()
    }

    /// Why loose part `p` cannot go into `slot` of loose assembly `a` (`None` = it can).
    fn stand_blocker(&self, a: &LoosePart, slot: &str, p: &LoosePart) -> Option<String> {
        let Some(s) = self.parts.slot(slot) else {
            return Some("no such slot".into());
        };
        if s.parent.as_deref() != Some(a.kind.as_str()) {
            return Some("does not go inside this part".into());
        }
        if p.kind != s.id {
            return Some("wrong kind of part".into());
        }
        if a.content(slot).is_some() {
            return Some("the slot is not empty".into());
        }
        let blockers = self.stand_blockers(a, s);
        if !blockers.is_empty() {
            return Some(self.blocker_reason(&blockers));
        }
        if let Some(why) = self.underneath_reason(s, |id| a.content(id).is_some()) {
            return Some(why);
        }
        if p.grade.is_none() {
            let family = a.grade.as_ref().or(a.model.as_ref());
            if p.model.as_ref() != family {
                return Some("a stock part of another engine".into());
            }
        }
        None
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
        for k in self.parts.carried(&slot.id) {
            next.slots.insert(k.id.clone(), None);
        }
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
        self.remove_part_with(acct, car, slot, true)
    }

    /// [`Economy::remove_part`]; `check_reqs` false skips the mod-requirement check (a
    /// mechanic swap takes a part off to fit its replacement: only the finished car is
    /// checked, so a Race ECU that needs an intake does not stop the intake being upgraded).
    fn remove_part_with(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        slot: &str,
        check_reqs: bool,
    ) -> EconResult<u32> {
        let c = self.workshop_car(acct, car)?;
        let s = self.try_slot(slot)?;
        let parts = self.car_parts(c);
        if check_reqs {
            self.check_remove(c, &parts, s)?;
        } else if parts.fitted(&s.id).is_none() {
            return Err(refuse(&s.id, "nothing fitted"));
        } else {
            let blockers = self.blockers(&parts, s);
            if !blockers.is_empty() {
                return Err(refuse(&s.id, self.blocker_reason(&blockers)));
            }
        }
        let fitted = parts.fitted(slot).cloned().expect("checked");
        let condition = self.slot_condition(c, s);
        let value = self.new_part_value(c, s, fitted.grade.as_deref());
        let model = match &s.parent {
            Some(parent) => self.family(c, &parts, parent),
            None => c.def_id.clone(),
        };
        // An assembly takes its fitted internals with it (stock ones of its engine family, worn
        // as they were); a host takes the parts mounted on it (stock ones of the car's model).
        let family = fitted.grade.clone().unwrap_or_else(|| c.def_id.clone());
        let inner: Vec<(String, FittedPart, Money, String, f32)> = self
            .parts
            .carried(slot)
            .filter_map(|k| {
                let f = parts.fitted(&k.id)?.clone();
                let v = self.new_part_value(c, k, f.grade.as_deref());
                let (model, condition) = if k.parent.is_some() {
                    (family.clone(), 1.0 - f.wear)
                } else {
                    (c.def_id.clone(), self.slot_condition(c, k))
                };
                Some((k.id.clone(), f, v, model, condition))
            })
            .collect();
        transact(acct, |tx| {
            let uid = tx.acct.next_part_uid.max(1);
            tx.acct.next_part_uid = uid + 1;
            let mut contents = Vec::new();
            for (kind, f, value, model, condition) in &inner {
                let cuid = tx.acct.next_part_uid;
                tx.acct.next_part_uid += 1;
                contents.push(LoosePart {
                    uid: cuid,
                    kind: kind.clone(),
                    model: f.grade.is_none().then(|| model.clone()),
                    grade: f.grade.clone(),
                    condition: *condition,
                    damage: Vec::new(),
                    value: *value,
                    contents: Vec::new(),
                });
            }
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
            for (kind, ..) in &inner {
                next.slots.insert(kind.clone(), None);
            }
            c.mods = self.derive_mods(&c.mods, &next);
            c.parts = Some(next);
            tx.acct.inventory.push(LoosePart {
                uid,
                kind: s.kind().to_string(),
                model: fitted.grade.is_none().then_some(model),
                grade: fitted.grade,
                condition,
                damage,
                value,
                contents,
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
        let wear = |q: &LoosePart| (1.0 - q.condition).clamp(0.0, 1.0);
        let mut next = parts.clone();
        let mut fitted = FittedPart::grade(p.grade.clone());
        let member = self.parts.assembly_of(s).is_some();
        if member {
            fitted.wear = wear(&p);
        }
        next.slots.insert(slot.to_string(), Some(fitted));
        for q in &p.contents {
            let mut f = FittedPart::grade(q.grade.clone());
            if member {
                f.wear = wear(q);
            }
            next.slots.insert(q.kind.clone(), Some(f));
        }
        let mut next_mods = self.derive_mods(&c.mods, &next);
        // Completing a set that supersedes a fitted mod (coilovers on all four corners take
        // the place of separate dampers) takes that mod off, refunded at resale value.
        let superseded = self.catalog.superseded_in(&next_mods);
        next_mods.retain(|m| !superseded.contains(m));
        let mut refunds = Vec::new();
        for id in &superseded {
            refunds.push((id.clone(), self.mod_refund(id)?));
        }
        self.check_mods(c, slot, &c.mods, &next_mods)?;
        // A part inside an assembly may finish a rebuild: every member within its limit.
        let rebuilt = match &s.parent {
            Some(parent) => self.parts.slot(parent).filter(|a| {
                self.line_severity(&c.damage.outstanding.lines, a) > 0.0
                    && self.rebuilt(a, |id| next.fitted(id).map(|f| f.wear))
            }),
            None => None,
        };
        transact(acct, |tx| {
            tx.acct.inventory.retain(|x| x.uid != part);
            let c = tx.car_mut(car)?;
            // An assembly with its damage line gone (rebuilt on the stand) is fresh too.
            let fresh = p.damage.is_empty() && (member || p.condition >= 1.0);
            let mut restore = Vec::new();
            if fresh {
                c.damage.outstanding.lines.retain(|l| !s.owns_line(&l.item));
                restore.extend(s.repair);
            } else {
                c.damage.outstanding.lines.extend(p.damage.iter().cloned());
            }
            if let Some(a) = rebuilt {
                c.damage.outstanding.lines.retain(|l| !a.owns_line(&l.item));
                restore.extend(a.repair);
            }
            c.damage.outstanding.total = c.damage.outstanding.lines.iter().map(|l| l.total).sum();
            if let Some(corner) = s.puncture {
                c.damage.handling.tire_puncture[corner as usize] =
                    (1.0 - p.condition).clamp(0.0, 1.0);
            }
            c.mods = next_mods;
            c.parts = Some(next);
            for (mod_id, refund) in refunds {
                tx.credit(refund, EventReason::ModRemoved { mod_id }, Some(car));
            }
            Ok(PartFit { restore })
        })
    }

    /// True when assembly `a` has every required part and every member is within its wear
    /// limit; `wear(slot)` is the wear of the part in a slot (`None` = empty).
    fn rebuilt(&self, a: &SlotDef, wear: impl Fn(&str) -> Option<f32>) -> bool {
        std::iter::once(a)
            .chain(self.parts.children(&a.id))
            .all(|m| match wear(&m.id) {
                Some(w) => w <= m.wear_limit(),
                None => !m.required,
            })
    }

    /// Fitted parts that must come off before `slot` can be worked on, all the way down, in
    /// removal order (the bumper before the radiator guard before the radiator).
    pub fn in_the_way(&self, parts: &CarParts, slot: &SlotDef) -> Vec<String> {
        fn walk(e: &Economy, parts: &CarParts, slot: &SlotDef, out: &mut Vec<String>) {
            for b in &slot.blocked_by {
                if parts.fitted(b).is_none() || out.contains(b) {
                    continue;
                }
                if let Some(bs) = e.parts.slot(b) {
                    walk(e, parts, bs, out);
                }
                if !out.contains(b) {
                    out.push(b.clone());
                }
            }
        }
        let mut out = Vec::new();
        walk(self, parts, slot, &mut out);
        out
    }

    /// Mechanic's labour for one part with `in_the_way` parts to take off and put back.
    pub fn mechanic_fee(&self, in_the_way: usize) -> Money {
        let r = self.parts.mechanic;
        (r.call_out + r.per_part * (in_the_way as u32 + 1)) as Money
    }

    /// Do it yourself: take off everything in the way of `slot` (not `slot` itself) into the
    /// inventory, in removal order. Returns the removed parts' uids. No cash changes.
    pub fn clear_way(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        slot: &str,
    ) -> EconResult<Vec<u32>> {
        let c = self.workshop_car(acct, car)?;
        let s = self.try_slot(slot)?;
        let way = self.in_the_way(&self.car_parts(c), s);
        let mut a = acct.clone();
        let mut events = Vec::new();
        let mut uids = Vec::new();
        for b in &way {
            let (uid, ev) = self.remove_part(&mut a, car, b)?;
            events.extend(ev);
            uids.push(uid);
        }
        *acct = a;
        Ok((uids, events))
    }

    /// Pay the mechanic to fit a part in `slot` of `car`: take off what is in the way, swap
    /// the part, put everything back. The new part comes from the inventory or is bought now
    /// (an aftermarket set is bought and fitted on every slot of the set). The old parts are
    /// sold to the dealer (`sell_old`) or kept in the inventory. All or nothing: the job is
    /// refused if the cash does not cover parts and labour.
    pub fn mechanic_install(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        slot: &str,
        source: &PartSource,
        sell_old: bool,
        mode: DamageMode,
    ) -> EconResult<MechanicJob> {
        let mods_before = self.workshop_car(acct, car)?.mods.clone();
        let s = self.try_slot(slot)?;
        let mut a = acct.clone();
        let mut events = Vec::new();
        let mut job = MechanicJob::default();
        let cash_before = a.cash;
        // The new parts and the slots they go in.
        let (news, targets): (Vec<u32>, Vec<String>) = match source {
            PartSource::Inventory(uid) => (vec![*uid], vec![slot.to_string()]),
            PartSource::Buy(grade) => {
                let (uids, ev) = self.buy_part(&mut a, car, s.kind(), grade.as_deref(), mode)?;
                events.extend(ev);
                let targets = if uids.len() > 1 {
                    self.parts
                        .slots_of_kind(s.kind())
                        .map(|x| x.id.clone())
                        .collect()
                } else {
                    vec![slot.to_string()]
                };
                (uids, targets)
            }
        };
        job.bought = cash_before - a.cash;
        let mut olds = Vec::new();
        for (t, new) in targets.iter().zip(&news) {
            let ts = self.try_slot(t)?;
            let way = self.in_the_way(&self.car_parts(a.try_car(car)?), ts);
            let mut off = Vec::new();
            for b in &way {
                let (uid, ev) = self.remove_part_with(&mut a, car, b, false)?;
                events.extend(ev);
                off.push((b.clone(), uid));
            }
            if self.car_parts(a.try_car(car)?).fitted(t).is_some() {
                let (uid, ev) = self.remove_part_with(&mut a, car, t, false)?;
                events.extend(ev);
                olds.push(uid);
            }
            let (fit, ev) = self.fit_part(&mut a, car, t, *new, mode)?;
            events.extend(ev);
            job.restore.extend(fit.restore);
            for (b, uid) in off.iter().rev() {
                let (fit, ev) = self.fit_part(&mut a, car, b, *uid, mode)?;
                events.extend(ev);
                job.restore.extend(fit.restore);
            }
            job.touched += way.len() as u32 + 1;
        }
        // The swap is checked as a whole: the finished car must keep every requirement it met.
        {
            let done = a.try_car(car)?;
            self.check_mods(done, slot, &mods_before, &done.mods)?;
        }
        let r = self.parts.mechanic;
        job.fee = (r.call_out + r.per_part * job.touched) as Money;
        let ((), ev) = transact(&mut a, |tx| {
            tx.spend(
                job.fee,
                EventReason::RepairPaid {
                    lines: job.touched,
                    labour_multiplier: 1.0,
                },
                Some(car),
            )
        })?;
        events.extend(ev);
        for uid in olds {
            if sell_old {
                let (price, ev) = self.sell_part(&mut a, uid)?;
                events.extend(ev);
                job.sold += price;
            } else {
                job.kept.push(uid);
            }
        }
        *acct = a;
        Ok((job, events))
    }

    /// Engine stand: take the part in `slot` out of loose assembly `assembly` into the
    /// inventory, in removal order. Returns the part's uid. No cash changes.
    pub fn strip_part(
        &self,
        acct: &mut PlayerAccount,
        assembly: u32,
        slot: &str,
    ) -> EconResult<u32> {
        let a = acct
            .inventory
            .iter()
            .find(|p| p.uid == assembly)
            .ok_or(EconomyError::UnknownPart(assembly))?;
        let s = self.try_slot(slot)?;
        if s.parent.as_deref() != Some(a.kind.as_str()) {
            return Err(refuse(slot, "does not go inside this part"));
        }
        let Some(inner) = a.content(slot).cloned() else {
            return Err(refuse(slot, "nothing fitted"));
        };
        let blockers = self.stand_blockers(a, s);
        if !blockers.is_empty() {
            return Err(refuse(slot, self.blocker_reason(&blockers)));
        }
        transact(acct, |tx| {
            let a = tx
                .acct
                .inventory
                .iter_mut()
                .find(|p| p.uid == assembly)
                .expect("checked");
            a.contents.retain(|c| c.kind != slot);
            tx.acct.inventory.push(inner.clone());
            Ok(inner.uid)
        })
    }

    /// Engine stand: put inventory part `part` into the empty `slot` of loose assembly
    /// `assembly`. When that completes a rebuild (every member within its wear limit), the
    /// assembly's damage lines are cleared: fitted to a car, it restores its lattice item.
    pub fn build_part(
        &self,
        acct: &mut PlayerAccount,
        assembly: u32,
        slot: &str,
        part: u32,
    ) -> EconResult<()> {
        let a = acct
            .inventory
            .iter()
            .find(|p| p.uid == assembly)
            .ok_or(EconomyError::UnknownPart(assembly))?;
        let p = acct
            .inventory
            .iter()
            .find(|p| p.uid == part && p.uid != assembly)
            .ok_or(EconomyError::UnknownPart(part))?
            .clone();
        if let Some(why) = self.stand_blocker(a, slot, &p) {
            return Err(refuse(slot, why));
        }
        let asm = self.try_slot(&a.kind)?;
        let mut next = a.clone();
        next.contents.push(p.clone());
        let order: Vec<&str> = self
            .parts
            .children(&a.kind)
            .map(|s| s.id.as_str())
            .collect();
        next.contents
            .sort_by_key(|c| order.iter().position(|k| *k == c.kind));
        let rebuilt = !next.damage.is_empty()
            && self.rebuilt(asm, |id| {
                if id == asm.id {
                    Some(1.0 - next.condition)
                } else {
                    next.content(id).map(|q| 1.0 - q.condition)
                }
            });
        if rebuilt {
            next.damage.clear();
        }
        transact(acct, |tx| {
            tx.acct.inventory.retain(|x| x.uid != part);
            let a = tx
                .acct
                .inventory
                .iter_mut()
                .find(|x| x.uid == assembly)
                .expect("checked");
            *a = next.clone();
            Ok(())
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
        let parts = self.car_parts(c);
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
            let one_fee = self.mechanic_fee(self.in_the_way(&parts, s).len());
            let set_fee = {
                let r = self.parts.mechanic;
                let n: usize = self
                    .parts
                    .slots_of_kind(s.kind())
                    .map(|x| self.in_the_way(&parts, x).len() + 1)
                    .sum();
                (r.call_out + r.per_part * n as u32) as Money
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
                    needs: None,
                    install_fee: one_fee,
                    replaces: Vec::new(),
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
                // It can be bought, but fitted only after this (the swan-neck wing before a
                // splitter). Not a `reason`: the workshop offers both together.
                let needs = self
                    .catalog
                    .missing_requirement(&c.mods, &m.id)
                    .map(|r| r.name.clone());
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
                    needs,
                    install_fee: if count > 1 { set_fee } else { one_fee },
                    replaces: c
                        .mods
                        .iter()
                        .filter(|i| m.supersedes.contains(i))
                        .filter_map(|i| self.catalog.get(i))
                        .map(|o| o.name.clone())
                        .collect(),
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
        // Stock internals belong to the engine in the car; a new engine comes complete with
        // stock internals of its own family.
        let parts = self.car_parts(c);
        let model = grade.is_none().then(|| match &s.parent {
            Some(parent) => self.family(c, &parts, parent),
            None => c.def_id.clone(),
        });
        let family = grade.map_or_else(|| c.def_id.clone(), str::to_string);
        let inner: Vec<(String, Money)> = self
            .parts
            .children(kind)
            .filter(|k| !k.optional)
            .map(|k| (k.id.clone(), self.new_part_value(c, k, None)))
            .collect();
        transact(acct, |tx| {
            tx.spend(price, reason, Some(car))?;
            let mut uids = Vec::new();
            for _ in 0..count {
                let uid = tx.acct.next_part_uid.max(1);
                tx.acct.next_part_uid = uid + 1;
                let mut contents = Vec::new();
                for (k, v) in &inner {
                    let cuid = tx.acct.next_part_uid;
                    tx.acct.next_part_uid += 1;
                    contents.push(LoosePart {
                        uid: cuid,
                        kind: k.clone(),
                        grade: None,
                        model: Some(family.clone()),
                        condition: 1.0,
                        damage: Vec::new(),
                        value: *v,
                        contents: Vec::new(),
                    });
                }
                tx.acct.inventory.push(LoosePart {
                    uid,
                    kind: kind.to_string(),
                    grade: grade.map(str::to_string),
                    model: model.clone(),
                    condition: 1.0,
                    damage: Vec::new(),
                    value,
                    contents,
                });
                uids.push(uid);
            }
            Ok(uids)
        })
    }

    /// What the dealer pays for a loose part: `mod_resale_frac × value × condition`, plus the
    /// parts inside it.
    pub fn part_sale_price(&self, p: &LoosePart) -> Money {
        let own = round(self.market.mod_resale_frac * p.value as f32 * p.condition.clamp(0.0, 1.0));
        own + p
            .contents
            .iter()
            .map(|c| self.part_sale_price(c))
            .sum::<Money>()
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
            if s.parent.as_ref().is_some_and(|p| next.fitted(p).is_none()) {
                continue;
            }
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

    /// Spread the rise of each fitted assembly's repair-line severity (`before` → the car's
    /// new estimate) as wear over its members (see the module docs).
    pub(crate) fn wear_from_damage(&self, car: &mut OwnedCar, before: &[RepairLine]) {
        // A car without a parts state derives its wear from its damage: from the old damage
        // here, or the rise would count twice.
        let mut parts = if car.parts.is_none() {
            let mut old = car.clone();
            old.damage.outstanding.lines = before.to_vec();
            self.car_parts(&old)
        } else {
            self.car_parts(car)
        };
        let mut changed = false;
        for a in self
            .parts
            .slots
            .iter()
            .filter(|s| self.parts.is_assembly(&s.id))
        {
            if parts.fitted(&a.id).is_none() {
                continue;
            }
            let rise = self.line_severity(&car.damage.outstanding.lines, a)
                - self.line_severity(before, a);
            if rise > 0.0 {
                self.add_wear(&mut parts, a, rise);
                changed = true;
            }
        }
        if changed {
            car.parts = Some(parts);
        }
    }

    /// A shop repaired `items`: an assembly whose line was repaired is rebuilt (no wear).
    pub(crate) fn rebuild_repaired(&self, car: &mut OwnedCar, items: &[RepairItem]) {
        let Some(parts) = car.parts.as_mut() else {
            return;
        };
        for a in self
            .parts
            .slots
            .iter()
            .filter(|s| self.parts.is_assembly(&s.id))
        {
            if !items.iter().any(|i| a.owns_line(i)) {
                continue;
            }
            for m in std::iter::once(a).chain(self.parts.children(&a.id)) {
                if let Some(Some(f)) = parts.slots.get_mut(&m.id) {
                    f.wear = 0.0;
                }
            }
        }
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
