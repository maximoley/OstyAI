//! Combat-outcome surrogate net inference (AlphaZero outer-loop plan,
//! Milestone 1, Task 1; bead TBD).
//!
//! `(ContextFeatures, encounter_idx) -> (win_prob, hp_frac_lost)` — the
//! per-encounter combat-outcome surrogate that will replace/augment the
//! MCTS combat probe in later milestones. Hand-rolled forward + a single
//! JSON weights file, same no-new-deps pattern as
//! [`run_value::ValueNet`](crate::eval::run_value::ValueNet).
//!
//! The context half of the input is the SAME dense layout as the ranker/
//! value nets (densified via [`ContextFeatures::write_dense`], then
//! standardized by the trainer's stored `feat_mu`/`feat_sd`). The
//! encounter half is a learned embedding table keyed by a small integer
//! `encounter_idx` — a fixed, sidecar-pinned index space (e.g. "which
//! monster/elite/boss encounter"), with the LAST row reserved as the
//! out-of-distribution fallback for any `idx >= n_encounters`.
//!
//! ## Weight file (trainer `--out`)
//!
//! `{schema_id, engine, context_dim, n_encounters, enc_dim, hidden,
//! feat_mu[ctx], feat_sd[ctx], enc_emb[n_encounters+1][enc_dim],
//! w1[context_dim+enc_dim][hidden], b1[hidden], w_win[hidden], b_win,
//! w_hp[hidden], b_hp, win_calib}`. `schema_id` MUST equal
//! [`ranker_features::feature_schema_id`] (the loader refuses otherwise —
//! a mismatched index space scores garbage). `win_calib` (optional;
//! `null`/absent = identity) is a [`WinCalib`] applied to `win_prob`
//! AFTER the sigmoid, inside the shared `forward` used by both
//! [`SurrogateNet::predict`] and [`SurrogateNet::predict_from_dense`] —
//! `hp_frac_lost` is never calibrated.

use std::path::Path;

use serde::Deserialize;

use crate::eval::ranker_features::{self, context_dim, ContextFeatures};

/// Predicted combat outcome for one `(context, encounter)` pair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CombatOutcome {
    /// `P(win)`, sigmoid-squashed into `[0, 1]`.
    pub win_prob: f32,
    /// Expected fraction of max HP lost, clamped into `[0, 1]`.
    pub hp_frac_lost: f32,
}

/// Errors loading or validating a surrogate-net weights JSON. Mirrors
/// [`run_value::ValueError`](crate::eval::run_value::ValueError).
#[derive(Debug)]
pub enum SurrogateError {
    Io(std::io::Error),
    Parse(String),
    SchemaMismatch {
        expected: String,
        got: String,
    },
    Shape {
        which: &'static str,
        expected: usize,
        got: usize,
    },
}

impl std::fmt::Display for SurrogateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "surrogate weights read failed: {e}"),
            Self::Parse(m) => write!(f, "surrogate weights parse failed: {m}"),
            Self::SchemaMismatch { expected, got } => write!(
                f,
                "surrogate-net FEATURE SCHEMA MISMATCH: weights trained for '{got}' but this \
                 build is '{expected}' — re-export features + retrain"
            ),
            Self::Shape {
                which,
                expected,
                got,
            } => {
                write!(
                    f,
                    "surrogate weights {which} length {got} mismatch — expected {expected}"
                )
            }
        }
    }
}

impl std::error::Error for SurrogateError {}

impl From<std::io::Error> for SurrogateError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Calibration applied to the raw sigmoid `win_prob` output, inside the
/// shared [`SurrogateNet::forward`] — so both [`SurrogateNet::predict`]
/// and [`SurrogateNet::predict_from_dense`] apply it identically.
/// `hp_frac_lost` is NEVER calibrated.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WinCalib {
    /// `p' = sigmoid(a * logit(p) + b)`.
    Platt { a: f32, b: f32 },
    /// Piecewise-linear interpolation over non-decreasing `(x, y)`
    /// breakpoints; clamps below `x[0]` -> `y[0]` and above `x[last]` ->
    /// `y[last]`.
    Isotonic { x: Vec<f32>, y: Vec<f32> },
}

