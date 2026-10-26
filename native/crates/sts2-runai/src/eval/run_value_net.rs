//! Whole-run win-probability value net inference (AlphaZero outer-loop
//! plan, Milestone 4; bead TBD).
//!
//! `ContextFeatures -> P(win)` — the outer-loop value head that scores a
//! run state's probability of eventually winning, plus an act-reached
//! auxiliary head (diagnostics/parity only, not consumed outside tests).
//! Hand-rolled forward + a single JSON weights file, same no-new-deps
//! pattern as [`run_value::ValueNet`](crate::eval::run_value::ValueNet)
//! and [`surrogate::SurrogateNet`](crate::eval::surrogate::SurrogateNet),
//! which this module mirrors closely (shared trunk + densify/standardize
//! convention) minus the encounter embedding, plus the aux head.
//!
//! ## Weight file (trainer `--out`)
//!
//! `{schema_id, engine, context_dim, hidden, feat_mu[ctx], feat_sd[ctx],
//! w1[context_dim][hidden], b1[hidden], w_val[hidden], b_val,
//! w_act[hidden][4], b_act[4], win_calib}`. `schema_id` MUST equal
//! [`ranker_features::feature_schema_id`] (the loader refuses otherwise —
//! a mismatched index space scores garbage). `win_calib` (optional;
//! `null`/absent = identity) is a
//! [`WinCalib`](crate::eval::surrogate::WinCalib) applied to the value
//! head's sigmoid output, inside the shared `forward` used by
//! [`RunValueNet::win_prob`] — the act-reached aux head is never
//! calibrated.

use std::path::Path;

use serde::Deserialize;

use crate::eval::ranker_features::{self, context_dim, ContextFeatures};
use crate::eval::surrogate::WinCalib;

/// Number of act-reached buckets the aux head predicts over.
const N_ACT_BUCKETS: usize = 4;

/// Errors loading or validating a run-value-net weights JSON. Mirrors
/// [`surrogate::SurrogateError`](crate::eval::surrogate::SurrogateError).
#[derive(Debug)]
pub enum RunValueNetError {
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

impl std::fmt::Display for RunValueNetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "run-value-net weights read failed: {e}"),
            Self::Parse(m) => write!(f, "run-value-net weights parse failed: {m}"),
            Self::SchemaMismatch { expected, got } => write!(
                f,
                "run-value-net FEATURE SCHEMA MISMATCH: weights trained for '{got}' but this \
                 build is '{expected}' — re-export features + retrain"
            ),
            Self::Shape {
                which,
                expected,
                got,
            } => {
                write!(
                    f,
                    "run-value-net weights {which} length {got} mismatch — expected {expected}"
                )
            }
        }
    }
}

impl std::error::Error for RunValueNetError {}

impl From<std::io::Error> for RunValueNetError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug, Deserialize)]
struct RunValueJson {
    schema_id: String,
    #[allow(dead_code)]
    engine: String,
    context_dim: usize,
    hidden: usize,
    feat_mu: Vec<f32>,
    feat_sd: Vec<f32>,
    /// `[context_dim][hidden]` row-major.
    w1: Vec<Vec<f32>>,
    b1: Vec<f32>,
    w_val: Vec<f32>,
    b_val: f32,
    /// `[hidden][N_ACT_BUCKETS]` row-major.
    w_act: Vec<Vec<f32>>,
    b_act: Vec<f32>,
    #[serde(default)]
    win_calib: Option<WinCalib>,
}

/// One-hidden-layer whole-run value net: shared `ContextFeatures` trunk,
/// a win-probability head (sigmoid + optional [`WinCalib`]) and an
/// act-reached auxiliary head (softmax over [`N_ACT_BUCKETS`] buckets).
/// `Send + Sync` (plain boxed slices) — share via `Arc` across workers.
#[derive(Debug)]
pub struct RunValueNet {
    schema_id: String,
    hidden: usize,
    feat_mu: Box<[f32]>,
    feat_sd: Box<[f32]>,
    /// `[context_dim × hidden]` row-major.
    w1: Box<[f32]>,
    b1: Box<[f32]>,
    w_val: Box<[f32]>,
    b_val: f32,
    /// Applied to the value head's sigmoid output, inside `forward`.
    win_calib: Option<WinCalib>,
    /// `[hidden × N_ACT_BUCKETS]` row-major.
    w_act: Box<[f32]>,
    b_act: [f32; N_ACT_BUCKETS],
}

impl RunValueNet {
    /// Load + validate a run-value-net weights JSON file. Fails loudly on
    /// schema-id mismatch and shape drift.
    pub fn from_weights_path(path: &Path) -> Result<Self, RunValueNetError> {
        Self::from_json_str(&std::fs::read_to_string(path)?)
    }

