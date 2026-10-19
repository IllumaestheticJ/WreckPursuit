//! Road water per grid cell: rain accumulation, runoff, infiltration, evaporation (by surface,
//! temperature, wind, sun, humidity and traffic) and puddle-prone cells.
//!
//! Cells are square, axis-aligned in world XY, row-major with X fastest. Points outside the
//! grid read a single "background" cell (asphalt, never a puddle) so queries never fail.

use wp_contracts::{SurfaceType, WorldQuery};
use wp_math::{Rng, StateHasher};

use crate::data::{SurfaceTable, ALL_SURFACES};

/// Grid placement and resolution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridSpec {
    /// World XY of the grid's minimum corner, m.
    pub origin: [f32; 2],
    /// Cell edge length, m.
    pub cell_m: f32,
    /// Cells along X.
    pub nx: u32,
    /// Cells along Y.
    pub ny: u32,
}

impl GridSpec {
    /// Smallest grid of `cell_m` cells covering the XY box `min..max`.
    pub fn covering(min: [f32; 2], max: [f32; 2], cell_m: f32) -> Self {
        let cell_m = cell_m.max(0.5);
        let n = |a: f32, b: f32| (((b - a) / cell_m).ceil() as u32).max(1);
        Self {
            origin: min,
            cell_m,
            nx: n(min[0], max[0]),
            ny: n(min[1], max[1]),
        }
    }

    /// Grid covering the XY bounding box of `points` plus `margin_m` on every side (e.g. the
    /// road network's node positions). With no points: a 2 km square around the origin, which
    /// covers the flat test world's driving area.
    pub fn around<'a>(
        points: impl IntoIterator<Item = &'a [f32; 3]>,
        margin_m: f32,
        cell_m: f32,
    ) -> Self {
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        for p in points {
            for k in 0..2 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        if !(min[0] <= max[0] && min[1] <= max[1]) {
            return Self::covering([-1000.0, -1000.0], [1000.0, 1000.0], cell_m);
        }
        let m = margin_m.max(0.0);
        Self::covering([min[0] - m, min[1] - m], [max[0] + m, max[1] + m], cell_m)
    }

    /// Number of cells.
    pub fn len(&self) -> usize {
        self.nx as usize * self.ny as usize
    }

    /// True if the grid has no cells.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Cell index containing world point `p` (Z ignored), or `None` outside the grid.
    pub fn cell_of(&self, p: [f32; 3]) -> Option<usize> {
        let fx = (p[0] - self.origin[0]) / self.cell_m;
        let fy = (p[1] - self.origin[1]) / self.cell_m;
        if !(fx >= 0.0 && fy >= 0.0) {
            return None; // also rejects NaN
        }
        let (ix, iy) = (fx as u32, fy as u32);
        if ix >= self.nx || iy >= self.ny {
            return None;
        }
        Some(iy as usize * self.nx as usize + ix as usize)
    }

    /// World XY of a cell's centre.
    pub fn cell_center(&self, i: usize) -> [f32; 2] {
        let ix = (i % self.nx as usize) as f32;
        let iy = (i / self.nx as usize) as f32;
        [
            self.origin[0] + (ix + 0.5) * self.cell_m,
            self.origin[1] + (iy + 0.5) * self.cell_m,
        ]
    }
}

/// Inputs of one hydrology update that are the same for every cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HydroInputs {
    /// Rain rate, mm/h.
    pub rain_mm_per_h: f32,
    /// Product of the temperature, wind, sun and humidity evaporation factors.
    pub evaporation_factor: f32,
}

