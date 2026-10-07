//! Explicit JSON field watches, before masking or template mining. These are observations
//! of new scalar values, not a statistical test or an inference about an error's cause.
//! Memory is bounded per field; incomplete coverage never becomes a clean comparison.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::io;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;

use crate::context::ContextWindow;
use crate::mask::{strip_escapes, strip_structural_prefix};

pub const MAX_FIELDS: usize = 16;
pub const MAX_VALUES: usize = 64;
pub const MAX_POINTER_BYTES: usize = 1024;
pub const MAX_VALUE_BYTES: usize = 4096;
pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
pub const MAX_EXCERPT_BYTES: usize = 4096;

/// A source location. Excerpts can be clipped; the value itself is never clipped.
#[derive(Serialize)]
pub struct FieldOccurrence {
    /// Zero-based baseline index; absent for the target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_index: Option<usize>,
    pub line_no: usize,
    pub raw: String,
    pub truncated: bool,
}

impl FieldOccurrence {
    fn new(baseline_index: Option<usize>, line_no: usize, raw: &str) -> Self {
        let mut end = raw.len().min(MAX_EXCERPT_BYTES);
        while !raw.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            baseline_index,
            line_no,
            raw: raw[..end].into(),
            truncated: end < raw.len(),
        }
    }
}

#[derive(Default, Serialize)]
pub struct FieldCoverage {
    pub lines: u64,
    pub matched: u64,
    pub missing: u64,
    pub non_json: u64,
    pub invalid_json: u64,
    pub non_scalar: u64,
    pub ambiguous: u64,
    pub oversized_records: u64,
    pub untracked: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_problem: Option<FieldOccurrence>,
}

impl FieldCoverage {
    fn complete(&self) -> bool {
        self.matched > 0
            && self.invalid_json == 0
            && self.non_scalar == 0
            && self.ambiguous == 0
            && self.oversized_records == 0
            && self.untracked == 0
    }
}

#[derive(Serialize)]
pub struct FieldValue {
    /// A JSON scalar encoded as text, including quotes for strings. This avoids losing
    /// large integers to JavaScript's Number, and preserves the spelling of JSON numbers.
    pub value_json: String,
    pub baseline_counts: Vec<u64>,
    pub target_count: u64,
    /// Unknown when the field comparison is incomplete.
    pub is_new: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_baseline: Option<FieldOccurrence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_target: Option<FieldOccurrence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextWindow>,
}

#[derive(Serialize)]
pub struct WatchedField {
    pub pointer: String,
    pub complete: bool,
    pub baselines: Vec<FieldCoverage>,
    pub target: FieldCoverage,
    pub values: Vec<FieldValue>,
}

impl WatchedField {
    pub fn new_values(&self) -> impl Iterator<Item = &FieldValue> {
        self.values.iter().filter(|v| v.is_new == Some(true))
    }
}

struct FieldState {
    pointer: String,
    tokens: Vec<String>,
    baselines: Vec<FieldCoverage>,
    target: FieldCoverage,
    values: BTreeMap<String, FieldValue>,
}

pub(crate) struct FieldTracker {
    fields: Vec<FieldState>,
}

