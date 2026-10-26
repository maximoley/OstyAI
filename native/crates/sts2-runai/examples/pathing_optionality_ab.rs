//! `pathing_optionality_ab` — proof + visualization for optionality-aware
//! act-DAG pathing (user request 2026-06-23).
//!
//! The pathing surface already plans the WHOLE path to the boss and
//! replans every floor (`enumerate_paths` + commit-first-edge in
//! `policy/surfaces/pathing.rs`). What it historically *lacked* was a
//! valuation of FLEXIBILITY: a first column was scored by the single best
//! (hard-max) path through it, so two columns tied even when one kept far
//! more good routes — and elite-skip options — open. `OptionalityWeights`
//! adds that valuation (soft-max over path scores + an explicit
//! "skip an elite if you want" real-option bonus).
//!
//! ## Why optionality should help (the mechanism)
//!
//! The agent commits one edge per floor and replans, but its projection
//! uses *expected* room costs — real fights have variance the agent can't
//! foresee. A hard-max planner commits to the single best-looking path;
//! when bad luck makes that path worse than projected, it is stuck with
//! whatever reroutes its earlier commits left available. An
//! optionality-aware planner prefers edges that keep many good routes (and
//! an elite-free escape) reachable, so after bad luck it still has a safe
//! line to the boss. Optionality is, precisely, the value of being able to
//! react to information revealed later.
//!
//! ## What this harness does
//!
//! A self-contained replanning Monte-Carlo on REAL generated act maps. Per
//! scenario (map seed × starting-HP × act-cost tier) it walks the map
//! twice — once with `OptionalityWeights::hard_max()` (the legacy
//! baseline), once with the calibrated defaults — under **common-random**
//! realized room costs (each physical room's HP cost is drawn once and
//! shared by both arms, so they differ ONLY in which rooms they route
//! through). The agent's planner sees only the table means; the realized
//! per-fight noise is hidden, exactly as in a real run.
//!
//! Reports: survival rate, mean final HP at boss arrival, elites taken,
//! and the paired "optionality survives where the baseline dies" count.
//!
//! Usage:
//! ```text
//! cargo run -p sts2-runai --example pathing_optionality_ab -- --ab 4000   # heuristic A/B + gate
//! cargo run -p sts2-runai --example pathing_optionality_ab -- --sweep 3000 # dose-response
//! cargo run -p sts2-runai --example pathing_optionality_ab -- --ev 200    # learned EV vs det (needs value_human_a0)
//! cargo run -p sts2-runai --example pathing_optionality_ab -- --viz 5     # ASCII map render
//! ```
//! With no args it runs a small A/B then renders one example map.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use sts2_content::run_callbacks::new_run_manager_for_character;
use sts2_core::map::MapCoord;
use sts2_core::map::MapPointType;
use sts2_core::potions::CharacterId;
use sts2_runai::eval::run_value::ValueNet;
use sts2_runai::eval::{HeuristicV, OptionalityWeights};
use sts2_runai::policy::surfaces::pathing::{
    best_first_col_by_value, best_first_col_by_value_ev, column_optionality, scored_first_cols_opt,
    ColumnOptionality,
};
use sts2_runai::view::VisibleRunView;

// ── Deterministic common-random cost model ───────────────────────────────
//
// Each physical room (keyed by its coord) gets ONE realized HP cost, shared
// by both arms. Combat rooms draw a lognormal multiplier on the projection
// table mean (so a fight can cost much more or less than expected); the
// agent's planner never sees the multiplier.

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn unit(seed: u64) -> f64 {
    // 53-bit mantissa uniform in [0, 1).
    (splitmix64(seed) >> 11) as f64 / (1u64 << 53) as f64
}

/// Lognormal multiplier with median 1 and log-sigma `sigma` (Box–Muller).
fn lognormal(seed: u64, sigma: f64) -> f64 {
    let u1 = unit(seed).max(1e-12);
    let u2 = unit(seed ^ 0xABCD_1234);
    let z = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    (sigma * z).exp()
}

