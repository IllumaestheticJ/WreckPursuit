// Shared e2e helper: start a quick (non-career) picker race the car may enter.
//
// Races refuse cars outside their entry band (wp_race::entry; the testbed reopens the picker),
// and with the developer panel's "career" box ticked a race becomes a career run. So: untick
// career, then try the races of `raceList(car, mode)` in order until one starts.

/** Start the first race `car` may enter (`car` null: the developer panel's car). Returns the
 * race id, or null when none starts. */
export async function startQuickRace(page, car = null, mode = "Arcade") {
  return page.evaluate(
    ([car, mode]) => {
      const w = window.__wp;
      const career = document.getElementById("career");
      if (career) career.checked = false;
      const listCar = car ?? (document.getElementById("car")?.value || "lindwurm_4t");
      for (const r of w.sim.raceList(listCar, mode)) {
        if (car) w.startRace(r.id, car);
        else w.startRace(r.id);
        if (w.sim.scenario().name.startsWith("race_")) return r.id;
      }
      return null;
    },
    [car, mode],
  );
}

/** Start the quick (non-career) race `id` on the page (as `?race=<id>` did before career races
 * existed: with the "career" box ticked a race is a career run). Returns the scenario name. */
export async function startRaceById(page, id, car = null) {
  return page.evaluate(
    ([id, car]) => {
      const w = window.__wp;
      const career = document.getElementById("career");
      if (career) career.checked = false;
      if (car) w.startRace(id, car);
      else w.startRace(id);
      return w.sim.scenario().name;
    },
    [id, car],
  );
}