/// Per-cell road water state (`WeatherState::wetness_by_cell`).
#[derive(Debug, Clone, PartialEq)]
pub struct WetnessGrid {
    spec: GridSpec,
    /// Surface per cell (+ background at the end).
    surface: Vec<SurfaceType>,
    /// Water capacity per cell, mm.
    cap_mm: Vec<f32>,
    /// Water per cell, mm.
    water_mm: Vec<f32>,
    /// Vehicles per cell, moving average.
    occupancy: Vec<f32>,
    /// Scratch: vehicles counted this update.
    counts: Vec<u16>,
    /// Indices of cells on road-like surfaces (for the mean wetness).
    road_cells: Vec<u32>,
}

/// A cell with standing water, for the renderer's puddle placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PuddleCell {
    /// Cell index.
    pub cell: u32,
    /// World XY of the cell centre, m.
    pub center: [f32; 2],
    /// Cell edge length, m (the puddle fits inside).
    pub size_m: f32,
    /// Standing-water depth above the film, mm.
    pub depth_mm: f32,
}

/// Road-like surfaces count toward the mean road wetness.
fn is_road(s: SurfaceType) -> bool {
    matches!(
        s,
        SurfaceType::Asphalt
            | SurfaceType::WetAsphalt
            | SurfaceType::Concrete
            | SurfaceType::Curb
            | SurfaceType::Metal
    )
}

fn surface_code(s: SurfaceType) -> u32 {
    ALL_SURFACES.iter().position(|x| *x == s).unwrap_or(0) as u32
}

impl WetnessGrid {
    /// Build a dry grid. `surfaces` holds one entry per cell (`spec.len()`); puddle-prone cells
    /// are chosen with `rng` from the table's `puddle_cell_fraction × puddle_weight`.
    ///
    /// # Panics
    /// If `surfaces.len() != spec.len()`.
    pub fn new(
        spec: GridSpec,
        surfaces: Vec<SurfaceType>,
        table: &SurfaceTable,
        rng: &mut Rng,
    ) -> Self {
        assert_eq!(surfaces.len(), spec.len(), "one surface per cell");
        let n = spec.len();
        let mut surface = surfaces;
        surface.push(SurfaceType::Asphalt); // background
        let mut cap_mm = Vec::with_capacity(n + 1);
        for (i, s) in surface.iter().enumerate() {
            let p = table.surface(*s);
            // Same number of draws per cell, so changing one weight does not reshuffle the map.
            let roll = rng.next_f32();
            let depth = rng.range_f32(table.puddle_capacity_mm[0], table.puddle_capacity_mm[1]);
            let puddle = i < n && roll < table.puddle_cell_fraction * p.puddle_weight;
            let cap = if puddle {
                depth.max(p.film_capacity_mm)
            } else {
                p.film_capacity_mm
            };
            cap_mm.push(cap.min(table.max_water_mm));
        }
        let road_cells = (0..n as u32)
            .filter(|i| is_road(surface[*i as usize]))
            .collect();
        Self {
            spec,
            surface,
            cap_mm,
            water_mm: vec![0.0; n + 1],
            occupancy: vec![0.0; n + 1],
            counts: vec![0; n + 1],
            road_cells,
        }
    }

    /// A grid of one surface everywhere (flat test worlds).
    pub fn uniform(
        spec: GridSpec,
        surface: SurfaceType,
        table: &SurfaceTable,
        rng: &mut Rng,
    ) -> Self {
        Self::new(spec, vec![surface; spec.len()], table, rng)
    }

    /// Sample `world.surface_at` at every cell centre (z = 0).
    pub fn from_world(
        spec: GridSpec,
        world: &dyn WorldQuery,
        table: &SurfaceTable,
        rng: &mut Rng,
    ) -> Self {
        let surfaces = (0..spec.len())
            .map(|i| {
                let c = spec.cell_center(i);
                world.surface_at([c[0], c[1], 0.0])
            })
            .collect();
        Self::new(spec, surfaces, table, rng)
    }

    /// Grid placement.
    pub fn spec(&self) -> &GridSpec {
        &self.spec
    }

