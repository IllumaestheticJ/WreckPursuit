//! Car lots in the browser career: the lot list and stock (`career_lot_json`), buying a lot
//! car (`career_buy_listing`), tyres (`career_replace_tires`) and **visiting a lot**
//! (`career_visit_lot`): a session on the lot itself, with today's stock parked in rows (dented,
//! missing panels and flat tyres included), the player's own car parked at the gate, and the
//! lot's fencing and fixtures. The testbed browses the parked cars with the camera and buys
//! from the listing panel; [`lot_visit_json`] maps each parked entity to its parking space.
//!
//! Layout (flat ground, +Y into the lot): rows of [`ROW_LEN`] spaces [`SPACE_M`] apart,
//! [`ROW_GAP_M`] between rows, every car facing the gate; the gate is at y = 0 and the
//! player's car waits just inside it.

use std::cell::RefCell;
use std::f32::consts::PI;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wp_contracts::{EntityId, VehicleInput};
use wp_economy::{CarUid, LotKind};

use crate::career::{end_active_run, with_career, Career, Overlay};
use crate::scenario::{entity, free_drive_in};
use crate::session::Session;

/// Spaces per row.
pub const ROW_LEN: u32 = 5;
/// Distance between spaces in a row, m.
pub const SPACE_M: f32 = 4.8;
/// Distance between rows, m.
pub const ROW_GAP_M: f32 = 11.0;
/// First row's distance from the gate, m.
pub const FIRST_ROW_M: f32 = 16.0;

/// Scenario name prefix of a lot visit (`lot_<lot id>`).
pub const VISIT_PREFIX: &str = "lot_";

/// One parked car of a visit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VisitCar {
    /// Sim entity.
    pub entity: u32,
    /// Parking space (the listing's slot).
    pub slot: u32,
}

/// The lot being visited (`lot_visit_json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LotVisit {
    /// Lot id.
    pub lot: String,
    /// The player's own car parked at the gate, if they came in one.
    pub own_car: Option<u32>,
    /// The cars for sale, by entity.
    pub cars: Vec<VisitCar>,
}

thread_local! {
    static VISIT: RefCell<Option<LotVisit>> = const { RefCell::new(None) };
}

/// Where space `slot` is: `(pos, yaw)`; cars face the gate (-Y).
pub fn space_pose(slot: u32) -> ([f32; 3], f32) {
    let row = slot / ROW_LEN;
    let col = slot % ROW_LEN;
    let x = (col as f32 - (ROW_LEN - 1) as f32 * 0.5) * SPACE_M;
    let y = FIRST_ROW_M + row as f32 * ROW_GAP_M;
    ([x, y, 0.0], PI)
}