impl WinCalib {
    /// Apply this calibrator to a raw sigmoid probability, clamped to
    /// `[0, 1]`. `pub(crate)` so sibling modules with their own value
    /// heads (e.g. [`run_value_net`](crate::eval::run_value_net)) can
    /// reuse the same calibrator type without duplicating it.
    pub(crate) fn apply(&self, p: f32) -> f32 {
        match self {
            Self::Platt { a, b } => {
                let p = p.clamp(1e-6, 1.0 - 1e-6);
                let logit = (p / (1.0 - p)).ln();
                let z = a * logit + b;
                1.0 / (1.0 + (-z).exp())
            }
            Self::Isotonic { x, y } => isotonic_interp(x, y, p),
        }
        .clamp(0.0, 1.0)
    }
}

/// Piecewise-linear interpolation of `raw_p` over the non-decreasing
/// `(breaks_x, breaks_y)` breakpoints. Clamps to the first/last `breaks_y`
/// outside the `breaks_x` range; returns `raw_p` unchanged if `breaks_x`
/// is empty (degenerate/malformed calibrator).
fn isotonic_interp(breaks_x: &[f32], breaks_y: &[f32], raw_p: f32) -> f32 {
    let len = breaks_x.len().min(breaks_y.len());
    if len == 0 {
        return raw_p;
    }
    if raw_p <= breaks_x[0] {
        return breaks_y[0];
    }
    if raw_p >= breaks_x[len - 1] {
        return breaks_y[len - 1];
    }
    for i in 1..len {
        if raw_p <= breaks_x[i] {
            let (lo_x, hi_x) = (breaks_x[i - 1], breaks_x[i]);
            let (lo_y, hi_y) = (breaks_y[i - 1], breaks_y[i]);
            let span = hi_x - lo_x;
            if span <= 0.0 {
                return hi_y;
            }
            let frac = (raw_p - lo_x) / span;
            return lo_y + frac * (hi_y - lo_y);
        }
    }
    breaks_y[len - 1]
}

#[derive(Debug, Deserialize)]
struct SurrogateJson {
    schema_id: String,
    #[allow(dead_code)]
    engine: String,
    context_dim: usize,
    n_encounters: usize,
    enc_dim: usize,
    hidden: usize,
    feat_mu: Vec<f32>,
    feat_sd: Vec<f32>,
    /// `[n_encounters+1][enc_dim]` row-major; last row is the OOD fallback.
    enc_emb: Vec<Vec<f32>>,
    /// `[context_dim+enc_dim][hidden]` row-major.
    w1: Vec<Vec<f32>>,
    b1: Vec<f32>,
    w_win: Vec<f32>,
    b_win: f32,
    w_hp: Vec<f32>,
    b_hp: f32,
    #[serde(default)]
    win_calib: Option<WinCalib>,
}

/// One-hidden-layer combat-outcome surrogate over `[dense context ++
/// encounter embedding]`. `Send + Sync` (plain boxed slices) — share via
/// `Arc` across workers.
#[derive(Debug)]
pub struct SurrogateNet {
    schema_id: String,
    n_encounters: usize,
    enc_dim: usize,
    hidden: usize,
    feat_mu: Box<[f32]>,
    feat_sd: Box<[f32]>,
    /// `[(n_encounters+1) × enc_dim]` row-major; last row = OOD fallback.
    enc_emb: Box<[f32]>,
    /// `[(context_dim+enc_dim) × hidden]` row-major.
    w1: Box<[f32]>,
    b1: Box<[f32]>,
    w_win: Box<[f32]>,
    b_win: f32,
    w_hp: Box<[f32]>,
    b_hp: f32,
    /// Applied to `win_prob` after the sigmoid, inside `forward` (Task 5).
    win_calib: Option<WinCalib>,
}

impl SurrogateNet {
    /// Load + validate a surrogate-net weights JSON. Fails loudly on
    /// schema-id mismatch and shape drift.
    pub fn from_weights_path(path: &Path) -> Result<Self, SurrogateError> {
        Self::from_json_slice(&std::fs::read(path)?)
    }