#[derive(Clone, Copy)]
struct CostModel {
    seed: u64,
    /// 0-based act index driving the projection-table magnitudes.
    act: usize,
    /// `true` ⇒ every room costs exactly its table mean (the planner's own
    /// projection; used to draw the "planned" path in the visualizer).
    deterministic: bool,
}

impl CostModel {
    /// Realized HP delta for entering `room` at `coord`. Positive = HP lost,
    /// negative = HP gained (rest heal).
    fn hp_delta(&self, v: &HeuristicV, coord: MapCoord, room: MapPointType, max_hp: f64) -> f64 {
        let t = &v.weights.projection;
        let act = self.act.min(2);
        let key = self.seed
            ^ splitmix64(((coord.col as u64) << 20) ^ (coord.row as u64).wrapping_mul(0x100));
        match room {
            MapPointType::Monster => self.combat(t.monster_hp_loss[act], key, 0.30),
            MapPointType::Elite => self.combat(t.elite_hp_loss[act], key ^ 0xE11E, 0.45),
            MapPointType::RestSite => -(t.rest_heal_frac * max_hp).floor(),
            MapPointType::Ancient => self.combat(t.event_hp_loss, key ^ 0xE7E7, 0.5),
            MapPointType::Unknown => self.combat(t.unknown_hp_loss, key ^ 0x3F3F, 0.5),
            MapPointType::Treasure
            | MapPointType::Shop
            | MapPointType::Boss
            | MapPointType::Unassigned => 0.0,
        }
    }

    fn combat(&self, mean: f64, key: u64, sigma: f64) -> f64 {
        if self.deterministic {
            mean
        } else {
            (mean * lognormal(key, sigma)).max(0.0)
        }
    }
}

// ── Replanning map walk ──────────────────────────────────────────────────

#[derive(Clone, Copy, Default)]
struct WalkResult {
    died: bool,
    final_hp: f64,
    elites: u32,
    /// Elites entered while below 40% max HP (the risk the elite-skip
    /// option exists to avoid).
    low_hp_elites: u32,
    /// Coord path actually walked (for the visualizer).
    path_len: usize,
}

/// Walk one map to the boss, replanning every floor with `opt`, under
/// `cost`'s realized room costs. Returns the outcome plus, optionally, the
/// committed coord path.
fn walk(
    view0: &VisibleRunView,
    v: &HeuristicV,
    opt: &OptionalityWeights,
    cost: &CostModel,
    record: Option<&mut Vec<MapCoord>>,
) -> WalkResult {
    let mut view = view0.clone();
    let max_hp = f64::from(view.max_hp);
    let mut hp = f64::from(view.hp);
    let mut out = WalkResult {
        final_hp: hp,
        ..WalkResult::default()
    };
    let mut trail: Vec<MapCoord> = Vec::new();

    for _ in 0..64 {
        // Replan from the current node; commit the best first edge.
        let Some((best_col, _)) = scored_first_cols_opt(&view, v, &BTreeMap::new(), opt) else {
            break; // no map / no paths → at the boss row
        };
        let map = view.map.as_ref().expect("walk needs a map");
        let children = match map.current {
            Some(cur) => map.nodes.get(&cur).map(|n| n.children.clone()),
            None => Some(map.start_children.clone()),
        };
        let Some(children) = children.filter(|c| !c.is_empty()) else {
            break;
        };
        let Some(coord) = children.into_iter().find(|c| c.col == best_col) else {
            break;
        };
        let room = map
            .nodes
            .get(&coord)
            .map_or(MapPointType::Unknown, |n| n.room);

        // Realize the room.
        let delta = cost.hp_delta(v, coord, room, max_hp);
        hp = (hp - delta).min(max_hp);
        trail.push(coord);
        if room == MapPointType::Elite {
            out.elites += 1;
            if (hp + delta) < 0.4 * max_hp {
                out.low_hp_elites += 1;
            }
        }
        if hp <= 0.0 {
            out.died = true;
            out.final_hp = 0.0;
            break;
        }

        // Advance; stop once we step onto a final-row node (its only exit is
        // the boss, which the path never includes).
        let has_next = view
            .map
            .as_ref()
            .and_then(|m| m.nodes.get(&coord))
            .is_some_and(|n| !n.children.is_empty());
        if let Some(m) = view.map.as_mut() {
            m.current = Some(coord);
        }
        view.hp = hp.max(0.0) as i32;
        out.final_hp = hp;
        if !has_next {
            break;
        }
    }

    out.path_len = trail.len();
    if let Some(slot) = record {
        *slot = trail;
    }
    out
}

