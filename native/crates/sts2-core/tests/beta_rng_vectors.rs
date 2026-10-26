//! Vectors emitted by the verified v0.111.0 game assembly, without an RNG rewrite.
//! Assembly identity and complete PRNG words are recorded in the fixture.
use serde_json::Value;
use sts2_core::dotnet_rng::{deterministic_hash_code, deterministic_hash_code_old, Rng, RngState};

fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/beta_rng_vectors.json")).unwrap()
}
fn state(value: &Value) -> RngState {
    serde_json::from_value(value.clone()).unwrap()
}

#[test]
fn beta_hashes_include_unicode_and_legacy_utf16() {
    for vector in vectors()["hashes"].as_array().unwrap() {
        let text = vector["text"].as_str().unwrap();
        assert_eq!(
            deterministic_hash_code(text),
            vector["hash"].as_u64().unwrap(),
            "{text}"
        );
        assert_eq!(
            i64::from(deterministic_hash_code_old(text)),
            vector["legacy"].as_i64().unwrap(),
            "{text}"
        );
    }
}

#[test]
fn beta_raw_draws_and_all_serialized_words_match() {
    for vector in vectors()["seqs"].as_array().unwrap() {
        let mut rng = Rng::new(vector["seed"].as_u64().unwrap());
        assert_eq!(rng.state(), state(&vector["start"]));
        for expected in vector["values"].as_array().unwrap() {
            assert_eq!(rng.next_unsigned_long(), expected.as_u64().unwrap());
        }
        assert_eq!(rng.state(), state(&vector["end"]));
        let mut restored = Rng::new(0);
        restored.load_state(state(&vector["end"]));
        assert_eq!(restored.next_unsigned_long(), rng.next_unsigned_long());
    }
}

#[test]
fn beta_mixed_float_gaussian_rejection_and_resume_match() {
    let fixture = vectors();
    let mixed = &fixture["mixed"];
    let mut rng = Rng::new(mixed["seed"].as_u64().unwrap());
    for step in mixed["steps"].as_array().unwrap() {
        let value = match step["op"].as_str().unwrap() {
            "NextInt(17)" => serde_json::json!(rng.next_int_exclusive(0, 17)),
            "NextFloat()" => serde_json::json!((rng.next_float(1.0) as f32).to_bits()),
            "NextDouble(-7,9)" => {
                serde_json::json!(rng.next_double_range(-7.0, 9.0).to_bits() as i64)
            }
            "NextBool()" => serde_json::json!(rng.next_bool()),
            "NextInt(-5,16)" => serde_json::json!(rng.next_int_exclusive(-5, 16)),
            "NextGaussianDouble(.5,.8,-2,3)" => {
                serde_json::json!(rng.next_gaussian_double(0.5, 0.8, -2.0, 3.0).to_bits() as i64)
            }
            "NextGaussianFloat(.5,.3,0,1)" => {
                serde_json::json!(rng.next_gaussian_float(0.5, 0.3, 0.0, 1.0).to_bits())
            }
            "NextGaussianInt(3,2,-4,8)" => {
                serde_json::json!(rng.next_gaussian_int(3.0, 2.0, -4, 8))
            }
            other => panic!("unknown oracle operation {other}"),
        };
        assert_eq!(value, step["value"], "{}", step["op"]);
        assert_eq!(rng.state(), state(&step["state"]), "{}", step["op"]);
        // Every intermediate snapshot must independently continue the sequence.
        let mut restored = Rng::new(0);
        restored.load_state(state(&step["state"]));
        rng = restored;
    }
}