    /// Load from raw JSON bytes (test fixtures).
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, SurrogateError> {
        let json: SurrogateJson =
            serde_json::from_slice(bytes).map_err(|e| SurrogateError::Parse(e.to_string()))?;
        let expected = ranker_features::feature_schema_id();
        if json.schema_id != expected {
            return Err(SurrogateError::SchemaMismatch {
                expected: expected.to_string(),
                got: json.schema_id,
            });
        }
        let cdim = context_dim();
        if json.context_dim != cdim {
            return Err(SurrogateError::Shape {
                which: "context_dim",
                expected: cdim,
                got: json.context_dim,
            });
        }
        if json.hidden == 0 {
            return Err(SurrogateError::Parse("zero hidden width".to_string()));
        }
        if json.enc_dim == 0 {
            return Err(SurrogateError::Parse("zero enc_dim width".to_string()));
        }
        if json.n_encounters == 0 {
            return Err(SurrogateError::Parse("zero n_encounters".to_string()));
        }
        if json.feat_mu.len() != cdim {
            return Err(SurrogateError::Shape {
                which: "feat_mu",
                expected: cdim,
                got: json.feat_mu.len(),
            });
        }
        if json.feat_sd.len() != cdim {
            return Err(SurrogateError::Shape {
                which: "feat_sd",
                expected: cdim,
                got: json.feat_sd.len(),
            });
        }
        if json.feat_sd.iter().any(|&s| s <= 0.0 || !s.is_finite()) {
            return Err(SurrogateError::Parse(
                "feat_sd must be strictly positive + finite".to_string(),
            ));
        }
        let expected_enc_rows = json.n_encounters + 1;
        if json.enc_emb.len() != expected_enc_rows {
            return Err(SurrogateError::Shape {
                which: "enc_emb.rows",
                expected: expected_enc_rows,
                got: json.enc_emb.len(),
            });
        }
        let mut enc_emb = Vec::with_capacity(expected_enc_rows * json.enc_dim);
        for row in &json.enc_emb {
            if row.len() != json.enc_dim {
                return Err(SurrogateError::Shape {
                    which: "enc_emb.cols",
                    expected: json.enc_dim,
                    got: row.len(),
                });
            }
            enc_emb.extend_from_slice(row);
        }
        let expected_w1_rows = cdim + json.enc_dim;
        if json.w1.len() != expected_w1_rows {
            return Err(SurrogateError::Shape {
                which: "w1.rows",
                expected: expected_w1_rows,
                got: json.w1.len(),
            });
        }
        let mut w1 = Vec::with_capacity(expected_w1_rows * json.hidden);
        for row in &json.w1 {
            if row.len() != json.hidden {
                return Err(SurrogateError::Shape {
                    which: "w1.cols",
                    expected: json.hidden,
                    got: row.len(),
                });
            }
            w1.extend_from_slice(row);
        }
        if json.b1.len() != json.hidden {
            return Err(SurrogateError::Shape {
                which: "b1",
                expected: json.hidden,
                got: json.b1.len(),
            });
        }
        if json.w_win.len() != json.hidden {
            return Err(SurrogateError::Shape {
                which: "w_win",
                expected: json.hidden,
                got: json.w_win.len(),
            });
        }
        if json.w_hp.len() != json.hidden {
            return Err(SurrogateError::Shape {
                which: "w_hp",
                expected: json.hidden,
                got: json.w_hp.len(),
            });
        }
        Ok(Self {
            schema_id: json.schema_id,
            n_encounters: json.n_encounters,
            enc_dim: json.enc_dim,
            hidden: json.hidden,
            feat_mu: json.feat_mu.into_boxed_slice(),
            feat_sd: json.feat_sd.into_boxed_slice(),
            enc_emb: enc_emb.into_boxed_slice(),
            w1: w1.into_boxed_slice(),
            b1: json.b1.into_boxed_slice(),
            w_win: json.w_win.into_boxed_slice(),
            b_win: json.b_win,
            w_hp: json.w_hp.into_boxed_slice(),
            b_hp: json.b_hp,
            win_calib: json.win_calib,
        })
    }

    /// Schema id the weights validated against.
    #[must_use]
    pub fn schema_id(&self) -> &str {
        &self.schema_id
    }