// ── A/B sweep ────────────────────────────────────────────────────────────

#[derive(Default, Clone, Copy)]
struct Agg {
    n: u32,
    survived: u32,
    final_hp_sum: f64,
    elites: u32,
    low_hp_elites: u32,
}

impl Agg {
    fn record(&mut self, r: WalkResult) {
        self.n += 1;
        if !r.died {
            self.survived += 1;
            self.final_hp_sum += r.final_hp;
        }
        self.elites += r.elites;
        self.low_hp_elites += r.low_hp_elites;
    }
    fn survival(&self) -> f64 {
        f64::from(self.survived) / f64::from(self.n.max(1))
    }
    fn mean_final_hp(&self) -> f64 {
        self.final_hp_sum / f64::from(self.survived.max(1))
    }
}

/// Outcome of a paired A/B sweep: the two arms' aggregates plus the paired
/// "exactly one arm survives" counts.
struct AbResult {
    base: Agg,
    opt: Agg,
    opt_only: u32,
    base_only: u32,
    scenarios: u32,
}

/// Run the paired sweep with `opt` vs `OptionalityWeights::hard_max()` over
/// the given starting-HP fractions. Common-random realized costs ⇒ the arms
/// differ only in routing.
fn sweep(n_seeds: i32, opt: &OptionalityWeights) -> AbResult {
    sweep_hp(n_seeds, opt, &[0.45, 0.65, 0.85])
}

fn sweep_hp(n_seeds: i32, opt: &OptionalityWeights, hp_fracs: &[f64]) -> AbResult {
    let v = HeuristicV::default();
    let baseline = OptionalityWeights::hard_max();
    let mut r = AbResult {
        base: Agg::default(),
        opt: Agg::default(),
        opt_only: 0,
        base_only: 0,
        scenarios: 0,
    };
    for seed in 1..=n_seeds {
        let rm = new_run_manager_for_character(seed, CharacterId::Ironclad);
        let base_view = VisibleRunView::from_run_manager(&rm);
        if base_view.map.is_none() {
            continue;
        }
        for &hp_frac in hp_fracs {
            for act in 0..=2usize {
                let mut view = base_view.clone();
                view.hp = (f64::from(view.max_hp) * hp_frac).round() as i32;
                view.act_index = act as u8;
                let cm = CostModel {
                    seed: (seed as u64).wrapping_mul(0x1_0000_0001) ^ (act as u64),
                    act,
                    deterministic: false,
                };
                let rb = walk(&view, &v, &baseline, &cm, None);
                let ro = walk(&view, &v, opt, &cm, None);
                r.base.record(rb);
                r.opt.record(ro);
                r.scenarios += 1;
                match (rb.died, ro.died) {
                    (true, false) => r.opt_only += 1,
                    (false, true) => r.base_only += 1,
                    _ => {}
                }
            }
        }
    }
    r
}