    /// Cell index for a point; the background cell (`spec.len()`) outside the grid.
    pub fn index(&self, p: [f32; 3]) -> usize {
        self.spec.cell_of(p).unwrap_or(self.spec.len())
    }

    /// Number of cells including the background cell.
    pub fn len_with_background(&self) -> usize {
        self.water_mm.len()
    }

    /// Water per cell, mm (background last).
    pub fn water_mm(&self) -> &[f32] {
        &self.water_mm
    }

    /// Water capacity per cell, mm (background last).
    pub fn capacity_mm(&self) -> &[f32] {
        &self.cap_mm
    }

    /// Surface per cell (background last).
    pub fn surfaces(&self) -> &[SurfaceType] {
        &self.surface
    }

    /// Traffic occupancy per cell, vehicles (moving average; background last).
    pub fn occupancy(&self) -> &[f32] {
        &self.occupancy
    }

    /// Water at cell `i`, mm.
    pub fn water_at_cell(&self, i: usize) -> f32 {
        self.water_mm[i]
    }

    /// Wetness 0..1 of a water depth.
    pub fn wetness_of(water_mm: f32, table: &SurfaceTable) -> f32 {
        (water_mm / table.film_mm).clamp(0.0, 1.0)
    }

    /// Standing water of a water depth, mm.
    pub fn standing_of(water_mm: f32, table: &SurfaceTable) -> f32 {
        (water_mm - table.film_mm).max(0.0)
    }

    /// Set every cell's water to `water_mm` (clamped to each cell's capacity).
    pub fn fill(&mut self, water_mm: f32) {
        for (w, cap) in self.water_mm.iter_mut().zip(&self.cap_mm) {
            *w = water_mm.clamp(0.0, *cap);
        }
    }

    /// Set one cell's traffic occupancy directly (tests, tools).
    pub fn set_occupancy(&mut self, i: usize, vehicles: f32) {
        self.occupancy[i] = vehicles.max(0.0);
    }

    /// Advance water and traffic occupancy by `game_dt` in-game seconds. `vehicles` are world
    /// positions of every moving vehicle (players, police, traffic) this update.
    pub fn step(
        &mut self,
        game_dt: f32,
        inp: &HydroInputs,
        vehicles: &[[f32; 3]],
        table: &SurfaceTable,
    ) {
        // Traffic occupancy (moving average of vehicles per cell).
        self.counts.fill(0);
        for p in vehicles {
            let i = self.index(*p);
            self.counts[i] = self.counts[i].saturating_add(1);
        }
        let alpha = (game_dt / table.traffic_tau_game_s).clamp(0.0, 1.0);
        let bg = self.spec.len();
        for (occ, c) in self.occupancy.iter_mut().zip(&self.counts) {
            *occ += (*c as f32 - *occ) * alpha;
        }
        // The background cell is "somewhere else in the city": no traffic drying bonus.
        self.occupancy[bg] = 0.0;

        let hours = game_dt / 3600.0;
        let rain = inp.rain_mm_per_h.max(0.0) * hours;
        let evap_base = inp.evaporation_factor.max(0.0) * hours;
        let film = table.film_mm;
        // Per-surface constants, looked up once per update.
        let mut per_surface = [(0.0f32, 0.0f32); 8];
        for (k, s) in ALL_SURFACES.iter().enumerate() {
            let p = table.surface(*s);
            per_surface[k] = (p.dry_mm_per_h, p.infiltration_mm_per_h * hours);
        }
        for i in 0..self.water_mm.len() {
            let (dry_mm_per_h, infil) = per_surface[surface_code(self.surface[i]) as usize];
            let traffic = 1.0
                + table.traffic_drying_gain * self.occupancy[i].min(table.traffic_occupancy_cap);
            let evap = dry_mm_per_h * evap_base * traffic;
            let mut w = self.water_mm[i] + rain - evap;
            // Infiltration only drains water standing above the film.
            if w > film {
                w -= infil.min(w - film);
            }
            self.water_mm[i] = w.clamp(0.0, self.cap_mm[i]);
        }
    }