/// Fences, kerbs and fixtures of a lot with `slots` spaces (static boxes).
pub fn lot_fixtures(kind: LotKind, slots: u32) -> Vec<simcli::scenario::Barrier> {
    use simcli::scenario::Barrier;
    let rows = slots.div_ceil(ROW_LEN).max(1);
    let half_w = (ROW_LEN as f32 * 0.5) * SPACE_M + 5.0;
    let back = FIRST_ROW_M + (rows - 1) as f32 * ROW_GAP_M + 9.0;
    let front = -12.0;
    let len = back - front;
    let mid_y = (back + front) * 0.5;
    let wall_h = if kind == LotKind::Junkyard { 2.4 } else { 0.5 };
    let b = |pos: [f32; 3], size: [f32; 3]| Barrier {
        pos,
        size,
        yaw: 0.0,
    };
    let mut out = vec![
        // Side and back fences (the gate side stays open).
        b([-half_w, mid_y, wall_h * 0.5], [0.3, len, wall_h]),
        b([half_w, mid_y, wall_h * 0.5], [0.3, len, wall_h]),
        b([0.0, back, wall_h * 0.5], [2.0 * half_w, 0.3, wall_h]),
        // Gate posts.
        b([-6.0, 0.0, 1.5], [0.5, 0.5, 3.0]),
        b([6.0, 0.0, 1.5], [0.5, 0.5, 3.0]),
    ];
    match kind {
        LotKind::Dealership => {
            // Sign over the gate on two tall pylons, and a showroom block at the back.
            out.push(b([-6.0, 0.0, 3.5], [0.5, 0.5, 2.0]));
            out.push(b([6.0, 0.0, 3.5], [0.5, 0.5, 2.0]));
            out.push(b([0.0, 0.0, 4.9], [12.5, 0.4, 1.4]));
            out.push(b([0.0, back + 5.0, 3.0], [2.0 * half_w, 9.0, 6.0]));
        }
        LotKind::UsedLot => {
            // Bunting poles down both sides and a sales hut by the gate.
            let mut y = 4.0;
            while y < back {
                out.push(b([-half_w + 1.0, y, 2.0], [0.15, 0.15, 4.0]));
                out.push(b([half_w - 1.0, y, 2.0], [0.15, 0.15, 4.0]));
                y += 8.0;
            }
            out.push(b([half_w - 4.0, 4.0, 1.4], [4.0, 3.0, 2.8]));
        }
        LotKind::Junkyard => {
            // Stacks of crushed cars along the fences and a crane-ish gantry at the back.
            let mut y = 3.0;
            let mut i = 0;
            while y < back - 3.0 {
                let h = 1 + i % 3;
                for k in 0..h {
                    let z = 0.45 + k as f32 * 0.9;
                    out.push(b([-half_w + 2.4, y, z], [4.0, 1.8, 0.85]));
                    out.push(b([half_w - 2.4, y + 1.0, z], [4.0, 1.8, 0.85]));
                }
                y += 4.5;
                i += 1;
            }
            out.push(b([-8.0, back - 2.0, 4.0], [0.6, 0.6, 8.0]));
            out.push(b([8.0, back - 2.0, 4.0], [0.6, 0.6, 8.0]));
            out.push(b([0.0, back - 2.0, 8.3], [16.6, 0.6, 0.6]));
        }
    }
    out
}

impl Career {
    /// Build the visit session for `lot_id`: today's unsold stock parked in its spaces with
    /// its wear and tyres, the active car (if usable) at the gate.
    pub fn visit_lot(&mut self, lot_id: &str) -> Result<(Session, LotVisit), String> {
        if self.run_active() {
            return Err("finish the run first (back to the garage)".into());
        }
        let lot = self
            .economy()
            .lot(lot_id)
            .cloned()
            .ok_or_else(|| format!("no lot {lot_id:?}"))?;
        let stock: Vec<_> = self
            .lot_stock(lot_id)?
            .into_iter()
            .filter(|(_, sold)| !sold)
            .map(|(l, _)| l)
            .collect();
        let acct = &self.profile().account;
        let own = acct
            .active_car
            .and_then(|u| acct.car(u))
            .filter(|c| c.is_usable())
            .cloned();
        // The entities are replaced below; free drive supplies the rest of the scenario.
        let mut sc = free_drive_in(
            "junkyard_beater",
            self.profile().mode,
            simcli::scenario::CitySpec::Flat,
        );
        sc.name = format!("{VISIT_PREFIX}{lot_id}");
        sc.time_of_day_h = if lot.kind == LotKind::Junkyard {
            17.0
        } else {
            12.5
        };
        sc.time_ratio = 1.0;
        sc.traffic_cars = false;
        sc.parked_cars = false;
        sc.police_cars = false;
        sc.barriers = lot_fixtures(lot.kind, lot.slots);
        sc.entities.clear();
        // Every car carries the colour it is drawn in: the player's own (the same as in the
        // garage and on the road) and each listing's factory paint.
        if let Some(c) = &own {
            let mut e = entity("player", &c.def_id, [0.0, 4.0, 0.0], 0.0);
            e.paint = Some(c.body_paint());
            sc.entities.push(e);
        }
        for l in &stock {
            let (pos, yaw) = space_pose(l.slot);
            let mut e = entity(&format!("space_{}", l.slot), &l.def_id, pos, yaw);
            e.paint = Some(l.paint);
            sc.entities.push(e);
        }
        if sc.entities.is_empty() {
            // An empty lot still needs one entity for the session; park a beater by the gate.
            sc.entities
                .push(entity("player", "junkyard_beater", [0.0, 4.0, 0.0], 0.0));
        }
        let files = match &own {
            Some(c) => self.car_files(c)?,
            None => Default::default(),
        };
        let mut session = Session::new_with_data(sc, false, &Overlay { files: &files })?;
        let ids: Vec<EntityId> = session.entities().to_vec();
        let offset = usize::from(own.is_some() || stock.is_empty());
        {
            let sim = session.sim_before_start_mut()?;
            if let (Some(c), Some(&id)) = (&own, ids.first()) {
                if let Some(snap) = self.lattice(c.uid)? {
                    let _ = sim
                        .damage_mut(id)
                        .map_err(|e| e.to_string())?
                        .restore_full(&snap);
                }
                if let Some((grip, flat)) = self.tire_effects(c.uid) {
                    sim.set_tire_condition(id, grip, flat)
                        .map_err(|e| e.to_string())?;
                }
            }
            for (l, &id) in stock.iter().zip(&ids[offset..]) {
                if let Some(snap) = self.listing_damage(l)? {
                    let _ = sim
                        .damage_mut(id)
                        .map_err(|e| e.to_string())?
                        .restore_full(&snap);
                }
                let (grip, flat) = self.economy().tire_effects(&l.tires);
                sim.set_tire_condition(id, grip, flat)
                    .map_err(|e| e.to_string())?;
            }
        }
        // Everything on the lot sits on its handbrake.
        let parked = VehicleInput {
            handbrake: 1.0,
            ..VehicleInput::default()
        };
        for &id in &ids {
            session.set_input(id, parked)?;
        }
        let visit = LotVisit {
            lot: lot_id.to_string(),
            own_car: own.as_ref().and(ids.first()).map(|e| e.0),
            cars: stock
                .iter()
                .zip(&ids[offset..])
                .map(|(l, id)| VisitCar {
                    entity: id.0,
                    slot: l.slot,
                })
                .collect(),
        };
        Ok((session, visit))
    }
}

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serializable")
}