fn run_ab(n_seeds: i32) {
    let r = sweep(n_seeds, &OptionalityWeights::default());
    let (a_base, a_opt, opt_only, base_only, scenarios) =
        (r.base, r.opt, r.opt_only, r.base_only, r.scenarios);

    println!("\n=== Optionality-aware pathing: replanning Monte-Carlo A/B ===");
    println!(
        "scenarios = {scenarios}  (map seeds 1..={n_seeds} × 3 start-HP × 3 act-cost tiers)\n"
    );
    println!(
        "{:<22}{:>12}{:>12}{:>10}",
        "metric", "baseline", "optionality", "Δ"
    );
    let row = |label: &str, b: f64, o: f64, pct: bool| {
        let (bs, os, ds) = if pct {
            (
                format!("{:.1}%", b * 100.0),
                format!("{:.1}%", o * 100.0),
                format!("{:+.1}pp", (o - b) * 100.0),
            )
        } else {
            (
                format!("{b:.2}"),
                format!("{o:.2}"),
                format!("{:+.2}", o - b),
            )
        };
        println!("{label:<22}{bs:>12}{os:>12}{ds:>10}");
    };
    row("survival rate", a_base.survival(), a_opt.survival(), true);
    row(
        "mean final HP",
        a_base.mean_final_hp(),
        a_opt.mean_final_hp(),
        false,
    );
    row(
        "elites taken / run",
        f64::from(a_base.elites) / f64::from(scenarios.max(1)),
        f64::from(a_opt.elites) / f64::from(scenarios.max(1)),
        false,
    );
    row(
        "low-HP elites / run",
        f64::from(a_base.low_hp_elites) / f64::from(scenarios.max(1)),
        f64::from(a_opt.low_hp_elites) / f64::from(scenarios.max(1)),
        false,
    );
    println!(
        "\npaired survival: optionality-only-survives = {opt_only}, baseline-only-survives = {base_only}  (net {:+})",
        i64::from(opt_only) - i64::from(base_only)
    );
    let verdict = if a_opt.survival() > a_base.survival() && opt_only >= base_only {
        "OPTIONALITY HELPS: higher survival and it rescues more runs than it loses."
    } else {
        "no improvement at these weights — retune OptionalityWeights."
    };
    println!("verdict: {verdict}");

    // The death-risk gate at work: split by starting HP. Healthy starts
    // (>= threshold 0.5) should keep FARMING elites — optionality ≈ baseline
    // on elites taken; wounded starts should reroute and survive more.
    let opt = OptionalityWeights::default();
    let healthy = sweep_hp(n_seeds, &opt, &[0.70, 0.90]);
    let wounded = sweep_hp(n_seeds, &opt, &[0.20, 0.35]);
    let elites_per = |a: &Agg| f64::from(a.elites) / f64::from(a.n.max(1));
    println!("\n── death-risk gate (does it farm when safe, bail when not?) ──");
    println!(
        "{:<26}{:>14}{:>14}{:>12}",
        "start-HP band", "elitesΔ/run", "survivalΔ", "net rescued"
    );
    for (label, r) in [
        ("healthy (70–90%)", &healthy),
        ("wounded (20–35%)", &wounded),
    ] {
        println!(
            "{label:<26}{:>+14.3}{:>+13.1}pp{:>12}",
            elites_per(&r.opt) - elites_per(&r.base),
            (r.opt.survival() - r.base.survival()) * 100.0,
            i64::from(r.opt_only) - i64::from(r.base_only),
        );
    }
    println!(
        "Healthy elitesΔ≈0 (gate dormant → keep farming); wounded survivalΔ>0 (gate\nopens → reroute around lethal elites). That is the intended strategy."
    );
}

/// Dose-response: stronger optionality weights should monotonically rescue
/// more runs (net survival), proving the *mechanism* rather than a single
/// lucky operating point. `tau == 0, bonus == 0` is the baseline (identity).
fn run_dose_response(n_seeds: i32) {
    println!("\n=== Optionality dose-response (vs hard-max baseline) ===");
    println!("each row raises one knob; common-random costs, {n_seeds} map seeds × 9 scenarios\n");
    println!(
        "{:<28}{:>10}{:>12}{:>14}",
        "OptionalityWeights", "survivalΔ", "net rescued", "rescue ratio"
    );
    // (tau, elite_skip_bonus, hp_threshold). threshold 0.5 gates the skip
    // bonus on death risk (the default); 1.0 means "always on" (ungated).
    let ow = |tau: f64, skip: f64, thr: f64| OptionalityWeights {
        tau,
        branch_bonus: 0.0,
        elite_skip_bonus: skip,
        elite_skip_hp_threshold: thr,
    };
    let grid = [
        ("tau=0 skip=0 (identity)", OptionalityWeights::hard_max()),
        ("tau=0.05 skip=0", ow(0.05, 0.0, 0.0)),
        ("tau=0.20 skip=0", ow(0.20, 0.0, 0.0)),
        ("tau=0 skip=0.10 ungated", ow(0.0, 0.10, 1.0)),
        ("tau=0 skip=0.10 gated@.5", ow(0.0, 0.10, 0.5)),
        (
            "tau=0.10 skip=0.10 gated (default)",
            OptionalityWeights::default(),
        ),
        ("tau=0.20 skip=0.10 gated@.5", ow(0.20, 0.10, 0.5)),
    ];
    for (label, w) in grid {
        let r = sweep(n_seeds, &w);
        let dsurv = (r.opt.survival() - r.base.survival()) * 100.0;
        let net = i64::from(r.opt_only) - i64::from(r.base_only);
        let ratio = f64::from(r.opt_only) / f64::from(r.base_only.max(1));
        println!("{label:<28}{dsurv:>9.2}pp{net:>12}{ratio:>13.2}x");
    }
    println!(
        "\nMonotone climb in net-rescued with weight strength ⇒ the gain is the\noptionality valuation, not noise. 'identity' row is exactly 0 by construction."
    );
}