    /// Mean wetness over road-like cells (all cells if there are none), 0..1. Summed in index
    /// order, so it is deterministic.
    pub fn mean_road_wetness(&self, table: &SurfaceTable) -> f32 {
        let (sum, n) = if self.road_cells.is_empty() {
            let n = self.spec.len().max(1);
            let s: f32 = self.water_mm[..self.spec.len()]
                .iter()
                .map(|w| Self::wetness_of(*w, table))
                .sum();
            (s, n)
        } else {
            let s: f32 = self
                .road_cells
                .iter()
                .map(|i| Self::wetness_of(self.water_mm[*i as usize], table))
                .sum();
            (s, self.road_cells.len())
        };
        sum / n as f32
    }

    /// Cells with at least `min_depth_mm` of standing water, in index order.
    pub fn puddle_cells(&self, min_depth_mm: f32, table: &SurfaceTable) -> Vec<PuddleCell> {
        (0..self.spec.len())
            .filter_map(|i| {
                let depth = Self::standing_of(self.water_mm[i], table);
                (depth >= min_depth_mm && depth > 0.0).then(|| PuddleCell {
                    cell: i as u32,
                    center: self.spec.cell_center(i),
                    size_m: self.spec.cell_m,
                    depth_mm: depth,
                })
            })
            .collect()
    }

    /// Water per cell quantised to `u8` at 0.05 mm per step (0 … 12.75 mm), background
    /// excluded. Used for net snapshots and as a renderer texture (row-major, X fastest).
    pub fn water_q8(&self) -> Vec<u8> {
        self.water_mm[..self.spec.len()]
            .iter()
            .map(|w| (w * 20.0).round().clamp(0.0, 255.0) as u8)
            .collect()
    }

    /// Inverse of [`WetnessGrid::water_q8`]. Returns false (and changes nothing) on a size
    /// mismatch.
    pub fn apply_water_q8(&mut self, q: &[u8]) -> bool {
        if q.len() != self.spec.len() {
            return false;
        }
        for (i, b) in q.iter().enumerate() {
            self.water_mm[i] = (*b as f32 / 20.0).min(self.cap_mm[i]);
        }
        true
    }

    /// Wetness per cell as `u8` (0 dry … 255 fully wet), background excluded, for a renderer
    /// texture.
    pub fn wetness_u8(&self, table: &SurfaceTable) -> Vec<u8> {
        self.water_mm[..self.spec.len()]
            .iter()
            .map(|w| (Self::wetness_of(*w, table) * 255.0).round() as u8)
            .collect()
    }