    /// Load from a raw JSON string (test fixtures).
    pub fn from_json_str(s: &str) -> Result<Self, RunValueNetError> {
        let json: RunValueJson =
            serde_json::from_str(s).map_err(|e| RunValueNetError::Parse(e.to_string()))?;
        let expected = ranker_features::feature_schema_id();
        if json.schema_id != expected {
            return Err(RunValueNetError::SchemaMismatch {
                expected: expected.to_string(),
                got: json.schema_id,
            });
        }
        let cdim = context_dim();
        if json.context_dim != cdim {
            return Err(RunValueNetError::Shape {
                which: "context_dim",
                expected: cdim,
                got: json.context_dim,
            });
        }
        if json.hidden == 0 {
            return Err(RunValueNetError::Parse("zero hidden width".to_string()));
        }
        if json.feat_mu.len() != cdim {
            return Err(RunValueNetError::Shape {
                which: "feat_mu",
                expected: cdim,
                got: json.feat_mu.len(),
            });
        }
        if json.feat_sd.len() != cdim {
            return Err(RunValueNetError::Shape {
                which: "feat_sd",
                expected: cdim,
                got: json.feat_sd.len(),
            });
        }
        if json.feat_sd.iter().any(|&s| s <= 0.0 || !s.is_finite()) {
            return Err(RunValueNetError::Parse(
                "feat_sd must be strictly positive + finite".to_string(),
            ));
        }
        if json.w1.len() != cdim {
            return Err(RunValueNetError::Shape {
                which: "w1.rows",
                expected: cdim,
                got: json.w1.len(),
            });
        }
        let mut w1 = Vec::with_capacity(cdim * json.hidden);
        for row in &json.w1 {
            if row.len() != json.hidden {
                return Err(RunValueNetError::Shape {
                    which: "w1.cols",
                    expected: json.hidden,
                    got: row.len(),
                });
            }
            w1.extend_from_slice(row);
        }
        if json.b1.len() != json.hidden {
            return Err(RunValueNetError::Shape {
                which: "b1",
                expected: json.hidden,
                got: json.b1.len(),
            });
        }
        if json.w_val.len() != json.hidden {
            return Err(RunValueNetError::Shape {
                which: "w_val",
                expected: json.hidden,
                got: json.w_val.len(),
            });
        }
        if json.w_act.len() != json.hidden {
            return Err(RunValueNetError::Shape {
                which: "w_act.rows",
                expected: json.hidden,
                got: json.w_act.len(),
            });
        }
        let mut w_act = Vec::with_capacity(json.hidden * N_ACT_BUCKETS);
        for row in &json.w_act {
            if row.len() != N_ACT_BUCKETS {
                return Err(RunValueNetError::Shape {
                    which: "w_act.cols",
                    expected: N_ACT_BUCKETS,
                    got: row.len(),
                });
            }
            w_act.extend_from_slice(row);
        }
        if json.b_act.len() != N_ACT_BUCKETS {
            return Err(RunValueNetError::Shape {
                which: "b_act",
                expected: N_ACT_BUCKETS,
                got: json.b_act.len(),
            });
        }
        let mut b_act = [0.0f32; N_ACT_BUCKETS];
        b_act.copy_from_slice(&json.b_act);

        Ok(Self {
            schema_id: json.schema_id,
            hidden: json.hidden,
            feat_mu: json.feat_mu.into_boxed_slice(),
            feat_sd: json.feat_sd.into_boxed_slice(),
            w1: w1.into_boxed_slice(),
            b1: json.b1.into_boxed_slice(),
            w_val: json.w_val.into_boxed_slice(),
            b_val: json.b_val,
            win_calib: json.win_calib,
            w_act: w_act.into_boxed_slice(),
            b_act,
        })
    }

    /// Schema id the weights validated against.
    #[must_use]
    pub fn schema_id(&self) -> &str {
        &self.schema_id
    }

    /// `P(win)` for `ctx`: densify + standardize -> one hidden ReLU layer
    /// -> sigmoid(value head) -> optional [`WinCalib`], clamped into
    /// `[0, 1]`.
    #[must_use]
    pub fn win_prob(&self, ctx: &ContextFeatures) -> f32 {
        let mut dense = vec![0.0f32; self.feat_mu.len()];
        ctx.write_dense(&mut dense);
        self.forward(&dense).0
    }

    /// Same forward as [`Self::win_prob`], but takes the **raw densified
    /// context** directly (the output of [`ContextFeatures::write_dense`],
    /// BEFORE standardization) instead of building it from a
    /// `ContextFeatures`. Standardization with the net's own `feat_mu`/
    /// `feat_sd` happens inside [`Self::forward`], shared with
    /// [`Self::win_prob`] — so a caller must NOT pre-standardize `dense`,
    /// or the input gets standardized twice.
    ///
    /// Exists so a Rust<->torch parity fixture can drive the net without
    /// reconstructing a `ContextFeatures` (see
    /// `tests/run_value_net_parity.rs`, mirrors
    /// [`SurrogateNet::predict_from_dense`](crate::eval::surrogate::SurrogateNet::predict_from_dense)).
    /// Panics (via `assert_eq!`) if `dense.len() != context_dim()` — a
    /// length mismatch means the fixture was densified against a
    /// different feature schema.
    #[must_use]
    pub fn win_prob_from_dense(&self, dense: &[f32]) -> f32 {
        assert_eq!(
            dense.len(),
            self.feat_mu.len(),
            "win_prob_from_dense: raw dense context length mismatch (expected {}, got {})",
            self.feat_mu.len(),
            dense.len()
        );
        self.forward(dense).0
    }