impl FieldTracker {
    pub fn new(pointers: &[String], baselines: usize) -> io::Result<Self> {
        let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidInput, message);
        if pointers.len() > MAX_FIELDS {
            return Err(invalid(format!("watch at most {MAX_FIELDS} JSON fields")));
        }
        let mut seen = HashSet::new();
        let mut fields = Vec::new();
        for pointer in pointers {
            if !pointer.starts_with('/') || pointer.len() > MAX_POINTER_BYTES {
                return Err(invalid(format!("--watch-field {pointer:?}: use a JSON Pointer beginning with / (at most {MAX_POINTER_BYTES} bytes)")));
            }
            let mut chars = pointer.chars();
            while let Some(c) = chars.next() {
                if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
                    return Err(invalid(format!(
                        "--watch-field {pointer:?}: escape ~ as ~0 and / within a key as ~1"
                    )));
                }
            }
            if !seen.insert(pointer) {
                return Err(invalid(format!("duplicate --watch-field {pointer:?}")));
            }
            let tokens = pointer[1..]
                .split('/')
                .map(|s| s.replace("~1", "/").replace("~0", "~"))
                .collect();
            fields.push(FieldState {
                pointer: pointer.clone(),
                tokens,
                baselines: (0..baselines).map(|_| FieldCoverage::default()).collect(),
                target: FieldCoverage::default(),
                values: BTreeMap::new(),
            });
        }
        Ok(Self { fields })
    }

    pub fn record(&mut self, baseline: Option<usize>, line_no: usize, raw: &str) {
        if self.fields.is_empty() {
            return;
        }
        let payload = payload(raw);
        let object = payload
            .as_ref()
            .ok()
            .and_then(|text| serde_json::from_str::<Object<'_>>(text).ok());
        for field in &mut self.fields {
            let coverage = match baseline {
                Some(i) => &mut field.baselines[i],
                None => &mut field.target,
            };
            coverage.lines += 1;
            let found = match (&payload, &object) {
                (Err(kind), _) => Err(*kind),
                (Ok(_), Some(object)) => select(object, &field.tokens),
                _ => Err(Skip::InvalidJson),
            };
            match found {
                Err(kind) => {
                    match kind {
                        Skip::Missing => coverage.missing += 1,
                        Skip::NonJson => coverage.non_json += 1,
                        Skip::InvalidJson => coverage.invalid_json += 1,
                        Skip::NonScalar => coverage.non_scalar += 1,
                        Skip::Ambiguous => coverage.ambiguous += 1,
                        Skip::Oversized => coverage.oversized_records += 1,
                    }
                    if !matches!(kind, Skip::Missing | Skip::NonJson)
                        && coverage.first_problem.is_none()
                    {
                        coverage.first_problem = Some(FieldOccurrence::new(baseline, line_no, raw));
                    }
                }
                Ok(value) => {
                    coverage.matched += 1;
                    if value.len() > MAX_VALUE_BYTES
                        || (!field.values.contains_key(&value) && field.values.len() >= MAX_VALUES)
                    {
                        coverage.untracked += 1;
                        if coverage.first_problem.is_none() {
                            coverage.first_problem =
                                Some(FieldOccurrence::new(baseline, line_no, raw));
                        }
                        continue;
                    }
                    let value_count = field.baselines.len();
                    let entry = field
                        .values
                        .entry(value.clone())
                        .or_insert_with(|| FieldValue {
                            value_json: value,
                            baseline_counts: vec![0; value_count],
                            target_count: 0,
                            is_new: None,
                            first_baseline: None,
                            first_target: None,
                            context: None,
                        });
                    if let Some(index) = baseline {
                        entry.baseline_counts[index] += 1;
                        if entry.first_baseline.is_none() {
                            entry.first_baseline =
                                Some(FieldOccurrence::new(baseline, line_no, raw));
                        }
                    } else {
                        entry.target_count += 1;
                        if entry.first_target.is_none() {
                            entry.first_target = Some(FieldOccurrence::new(None, line_no, raw));
                        }
                    }
                }
            }
        }
    }

    pub fn finish(self) -> Vec<WatchedField> {
        self.fields
            .into_iter()
            .map(|field| {
                let complete = !field.baselines.is_empty()
                    && field.baselines.iter().all(FieldCoverage::complete)
                    && field.target.complete();
                let values = field
                    .values
                    .into_values()
                    .map(|mut value| {
                        value.is_new = complete.then(|| {
                            value.target_count > 0 && value.baseline_counts.iter().all(|&n| n == 0)
                        });
                        value
                    })
                    .collect();
                WatchedField {
                    pointer: field.pointer,
                    complete,
                    baselines: field.baselines,
                    target: field.target,
                    values,
                }
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
enum Skip {
    Missing,
    NonJson,
    InvalidJson,
    NonScalar,
    Ambiguous,
    Oversized,
}

/// Keep member names and borrowed raw values, including duplicate names. A normal JSON
/// map silently keeps the last duplicate, but RFC 6901 says an ambiguous lookup fails.
struct Object<'a>(Vec<(String, &'a RawValue)>);

impl<'de> Deserialize<'de> for Object<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = Object<'de>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut fields = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    fields.push(entry);
                }
                Ok(Object(fields))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

impl<'a> Object<'a> {
    fn get(&self, name: &str) -> Result<Option<&'a RawValue>, Skip> {
        let mut found = self.0.iter().filter(|(key, _)| key == name);
        let first = found.next().map(|(_, value)| *value);
        if found.next().is_some() {
            Err(Skip::Ambiguous)
        } else {
            Ok(first)
        }
    }
}

fn payload(raw: &str) -> Result<String, Skip> {
    if raw.len() > MAX_RECORD_BYTES {
        return Err(Skip::Oversized);
    }
    let clean = strip_escapes(raw);
    let (_, text) = strip_structural_prefix(&clean);
    if !text.trim_start().starts_with('{') {
        return Err(Skip::NonJson);
    }
    let object: Object<'_> = serde_json::from_str(&text).map_err(|_| Skip::InvalidJson)?;
    // Match the existing Docker-envelope heuristic, but reject ambiguous envelope keys.
    let log = object.get("log")?;
    if let Some(log) = log.filter(|v| v.get().starts_with('"')) {
        if object.get("stream")?.is_some() || object.get("time")?.is_some() {
            let inner: String = serde_json::from_str(log.get()).map_err(|_| Skip::InvalidJson)?;
            let (_, inner) = strip_structural_prefix(&inner);
            if !inner.trim_start().starts_with('{') {
                return Err(Skip::NonJson);
            }
            return Ok(inner);
        }
    }
    Ok(text)
}

fn select(root: &Object<'_>, tokens: &[String]) -> Result<String, Skip> {
    let mut value = root.get(&tokens[0])?.ok_or(Skip::Missing)?;
    for token in &tokens[1..] {
        value = match value.get().as_bytes().first() {
            Some(b'{') => {
                let object: Object<'_> =
                    serde_json::from_str(value.get()).map_err(|_| Skip::InvalidJson)?;
                object.get(token)?.ok_or(Skip::Missing)?
            }
            Some(b'[') => {
                if token.is_empty()
                    || (token.len() > 1 && token.starts_with('0'))
                    || !token.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(Skip::Missing);
                }
                let index: usize = token.parse().map_err(|_| Skip::Missing)?;
                let array: Vec<&RawValue> =
                    serde_json::from_str(value.get()).map_err(|_| Skip::InvalidJson)?;
                *array.get(index).ok_or(Skip::Missing)?
            }
            _ => return Err(Skip::Missing),
        };
    }
    match value.get().as_bytes().first() {
        Some(b'{' | b'[') => Err(Skip::NonScalar),
        Some(b'"') => {
            let decoded: String =
                serde_json::from_str(value.get()).map_err(|_| Skip::InvalidJson)?;
            Ok(serde_json::to_string(&decoded).expect("a string serializes"))
        }
        _ => Ok(value.get().to_string()),
    }
}