/// Today's stock of lot `lot_id` (`career::LotStockView` JSON).
#[wasm_bindgen]
pub fn career_lot_json(lot_id: &str) -> Result<String, JsValue> {
    with_career(|c| c.lot_view(lot_id).map(|v| json(&v)))
}

/// Buy the car in space `slot` of `lot_id`. Returns the garage view JSON.
#[wasm_bindgen]
pub fn career_buy_listing(lot_id: &str, slot: u32) -> Result<String, JsValue> {
    with_career(|c| {
        c.buy_listing(lot_id, slot)?;
        Ok(json(&c.view()))
    })
}

/// Replace tyres on car `uid`: `wheels_json` = `[fl, fr, rl, rr]` booleans. Returns the
/// garage view JSON.
#[wasm_bindgen]
pub fn career_replace_tires(uid: u32, wheels_json: &str) -> Result<String, JsValue> {
    let wheels: [bool; 4] = serde_json::from_str(wheels_json).map_err(crate::err)?;
    with_career(|c| {
        c.replace_tires(CarUid(uid), wheels)?;
        Ok(json(&c.view()))
    })
}

/// Visit lot `lot_id`: ends a career run in progress (back to the garage first), then
/// installs a session on the lot. Returns the entity ids like `init`; [`lot_visit_json`]
/// says which is which.
#[wasm_bindgen]
pub fn career_visit_lot(lot_id: &str) -> Result<Vec<u32>, JsValue> {
    end_active_run()?;
    let (session, visit) = with_career(|c| c.visit_lot(lot_id))?;
    let ids = crate::install(session);
    VISIT.with(|v| *v.borrow_mut() = Some(visit));
    Ok(ids)
}

/// The lot being visited ([`LotVisit`] JSON), or `null` when the current session is not a
/// lot visit.
#[wasm_bindgen]
pub fn lot_visit_json() -> Result<String, JsValue> {
    let name = crate::with_session_opt(|s| Ok::<_, String>(s.map(|s| s.scenario().name.clone())))
        .map_err(crate::err)?;
    let visit = VISIT.with(|v| v.borrow().clone());
    let current = visit.filter(|v| name.as_deref() == Some(&format!("{VISIT_PREFIX}{}", v.lot)));
    Ok(json(&current))
}