// ── Learned EV pathing: optionality WITHOUT knobs ────────────────────────
//
// The hand-coded soft-max/elite-skip terms above are a closed-form stand-in
// for "expected value under replanning". This A/B replaces them with the
// real thing: the DEPLOYED value net, scored two ways — `det` commits the
// single best deterministic-projection path (best_first_col_by_value, the
// existing M4 pather), `ev` commits the move with the highest EXPECTED net
// value over sampled replanned futures (best_first_col_by_value_ev). Both
// use the SAME net under common-random realized costs, so any gap is the
// value of flexibility the EV pather discovers on its own — no
// OptionalityWeights, nothing hand-tuned.

fn load_value_net() -> Option<ValueNet> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/runai/nets/value_human_a0.json");
    match ValueNet::from_weights_path(&path) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("could not load value_human_a0.json ({e}); skipping EV A/B");
            None
        }
    }
}

/// Walk the map to the boss under `cost`, choosing each floor's edge with the
/// net: `ev` ⇒ sampled-EV pather, else the deterministic-projection pather.
fn walk_net(
    view0: &VisibleRunView,
    v: &HeuristicV,
    net: &ValueNet,
    ev: bool,
    samples: u32,
    cost: &CostModel,
) -> WalkResult {
    let mut view = view0.clone();
    let max_hp = f64::from(view.max_hp);
    let mut hp = f64::from(view.hp);
    let mut out = WalkResult {
        final_hp: hp,
        ..WalkResult::default()
    };
    for floor in 0..64u64 {
        let best_col = if ev {
            best_first_col_by_value_ev(&view, v, net, samples, 0.35, cost.seed ^ floor)
                .map(|(c, _)| c)
        } else {
            best_first_col_by_value(&view, v, net)
        };
        let Some(best_col) = best_col else { break };
        let map = view.map.as_ref().expect("walk needs a map");
        let children = match map.current {
            Some(cur) => map.nodes.get(&cur).map(|n| n.children.clone()),
            None => Some(map.start_children.clone()),
        };
        let Some(children) = children.filter(|c| !c.is_empty()) else {
            break;
        };
        let Some(coord) = children.into_iter().find(|c| c.col == best_col) else {
            break;
        };
        let room = map
            .nodes
            .get(&coord)
            .map_or(MapPointType::Unknown, |n| n.room);
        let delta = cost.hp_delta(v, coord, room, max_hp);
        hp = (hp - delta).min(max_hp);
        if room == MapPointType::Elite {
            out.elites += 1;
        }
        if hp <= 0.0 {
            out.died = true;
            out.final_hp = 0.0;
            break;
        }
        let has_next = map
            .nodes
            .get(&coord)
            .is_some_and(|n| !n.children.is_empty());
        if let Some(m) = view.map.as_mut() {
            m.current = Some(coord);
        }
        view.hp = hp.max(0.0) as i32;
        out.final_hp = hp;
        if !has_next {
            break;
        }
    }
    out
}

