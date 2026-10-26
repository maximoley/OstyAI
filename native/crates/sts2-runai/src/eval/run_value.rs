//! Run-level value net inference (full-run-AI plan §5/M4; bead
//! `sts2-rs-40ikw.8`). Hand-rolled forward + single-JSON loader, same
//! no-new-deps pattern as [`ranker`](crate::eval::ranker).
//!
//! `V(run_state) → expected final act reached` — the graded, non-myopic
//! outcome value (trained by `sts2-pycli/python/train_value.py`). One
//! hidden ReLU layer over the **dense** context vector (deck histogram
//! `2·n_card` + relic multi-hot + scalars — the SAME layout as the
//! ranker's [`ContextFeatures`], densified here), standardized per the
//! trainer's stored feature mean/std, then de-normalized to act units.
//!
//! ## Weight file (train_value.py `--out`)
//!
//! `{schema_id, context_dim, hidden, feat_mu[ctx], feat_sd[ctx], y_mu,
//! y_sd, w1[ctx][hidden], b1[hidden], w2[hidden], b2}`. `schema_id` MUST
//! equal [`ranker_features::feature_schema_id`] (the loader refuses
//! otherwise — a mismatched index space scores garbage).

use std::path::Path;

use serde::Deserialize;

use crate::eval::ranker_features::{
    self, context_dim, n_cards, n_relics, n_scalars, ContextFeatures,
};

/// Errors loading or validating a value-net weights JSON.
#[derive(Debug)]
pub enum ValueError {
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

impl std::fmt::Display for ValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "value weights read failed: {e}"),
            Self::Parse(m) => write!(f, "value weights parse failed: {m}"),
            Self::SchemaMismatch { expected, got } => write!(
                f,
                "value-net FEATURE SCHEMA MISMATCH: weights trained for '{got}' but this build \
                 is '{expected}' — re-export features + retrain (train_value.py)"
            ),
            Self::Shape {
                which,
                expected,
                got,
            } => {
                write!(
                    f,
                    "value weights {which} length {got} mismatch — expected {expected}"
                )
            }
        }
    }
}

impl std::error::Error for ValueError {}

impl From<std::io::Error> for ValueError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug, Deserialize)]
struct ValueJson {
    schema_id: String,
    context_dim: usize,
    hidden: usize,
    feat_mu: Vec<f32>,
    feat_sd: Vec<f32>,
    y_mu: f32,
    y_sd: f32,
    /// `[context_dim][hidden]` row-major (input-feature fan-out per row).
    w1: Vec<Vec<f32>>,
    b1: Vec<f32>,
    /// `[hidden]`.
    w2: Vec<f32>,
    b2: f32,
}

/// One-hidden-layer value net over the dense run-state context.
/// `Send + Sync` (plain boxed slices) — share via `Arc` across workers.
#[derive(Debug)]
pub struct ValueNet {
    schema_id: String,
    hidden: usize,
    feat_mu: Box<[f32]>,
    feat_sd: Box<[f32]>,
    y_mu: f32,
    y_sd: f32,
    /// `[context_dim × hidden]` row-major.
    w1: Box<[f32]>,
    b1: Box<[f32]>,
    w2: Box<[f32]>,
    b2: f32,
}

impl ValueNet {
    /// Load + validate a `train_value.py` weights JSON. Fails loudly on
    /// schema-id mismatch and shape drift.
    pub fn from_weights_path(path: &Path) -> Result<Self, ValueError> {
        Self::from_json_slice(&std::fs::read(path)?)
    }

    /// Load from raw JSON bytes (test fixtures).
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, ValueError> {
        let json: ValueJson =
            serde_json::from_slice(bytes).map_err(|e| ValueError::Parse(e.to_string()))?;
        let expected = ranker_features::feature_schema_id();
        if json.schema_id != expected {
            return Err(ValueError::SchemaMismatch {
                expected: expected.to_string(),
                got: json.schema_id,
            });
        }
        let cdim = context_dim();
        if json.context_dim != cdim {
            return Err(ValueError::Shape {
                which: "context_dim",
                expected: cdim,
                got: json.context_dim,
            });
        }
        if json.hidden == 0 {
            return Err(ValueError::Parse("zero hidden width".to_string()));
        }
        if json.feat_mu.len() != cdim {
            return Err(ValueError::Shape {
                which: "feat_mu",
                expected: cdim,
                got: json.feat_mu.len(),
            });
        }
        if json.feat_sd.len() != cdim {
            return Err(ValueError::Shape {
                which: "feat_sd",
                expected: cdim,
                got: json.feat_sd.len(),
            });
        }
        if json.feat_sd.iter().any(|&s| s <= 0.0 || !s.is_finite()) {
            return Err(ValueError::Parse(
                "feat_sd must be strictly positive + finite".to_string(),
            ));
        }
        if json.w1.len() != cdim {
            return Err(ValueError::Shape {
                which: "w1.rows",
                expected: cdim,
                got: json.w1.len(),
            });
        }
        let mut w1 = Vec::with_capacity(cdim * json.hidden);
        for row in &json.w1 {
            if row.len() != json.hidden {
                return Err(ValueError::Shape {
                    which: "w1.cols",
                    expected: json.hidden,
                    got: row.len(),
                });
            }
            w1.extend_from_slice(row);
        }
        if json.b1.len() != json.hidden {
            return Err(ValueError::Shape {
                which: "b1",
                expected: json.hidden,
                got: json.b1.len(),
            });
        }
        if json.w2.len() != json.hidden {
            return Err(ValueError::Shape {
                which: "w2",
                expected: json.hidden,
                got: json.w2.len(),
            });
        }
        Ok(Self {
            schema_id: json.schema_id,
            hidden: json.hidden,
            feat_mu: json.feat_mu.into_boxed_slice(),
            feat_sd: json.feat_sd.into_boxed_slice(),
            y_mu: json.y_mu,
            y_sd: json.y_sd,
            w1: w1.into_boxed_slice(),
            b1: json.b1.into_boxed_slice(),
            w2: json.w2.into_boxed_slice(),
            b2: json.b2,
        })
    }