    /// Mix the grid state into a determinism hash.
    pub fn hash_into(&self, h: &mut StateHasher) {
        h.write_u32(self.spec.nx);
        h.write_u32(self.spec.ny);
        for (w, o) in self.water_mm.iter().zip(&self.occupancy) {
            h.write_f32_q(*w, 1e4);
            h.write_f32_q(*o, 1e4);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::WeatherData;

    fn table() -> SurfaceTable {
        WeatherData::embedded().surfaces
    }

    #[test]
    fn cell_lookup_round_trips() {
        let spec = GridSpec::covering([-100.0, -50.0], [100.0, 50.0], 8.0);
        assert_eq!((spec.nx, spec.ny), (25, 13));
        for i in [0, 1, 24, 25, 200, spec.len() - 1] {
            let c = spec.cell_center(i);
            assert_eq!(spec.cell_of([c[0], c[1], 3.0]), Some(i));
        }
        assert_eq!(spec.cell_of([-101.0, 0.0, 0.0]), None);
        assert_eq!(spec.cell_of([f32::NAN, 0.0, 0.0]), None);
    }

    #[test]
    fn around_points_and_empty_fallback() {
        let pts = [[10.0, -20.0, 0.0], [110.0, 60.0, 5.0]];
        let g = GridSpec::around(pts.iter(), 50.0, 8.0);
        assert_eq!(g.origin, [-40.0, -70.0]);
        assert!(g.cell_of([159.9, 109.9, 0.0]).is_some());
        assert!(g.cell_of([-41.0, 0.0, 0.0]).is_none());
        let e = GridSpec::around(std::iter::empty(), 50.0, 8.0);
        assert!(e.cell_of([999.0, -999.0, 0.0]).is_some());
    }

    #[test]
    fn rain_fills_to_capacity_and_no_further() {
        let t = table();
        let spec = GridSpec::covering([0.0, 0.0], [80.0, 80.0], 8.0);
        let mut g = WetnessGrid::uniform(spec, SurfaceType::Asphalt, &t, &mut Rng::new(3));
        let inp = HydroInputs {
            rain_mm_per_h: 100.0,
            evaporation_factor: 0.1,
        };
        for _ in 0..200 {
            g.step(60.0, &inp, &[], &t);
        }
        for (w, cap) in g.water_mm().iter().zip(g.capacity_mm()) {
            assert!(*w >= 0.0 && *w <= *cap);
            assert!((*w - *cap).abs() < 1e-4);
        }
    }

    #[test]
    fn busy_cells_dry_first() {
        let t = table();
        let spec = GridSpec::covering([0.0, 0.0], [16.0, 8.0], 8.0);
        let mut g = WetnessGrid::uniform(spec, SurfaceType::Asphalt, &t, &mut Rng::new(1));
        g.fill(1.0);
        let busy = [[4.0, 4.0, 0.0]];
        let inp = HydroInputs {
            rain_mm_per_h: 0.0,
            evaporation_factor: 1.0,
        };
        for _ in 0..30 {
            g.step(60.0, &inp, &busy, &t);
        }
        assert!(g.water_at_cell(0) < g.water_at_cell(1));
        assert!(g.occupancy()[0] > 0.9);
    }

    #[test]
    fn grass_drains_standing_water_but_keeps_a_film() {
        let t = table();
        let spec = GridSpec::covering([0.0, 0.0], [8.0, 8.0], 8.0);
        let mut g = WetnessGrid::uniform(spec, SurfaceType::Grass, &t, &mut Rng::new(1));
        g.fill(3.0);
        let inp = HydroInputs {
            rain_mm_per_h: 0.0,
            evaporation_factor: 0.0,
        };
        g.step(3600.0, &inp, &[], &t);
        assert!((g.water_at_cell(0) - t.film_mm).abs() < 1e-4);
    }

    #[test]
    fn snapshot_bytes_round_trip() {
        let t = table();
        let spec = GridSpec::covering([0.0, 0.0], [64.0, 64.0], 8.0);
        let mut g = WetnessGrid::uniform(spec, SurfaceType::Asphalt, &t, &mut Rng::new(9));
        g.fill(1.2);
        let q = g.water_q8();
        let mut h = WetnessGrid::uniform(spec, SurfaceType::Asphalt, &t, &mut Rng::new(9));
        assert!(h.apply_water_q8(&q));
        assert_eq!(h.water_q8(), q);
        assert!(!h.apply_water_q8(&q[1..]));
    }

    #[test]
    fn some_cells_are_puddle_prone() {
        let t = table();
        let spec = GridSpec::covering([0.0, 0.0], [400.0, 400.0], 8.0);
        let g = WetnessGrid::uniform(spec, SurfaceType::Asphalt, &t, &mut Rng::new(5));
        let film = t.surface(SurfaceType::Asphalt).film_capacity_mm;
        let puddles = g.capacity_mm()[..spec.len()]
            .iter()
            .filter(|c| **c > film)
            .count();
        let frac = puddles as f32 / spec.len() as f32;
        assert!((0.03..0.09).contains(&frac), "{frac}");
    }
}