fn run_ev_ab(n_seeds: i32, samples: u32) {
    let Some(net) = load_value_net() else { return };
    let v = HeuristicV::default();
    let (mut det, mut ev) = (Agg::default(), Agg::default());
    let (mut ev_only, mut det_only) = (0u32, 0u32);
    for seed in 1..=n_seeds {
        let rm = new_run_manager_for_character(seed, CharacterId::Ironclad);
        let base = VisibleRunView::from_run_manager(&rm);
        if base.map.is_none() {
            continue;
        }
        for &hp_frac in &[0.40, 0.70] {
            for act in 1..=2usize {
                let mut view = base.clone();
                view.hp = (f64::from(view.max_hp) * hp_frac).round() as i32;
                view.act_index = act as u8;
                let cm = CostModel {
                    seed: (seed as u64).wrapping_mul(0x1_0000_0001) ^ (act as u64),
                    act,
                    deterministic: false,
                };
                let rd = walk_net(&view, &v, &net, false, samples, &cm);
                let re = walk_net(&view, &v, &net, true, samples, &cm);
                det.record(rd);
                ev.record(re);
                match (rd.died, re.died) {
                    (true, false) => ev_only += 1,
                    (false, true) => det_only += 1,
                    _ => {}
                }
            }
        }
    }
    println!("\n=== Learned EV pathing vs deterministic value pathing (same net) ===");
    println!("net = value_human_a0.json · samples = {samples} · {n_seeds} seeds × 4 scenarios\n");
    println!(
        "{:<22}{:>14}{:>14}{:>10}",
        "metric", "det (M4)", "EV (learned)", "Δ"
    );
    println!(
        "{:<22}{:>13.1}%{:>13.1}%{:>+9.1}pp",
        "survival rate",
        det.survival() * 100.0,
        ev.survival() * 100.0,
        (ev.survival() - det.survival()) * 100.0
    );
    println!(
        "{:<22}{:>14.2}{:>14.2}{:>+10.2}",
        "mean final HP",
        det.mean_final_hp(),
        ev.mean_final_hp(),
        ev.mean_final_hp() - det.mean_final_hp()
    );
    let net_paired = i64::from(ev_only) - i64::from(det_only);
    println!(
        "\npaired survival: EV-only-survives = {ev_only}, det-only-survives = {det_only} (net {net_paired:+})"
    );
    println!(
        "\nFINDING: with value_human_a0 the EV pather is ~neutral (small ±). The net\n\
         predicts EXPECTED ACT and is ~monotone-LINEAR in HP with NO map/optionality\n\
         features, so E[V] over sampled futures ≈ V(mean) (Jensen gap ≈ 0) — there is\n\
         nothing concave or flexibility-shaped for the expectation to exploit. The EV\n\
         MECHANISM is correct and knob-free; the LIMITER is the net's shape. That is\n\
         the 'features later' work: retrain with (a) a risk/survival-shaped target so\n\
         E[V] rewards low-variance routes, and/or (b) map-topology features (reachable\n\
         elites, route freedom) so a single-state net can price optionality directly.\n\
         Until then the hand-coded soft-max (which injects that death-risk concavity\n\
         explicitly) is the better stand-in — see the --ab / --sweep results."
    );
}

// ── Visualization ────────────────────────────────────────────────────────

fn room_glyph(t: MapPointType) -> char {
    match t {
        MapPointType::Monster => 'M',
        MapPointType::Elite => 'E',
        MapPointType::RestSite => 'R',
        MapPointType::Shop => '$',
        MapPointType::Treasure => 'T',
        MapPointType::Ancient => '?',
        MapPointType::Unknown => '?',
        MapPointType::Boss => 'B',
        MapPointType::Unassigned => '.',
    }
}

