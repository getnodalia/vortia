//! The envelope a report is saved in: `{schema, version, payload, sha256}`, where the digest is
//! of the payload written with its keys in order. A report that was edited by hand, or cut short,
//! no longer matches its digest and is refused when it is read back to be compared. The digest
//! detects a change; it is neither an approval nor a signature.

use serde_json::{Map, Value, json};

use crate::error::{PostError, Result};
use crate::sha256::sha256_hex;

/// The envelope version this build writes and reads.
pub const VERSION: u64 = 1;

/// The most a report file may hold when it is read back: a report is a few kilobytes.
pub const MAX_REPORT_BYTES: usize = 4 * 1024 * 1024;

/// `payload` written compactly with the keys of every object sorted, so the same payload gives
/// the same digest whatever order it was built in.
fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                if let Some(v) = map.get(key) {
                    canonical(v, out);
                }
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(v, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// The digest of a payload.
pub fn digest(payload: &Value) -> String {
    let mut text = String::new();
    canonical(payload, &mut text);
    sha256_hex(text.as_bytes())
}

/// A report as text: the envelope around `payload`.
pub fn seal(schema: &str, payload: Value) -> String {
    let sha256 = digest(&payload);
    let envelope = json!({
        "schema": schema,
        "version": VERSION,
        "payload": payload,
        "sha256": sha256,
    });
    serde_json::to_string_pretty(&envelope).unwrap_or_default()
}

/// A report read back and found whole.
#[derive(Debug, Clone, PartialEq)]
pub struct Sealed {
    pub schema: String,
    pub sha256: String,
    pub payload: Map<String, Value>,
}

impl Sealed {
    /// The text of a report: its envelope must be there and its digest must match its payload.
    pub fn open(text: &str) -> Result<Self> {
        let bad = |why: &str| PostError::Invalid(format!("not a Vortia report: {why}"));
        if text.len() > MAX_REPORT_BYTES {
            return Err(bad("the file is too large"));
        }
        let value: Value = serde_json::from_str(text).map_err(|e| bad(&e.to_string()))?;
        let schema = value
            .get("schema")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("it has no schema"))?;
        if value.get("version").and_then(Value::as_u64) != Some(VERSION) {
            return Err(bad("its version is not one this build reads"));
        }
        let payload = value
            .get("payload")
            .filter(|p| p.is_object())
            .ok_or_else(|| bad("it has no payload"))?;
        let stored = value
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("it has no digest"))?;
        if digest(payload) != stored {
            return Err(PostError::Invalid(
                "the report's digest does not match its contents: it was changed after it was saved"
                    .into(),
            ));
        }
        let Value::Object(payload) = payload.clone() else {
            return Err(bad("it has no payload"));
        };
        Ok(Self {
            schema: schema.to_owned(),
            sha256: stored.to_owned(),
            payload,
        })
    }

    /// A number of the payload's `metrics`.
    pub fn metric(&self, key: &str) -> Option<f64> {
        self.payload
            .get("metrics")?
            .get(key)?
            .as_f64()
            .filter(|v| v.is_finite())
    }

    /// The names of the payload's `metrics` that are numbers, in the order the report has them.
    pub fn metric_names(&self) -> Vec<String> {
        self.payload
            .get("metrics")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter(|(_, v)| v.as_f64().is_some_and(f64::is_finite))
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A text of the payload.
    pub fn text(&self, key: &str) -> &str {
        self.payload
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }
}