    /// Act-reached aux head: `softmax(W_act^T h + b_act)` over
    /// [`N_ACT_BUCKETS`] buckets. Diagnostics/parity only — not part of
    /// the deployed inference surface, so kept test-only.
    #[cfg(test)]
    fn act_probs(&self, ctx: &ContextFeatures) -> [f32; N_ACT_BUCKETS] {
        let mut dense = vec![0.0f32; self.feat_mu.len()];
        ctx.write_dense(&mut dense);
        self.forward(&dense).1
    }

    /// Shared forward: standardize `dense` (raw, pre-standardization)
    /// with `feat_mu`/`feat_sd` (sparse-skip zeros, mirroring
    /// [`run_value::ValueNet`](crate::eval::run_value::ValueNet)'s
    /// `densify`), one hidden ReLU layer, then both heads.
    /// [`Self::win_prob`], [`Self::win_prob_from_dense`], and
    /// [`Self::act_probs`] delegate here so they can't diverge.
    fn forward(&self, dense: &[f32]) -> (f32, [f32; N_ACT_BUCKETS]) {
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
        for v in &mut h {
            *v = v.max(0.0);
        }

        let mut val_logit = self.b_val;
        for (&hv, &wv) in h.iter().zip(self.w_val.iter()) {
            val_logit += hv * wv;
        }
        let raw_win_prob = 1.0 / (1.0 + (-val_logit).exp());
        let win_prob = match &self.win_calib {
            Some(calib) => calib.apply(raw_win_prob),
            None => raw_win_prob,
        };

        let mut act_logits = self.b_act;
        for (hi, &hv) in h.iter().enumerate() {
            let wrow = &self.w_act[hi * N_ACT_BUCKETS..(hi + 1) * N_ACT_BUCKETS];
            for (o, wv) in act_logits.iter_mut().zip(wrow) {
                *o += hv * wv;
            }
        }
        let max_logit = act_logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mut act_probs = [0.0f32; N_ACT_BUCKETS];
        let mut sum = 0.0f32;
        for (o, &l) in act_probs.iter_mut().zip(act_logits.iter()) {
            *o = (l - max_logit).exp();
            sum += *o;
        }
        for o in &mut act_probs {
            *o /= sum;
        }

        (win_prob, act_probs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::ranker_features::{context_dim, feature_schema_id, n_scalars};

    /// Hand-arithmetic fixture: `feat_mu`/`feat_sd` neutral (0/1) so a
    /// zero context standardizes to all-zero, meaning the hidden layer is
    /// exactly `relu(b1)`. `b1 = [1.0, -1.0]` -> `h = [1.0, 0.0]`.
    fn tiny_net_json() -> String {
        let cdim = context_dim();
        serde_json::json!({
            "schema_id": feature_schema_id(),
            "engine": "test",
            "context_dim": cdim,
            "hidden": 2,
            "feat_mu": vec![0.0f32; cdim],
            "feat_sd": vec![1.0f32; cdim],
            "w1": vec![vec![0.0f32, 0.0f32]; cdim],
            "b1": [1.0f32, -1.0f32],
            "w_val": [2.0f32, 3.0f32],
            "b_val": 0.5f32,
            "w_act": [[1.0f32, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]],
            "b_act": [0.0f32, 0.0, 0.0, 0.0],
            "win_calib": null,
        })
        .to_string()
    }

    fn zero_ctx() -> ContextFeatures {
        ContextFeatures {
            deck: vec![],
            relics: vec![],
            scalars: vec![0.0; n_scalars()],
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn win_prob_matches_hand_arithmetic() {
        let net = RunValueNet::from_json_str(&tiny_net_json()).expect("loads");
        // h = relu([1.0, -1.0]) = [1.0, 0.0]
        // val_logit = 0.5 + 1.0*2.0 + 0.0*3.0 = 2.5
        let expected = 1.0 / (1.0 + (-2.5f32).exp());
        let got = net.win_prob(&zero_ctx());
        assert!(
            (got - expected).abs() < 1e-6,
            "expected {expected}, got {got}"
        );
    }

    #[test]
    fn act_probs_sum_to_one() {
        let net = RunValueNet::from_json_str(&tiny_net_json()).expect("loads");
        let probs = net.act_probs(&zero_ctx());
        let sum: f32 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "sum={sum}, probs={probs:?}");
        // act_logits = b_act + h[0]*[1,0,0,0] + h[1]*[0,1,0,0] = [1,0,0,0]
        // -> bucket 0 has the highest probability.
        assert!(probs[0] > probs[1]);
        assert!(probs[0] > probs[2]);
        assert!(probs[0] > probs[3]);
    }

    #[test]
    fn schema_mismatch_is_rejected() {
        let bad = tiny_net_json().replace(feature_schema_id(), "WRONG-SCHEMA");
        assert!(matches!(
            RunValueNet::from_json_str(&bad),
            Err(RunValueNetError::SchemaMismatch { .. })
        ));
    }
}