    /// `(win_prob, hp_frac_lost)` for `ctx` against encounter `encounter_idx`.
    /// `encounter_idx >= n_encounters` (OOD) uses the reserved last
    /// `enc_emb` row.
    #[must_use]
    pub fn predict(&self, ctx: &ContextFeatures, encounter_idx: usize) -> CombatOutcome {
        let mut dense = vec![0.0f32; self.feat_mu.len()];
        ctx.write_dense(&mut dense);
        self.forward(&dense, encounter_idx)
    }

    /// Same forward as [`Self::predict`], but takes the **raw densified
    /// context** directly (the output of [`ContextFeatures::write_dense`],
    /// BEFORE standardization) instead of building it from a
    /// `ContextFeatures`. Standardization with the net's own
    /// `feat_mu`/`feat_sd` happens inside [`Self::forward`], shared with
    /// `predict` — so a caller must NOT pre-standardize `dense`, or the
    /// input gets standardized twice.
    ///
    /// Exists so a Rust<->torch parity fixture can drive the net without
    /// reconstructing a `ContextFeatures` (see
    /// `tests/surrogate_parity.rs`). Panics (via `assert_eq!`) if
    /// `dense.len() != context_dim()` — a length mismatch means the
    /// fixture was densified against a different feature schema.
    #[must_use]
    pub fn predict_from_dense(&self, dense: &[f32], encounter_idx: usize) -> CombatOutcome {
        assert_eq!(
            dense.len(),
            self.feat_mu.len(),
            "predict_from_dense: raw dense context length mismatch (expected {}, got {})",
            self.feat_mu.len(),
            dense.len()
        );
        self.forward(dense, encounter_idx)
    }

    /// Shared forward: standardize `dense` (raw, pre-standardization)
    /// with `feat_mu`/`feat_sd`, concat the `encounter_idx` embedding,
    /// one hidden ReLU layer, two linear heads, then apply `win_calib`
    /// (if any) to the win-head sigmoid output. Both [`Self::predict`]
    /// and [`Self::predict_from_dense`] delegate here so they can't
    /// diverge.
    fn forward(&self, dense: &[f32], encounter_idx: usize) -> CombatOutcome {
        let row = encounter_idx.min(self.n_encounters);
        let emb = &self.enc_emb[row * self.enc_dim..(row + 1) * self.enc_dim];

        let h_dim = self.hidden;
        let mut h = self.b1.to_vec();
        for (i, (&xi, (&mu, &sd))) in dense
            .iter()
            .zip(self.feat_mu.iter().zip(self.feat_sd.iter()))
            .enumerate()
        {
            let xn = (xi - mu) / sd;
            if xn == 0.0 {
                continue;
            }
            let wrow = &self.w1[i * h_dim..(i + 1) * h_dim];
            for (o, wv) in h.iter_mut().zip(wrow) {
                *o += xn * wv;
            }
        }
        let ctx_rows = dense.len();
        for (j, &ev) in emb.iter().enumerate() {
            if ev == 0.0 {
                continue;
            }
            let wrow_idx = ctx_rows + j;
            let wrow = &self.w1[wrow_idx * h_dim..(wrow_idx + 1) * h_dim];
            for (o, wv) in h.iter_mut().zip(wrow) {
                *o += ev * wv;
            }
        }
        for v in &mut h {
            *v = v.max(0.0);
        }

        let mut win_logit = self.b_win;
        for (&hv, &wv) in h.iter().zip(self.w_win.iter()) {
            win_logit += hv * wv;
        }
        let win_prob = 1.0 / (1.0 + (-win_logit).exp());
        let win_prob = match &self.win_calib {
            Some(calib) => calib.apply(win_prob),
            None => win_prob,
        };

        let mut hp = self.b_hp;
        for (&hv, &wv) in h.iter().zip(self.w_hp.iter()) {
            hp += hv * wv;
        }
        let hp_frac_lost = hp.clamp(0.0, 1.0);

        CombatOutcome {
            win_prob,
            hp_frac_lost,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::ranker_features::{context_dim, feature_schema_id};

    /// Hand-arithmetic fixture: context_dim inputs all zero via feat_mu==x,
    /// enc_emb row 0 = [1.0], w1 maps enc slot to hidden=1, both heads read it.
    fn tiny_net_json() -> String {
        let cdim = context_dim();
        let feat_mu: Vec<f32> = vec![0.0; cdim];
        let feat_sd: Vec<f32> = vec![1.0; cdim];
        // w1 rows: cdim zero-rows (context) then 1 row for the enc slot = [2.0].
        let mut w1: Vec<Vec<f32>> = vec![vec![0.0]; cdim];
        w1.push(vec![2.0]); // enc_dim=1 contributes 2.0*enc_emb -> hidden pre-act
        format!(
            r#"{{"schema_id":"{sid}","engine":"test","context_dim":{cdim},
            "n_encounters":1,"enc_dim":1,"hidden":1,
            "feat_mu":{fmu},"feat_sd":{fsd},
            "enc_emb":[[1.0],[0.0]],
            "w1":{w1},"b1":[0.0],
            "w_win":[1.0],"b_win":0.0,
            "w_hp":[0.25],"b_hp":0.0,
            "win_calib":null}}"#,
            sid = feature_schema_id(),
            cdim = cdim,
            fmu = serde_json::to_string(&feat_mu).unwrap(),
            fsd = serde_json::to_string(&feat_sd).unwrap(),
            w1 = serde_json::to_string(&w1).unwrap(),
        )
    }