    /// Schema id the weights validated against.
    #[must_use]
    pub fn schema_id(&self) -> &str {
        &self.schema_id
    }

    /// Densify a [`ContextFeatures`] into the `context_dim` vector
    /// (mirrors `train_value.py::densify` / `ContextFeatures::write_dense`).
    fn densify(&self, ctx: &ContextFeatures) -> Vec<f32> {
        let mut x = vec![0.0f32; self.feat_mu.len()];
        let nc = n_cards();
        for &(idx, count, upgraded) in &ctx.deck {
            let i = idx as usize;
            if i < nc {
                x[2 * i] = count;
                x[2 * i + 1] = upgraded;
            }
        }
        let base = 2 * nc;
        let nr = n_relics();
        for &(idx, count) in &ctx.relics {
            let i = idx as usize;
            if i < nr {
                x[base + i] = count;
            }
        }
        let sbase = base + nr;
        for (i, &s) in ctx.scalars.iter().enumerate().take(n_scalars()) {
            x[sbase + i] = s;
        }
        x
    }

    /// `V(run_state)` in act units (de-normalized). Standardize → one
    /// hidden ReLU layer → linear out → ·y_sd + y_mu.
    #[must_use]
    pub fn value(&self, ctx: &ContextFeatures) -> f64 {
        let x = self.densify(ctx);
        let h_dim = self.hidden;
        let mut h = self.b1.to_vec();
        for (i, (&xi, (&mu, &sd))) in x
            .iter()
            .zip(self.feat_mu.iter().zip(self.feat_sd.iter()))
            .enumerate()
        {
            let xn = (xi - mu) / sd;
            if xn == 0.0 {
                continue;
            }
            let row = &self.w1[i * h_dim..(i + 1) * h_dim];
            for (o, wv) in h.iter_mut().zip(row) {
                *o += xn * wv;
            }
        }
        let mut out = self.b2;
        for (hv, &wv) in h.iter().zip(self.w2.iter()) {
            if *hv > 0.0 {
                out += hv * wv;
            }
        }
        f64::from(out) * f64::from(self.y_sd) + f64::from(self.y_mu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_net_json(schema: &str, hidden_b1: [f32; 2], w2: [f32; 2], b2: f32) -> String {
        let cdim = context_dim();
        serde_json::json!({
            "schema_id": schema,
            "context_dim": cdim,
            "hidden": 2,
            "feat_mu": vec![0.0f32; cdim],
            "feat_sd": vec![1.0f32; cdim],
            "y_mu": 0.0,
            "y_sd": 1.0,
            "w1": vec![vec![0.0f32; 2]; cdim],
            "b1": hidden_b1,
            "w2": w2,
            "b2": b2,
        })
        .to_string()
    }

    fn empty_ctx() -> ContextFeatures {
        serde_json::from_value(serde_json::json!({
            "deck": [],
            "relics": [],
            "scalars": vec![0.0f32; n_scalars()],
        }))
        .expect("context deserializes")
    }

    /// Forward math: empty context → standardized input is all-zero → the
    /// hidden layer is `relu(b1)`, so `V = (b2 + Σ relu(b1)·w2)·y_sd +
    /// y_mu`. b1=[1,-1], w2=[2,3], b2=0.5 ⇒ relu=[1,0] ⇒ 0.5+2 = 2.5.
    #[test]
    fn forward_math_relu_and_bias() {
        let json = tiny_net_json(
            ranker_features::feature_schema_id(),
            [1.0, -1.0],
            [2.0, 3.0],
            0.5,
        );
        let net = ValueNet::from_json_slice(json.as_bytes()).expect("loads");
        let v = net.value(&empty_ctx());
        assert!((v - 2.5).abs() < 1e-5, "expected 2.5, got {v}");
    }

    /// A weight file trained against a different feature schema must
    /// refuse to load (a mismatched index space scores garbage).
    #[test]
    fn schema_mismatch_refuses() {
        let json = tiny_net_json(
            "sts2-runai-ranker-v0:0000000000000000",
            [1.0, 1.0],
            [1.0, 1.0],
            0.0,
        );
        let err = ValueNet::from_json_slice(json.as_bytes()).expect_err("must refuse");
        assert!(matches!(err, ValueError::SchemaMismatch { .. }), "{err}");
    }
}