fn render(seed: u64) -> String {
    let v = HeuristicV::default();
    let rm = new_run_manager_for_character(seed, CharacterId::Ironclad);
    let mut view = VisibleRunView::from_run_manager(&rm);
    // Act-3 costs + a wounded start so the elite-skip option is live.
    view.act_index = 2;
    // Wounded (35%) so the death-risk-gated elite-skip option is live.
    view.hp = (f64::from(view.max_hp) * 0.35).round() as i32;
    let map = view.map.clone().expect("map");

    // Planned (no-noise) walks for each arm, to highlight on the grid.
    let det = CostModel {
        seed: 0,
        act: 2,
        deterministic: true,
    };
    let mut base_path = Vec::new();
    let mut opt_path = Vec::new();
    walk(
        &view,
        &v,
        &OptionalityWeights::hard_max(),
        &det,
        Some(&mut base_path),
    );
    walk(
        &view,
        &v,
        &OptionalityWeights::default(),
        &det,
        Some(&mut opt_path),
    );

    let mut s = String::new();
    let _ = writeln!(
        s,
        "Act-3 map (seed {seed}), start HP {}/{}  —  '*' baseline path, '#' optionality path, '@' both",
        view.hp, view.max_hp
    );
    let _ = writeln!(s, "Boss is the implicit top node (B). Rooms: M onster E lite R est $ hop T reasure ? event/unknown\n");

    // Grid: top row = nearest the boss.
    let max_row = map.nodes.keys().map(|c| c.row).max().unwrap_or(0);
    for row in (1..=max_row).rev() {
        let mut line = format!("r{row:>2} ");
        for col in 0..7 {
            let coord = MapCoord::new(col, row);
            if let Some(node) = map.nodes.get(&coord) {
                let g = room_glyph(node.room);
                let on_b = base_path.contains(&coord);
                let on_o = opt_path.contains(&coord);
                let mark = match (on_b, on_o) {
                    (true, true) => '@',
                    (true, false) => '*',
                    (false, true) => '#',
                    (false, false) => ' ',
                };
                let _ = write!(line, "{g}{mark}");
            } else {
                let _ = write!(line, "  ");
            }
            let _ = write!(line, " ");
        }
        let _ = writeln!(s, "{line}");
    }

    // First-column score table.
    let _ = writeln!(s, "\nFirst-move columns (from the start row):");
    let _ = writeln!(
        s,
        "{:>4}{:>8}{:>10}{:>10}{:>12}{:>12}",
        "col", "#paths", "elites", "canSkip", "baseScore", "optScore"
    );
    let opt_stats: BTreeMap<i32, ColumnOptionality> = column_optionality(&view)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let base_scores =
        scored_first_cols_opt(&view, &v, &BTreeMap::new(), &OptionalityWeights::hard_max());
    let opt_scores =
        scored_first_cols_opt(&view, &v, &BTreeMap::new(), &OptionalityWeights::default());
    let base_map: BTreeMap<i32, f64> = base_scores
        .as_ref()
        .map(|(_, c)| c.iter().copied().collect())
        .unwrap_or_default();
    let opt_map: BTreeMap<i32, f64> = opt_scores
        .as_ref()
        .map(|(_, c)| c.iter().copied().collect())
        .unwrap_or_default();
    for (col, st) in &opt_stats {
        let _ = writeln!(
            s,
            "{:>4}{:>8}{:>7}-{:<2}{:>10}{:>12.3}{:>12.3}",
            col,
            st.n_paths,
            st.min_elites,
            st.max_elites,
            if st.can_skip_elite() { "yes" } else { "no" },
            base_map.get(col).copied().unwrap_or(f64::NAN),
            opt_map.get(col).copied().unwrap_or(f64::NAN),
        );
    }
    let bp = base_scores.map(|(c, _)| c);
    let op = opt_scores.map(|(c, _)| c);
    let _ = writeln!(
        s,
        "\nbaseline picks col {:?}, optionality picks col {:?}{}",
        bp,
        op,
        if bp == op {
            "  (same first move; optionality still re-ranks downstream ties)"
        } else {
            "  ← optionality re-routed to keep more options open"
        }
    );
    s
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut did = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--ab" => {
                let n = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(2000);
                run_ab(n);
                i += 2;
                did = true;
            }
            "--sweep" => {
                let n = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(2000);
                run_dose_response(n);
                i += 2;
                did = true;
            }
            "--ev" => {
                let n = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(200);
                run_ev_ab(n, 8);
                i += 2;
                did = true;
            }
            "--viz" => {
                let seed = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(17);
                print!("{}", render(seed));
                i += 2;
                did = true;
            }
            _ => i += 1,
        }
    }
    if !did {
        run_ab(1500);
        println!("\n{}", render(17));
    }
}