    fn zero_ctx() -> ContextFeatures {
        ContextFeatures {
            deck: vec![],
            relics: vec![],
            scalars: vec![0.0; crate::eval::ranker_features::n_scalars()],
        }
    }

    #[test]
    fn surrogate_forward_matches_hand_arithmetic() {
        let net = SurrogateNet::from_json_slice(tiny_net_json().as_bytes()).unwrap();
        // With all context standardized to 0 and enc_emb[0]=1.0:
        //   pre-act hidden = 2.0*1.0 = 2.0 -> relu -> 2.0
        //   win_logit = 1.0*2.0 = 2.0 -> sigmoid(2.0) ≈ 0.880797
        //   hp_frac   = 0.25*2.0 = 0.5
        let ctx = zero_ctx();
        let out = net.predict(&ctx, 0);
        assert!(
            (out.win_prob - 0.880797).abs() < 1e-4,
            "win={}",
            out.win_prob
        );
        assert!(
            (out.hp_frac_lost - 0.5).abs() < 1e-6,
            "hp={}",
            out.hp_frac_lost
        );
    }

    #[test]
    fn schema_mismatch_is_rejected() {
        let bad = tiny_net_json().replace(feature_schema_id(), "WRONG-SCHEMA");
        assert!(matches!(
            SurrogateNet::from_json_slice(bad.as_bytes()),
            Err(SurrogateError::SchemaMismatch { .. })
        ));
    }

    #[test]
    fn ood_encounter_uses_last_emb_row() {
        let net = SurrogateNet::from_json_slice(tiny_net_json().as_bytes()).unwrap();
        // enc_emb row 1 (OOD) = [0.0] -> hidden 0 -> win sigmoid(0)=0.5, hp 0.
        let out = net.predict(&zero_ctx(), 99);
        assert!((out.win_prob - 0.5).abs() < 1e-6);
        assert!(out.hp_frac_lost.abs() < 1e-6);
    }

    #[test]
    fn platt_calibration_is_applied_and_monotone() {
        // net with win_calib platt {a:2.0,b:0.0}: p' = sigmoid(2*logit(p)).
        let json = tiny_net_json().replace(
            "\"win_calib\":null",
            "\"win_calib\":{\"kind\":\"platt\",\"a\":2.0,\"b\":0.0}",
        );
        let net = SurrogateNet::from_json_slice(json.as_bytes()).unwrap();
        // uncalibrated win_prob at enc 0 was 0.880797 (logit≈2.0);
        // platt a=2 -> sigmoid(4.0) ≈ 0.982014.
        let out = net.predict(&zero_ctx(), 0);
        assert!(
            (out.win_prob - 0.982014).abs() < 1e-4,
            "cal win={}",
            out.win_prob
        );
        // monotonicity: enc 1 (logit 0 -> 0.5) stays 0.5 under platt b=0.
        let lo = net.predict(&zero_ctx(), 1);
        assert!(lo.win_prob < out.win_prob);
    }
}
