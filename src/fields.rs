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
pub const MAX_GROUP_FIELDS: usize = 4;
pub const MAX_GROUPED_VALUES: usize = 256;
pub const MAX_GROUP_BYTES: usize = 4096;
pub const MAX_POINTER_BYTES: usize = 1024;
pub const MAX_VALUE_BYTES: usize = 4096;
pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
pub const MAX_EXCERPT_BYTES: usize = 4096;
pub const MAX_CONTEXT_BYTES: usize = 8192;
pub const MAX_CONTEXT_LINES_PER_SIDE: usize = 10;

fn excerpt_end(raw: &str, limit: usize) -> usize {
    let mut end = raw.len().min(limit);
    while !raw.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn is_false(value: &bool) -> bool {
    !value
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

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
        let end = excerpt_end(raw, MAX_EXCERPT_BYTES);
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
    /// Selected scalar records that could not be assigned to an exact group.
    #[serde(skip_serializing_if = "is_zero")]
    pub group_missing: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub group_non_scalar: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub group_ambiguous: u64,
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
            && self.group_missing == 0
            && self.group_non_scalar == 0
            && self.group_ambiguous == 0
    }

    fn skip(&mut self, kind: Skip, group: bool, baseline: Option<usize>, line: usize, raw: &str) {
        match (kind, group) {
            (Skip::Missing, true) => self.group_missing += 1,
            (Skip::NonScalar, true) => self.group_non_scalar += 1,
            (Skip::Ambiguous, true) => self.group_ambiguous += 1,
            (Skip::Missing, false) => self.missing += 1,
            (Skip::NonScalar, false) => self.non_scalar += 1,
            (Skip::Ambiguous, false) => self.ambiguous += 1,
            (Skip::NonJson, _) => self.non_json += 1,
            (Skip::InvalidJson, _) => self.invalid_json += 1,
            (Skip::Oversized, _) => self.oversized_records += 1,
        }
        if (group || !matches!(kind, Skip::Missing | Skip::NonJson)) && self.first_problem.is_none()
        {
            self.first_problem = Some(FieldOccurrence::new(baseline, line, raw));
        }
    }
}

#[derive(Serialize)]
pub struct FieldValue {
    /// A JSON scalar encoded as text, including quotes for strings. This avoids losing
    /// large integers to JavaScript's Number, and preserves the spelling of JSON numbers.
    pub value_json: String,
    /// Exact JSON scalars in the order of WatchedField::group_by; empty for pooled watches.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub group_values_json: Vec<String>,
    /// Whether this group had a scalar observation of this watched field in any baseline.
    /// Absent for pooled or incomplete watches. A new group is not a proven value change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_seen_in_baseline: Option<bool>,
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
    #[serde(skip_serializing_if = "is_false")]
    pub context_truncated: bool,
}

impl FieldValue {
    /// Many watched fields can refer to the same large source line. Bound each attached
    /// window as well as the ledger, and retain the closest lines on both sides first.
    pub(crate) fn attach_context(&mut self, source: &ContextWindow) {
        let mut context = ContextWindow {
            before: Vec::new(),
            after: Vec::new(),
        };
        let mut remaining = MAX_CONTEXT_BYTES;
        self.context_truncated = source.before.len() > MAX_CONTEXT_LINES_PER_SIDE
            || source.after.len() > MAX_CONTEXT_LINES_PER_SIDE;
        for distance in 0..MAX_CONTEXT_LINES_PER_SIDE {
            let before = source.before.iter().rev().nth(distance);
            let after = source.after.get(distance);
            for (line, output) in [(before, &mut context.before), (after, &mut context.after)] {
                if let Some((no, raw)) = line {
                    let end = excerpt_end(raw, MAX_EXCERPT_BYTES.min(remaining));
                    self.context_truncated |= end < raw.len();
                    if end > 0 || raw.is_empty() {
                        output.push((*no, raw[..end].to_string()));
                        remaining -= end;
                    }
                }
            }
        }
        context.before.reverse();
        self.context = Some(context);
    }
}

#[derive(Serialize)]
pub struct WatchedField {
    pub pointer: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub group_by: Vec<String>,
    pub complete: bool,
    pub baselines: Vec<FieldCoverage>,
    pub target: FieldCoverage,
    pub values: Vec<FieldValue>,
    /// Optional comparison of known values' rates, with independent coverage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_comparison: Option<crate::field_rates::FieldRateComparison>,
}

impl WatchedField {
    pub fn new_values(&self) -> impl Iterator<Item = &FieldValue> {
        self.values.iter().filter(|v| v.is_new == Some(true))
    }

    /// Values with source evidence used by a novelty or rate finding.
    pub fn finding_values(&self) -> impl Iterator<Item = &FieldValue> {
        let rate_indexes: HashSet<usize> = self
            .rate_comparison
            .iter()
            .flat_map(|rates| &rates.groups)
            .flat_map(|group| &group.changes)
            .map(|change| change.value_index)
            .collect();
        self.values
            .iter()
            .enumerate()
            .filter(move |(index, value)| {
                value.is_new == Some(true) || rate_indexes.contains(index)
            })
            .map(|(_, value)| value)
    }
}

struct FieldState {
    pointer: String,
    tokens: Vec<String>,
    baselines: Vec<FieldCoverage>,
    target: FieldCoverage,
    values: BTreeMap<(Vec<String>, String), FieldValue>,
}

pub(crate) struct FieldTracker {
    fields: Vec<FieldState>,
    group_by: Vec<String>,
    group_tokens: Vec<Vec<String>>,
}

fn selectors(pointers: &[String], flag: &str, limit: usize) -> io::Result<Vec<Vec<String>>> {
    let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidInput, message);
    if pointers.len() > limit {
        return Err(invalid(format!(
            "{flag}: select at most {limit} JSON fields"
        )));
    }
    let mut seen = HashSet::new();
    let mut selectors = Vec::new();
    for pointer in pointers {
        if !pointer.starts_with('/') || pointer.len() > MAX_POINTER_BYTES {
            return Err(invalid(format!("{flag} {pointer:?}: use a JSON Pointer beginning with / (at most {MAX_POINTER_BYTES} bytes)")));
        }
        let mut chars = pointer.chars();
        while let Some(c) = chars.next() {
            if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
                return Err(invalid(format!(
                    "{flag} {pointer:?}: escape ~ as ~0 and / within a key as ~1"
                )));
            }
        }
        if !seen.insert(pointer) {
            return Err(invalid(format!("duplicate {flag} {pointer:?}")));
        }
        selectors.push(
            pointer[1..]
                .split('/')
                .map(|s| s.replace("~1", "/").replace("~0", "~"))
                .collect(),
        );
    }
    Ok(selectors)
}

impl FieldTracker {
    pub fn new(pointers: &[String], group_by: &[String], baselines: usize) -> io::Result<Self> {
        if pointers.is_empty() && !group_by.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--watch-by requires at least one --watch-field",
            ));
        }
        let group_tokens = selectors(group_by, "--watch-by", MAX_GROUP_FIELDS)?;
        let fields = pointers
            .iter()
            .zip(selectors(pointers, "--watch-field", MAX_FIELDS)?)
            .map(|(pointer, tokens)| FieldState {
                pointer: pointer.clone(),
                tokens,
                baselines: (0..baselines).map(|_| FieldCoverage::default()).collect(),
                target: FieldCoverage::default(),
                values: BTreeMap::new(),
            })
            .collect();
        Ok(Self {
            fields,
            group_by: group_by.to_vec(),
            group_tokens,
        })
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
        let selected_group = object.as_ref().map(|object| {
            self.group_tokens
                .iter()
                .map(|tokens| select(object, tokens))
                .collect::<Result<Vec<_>, _>>()
        });
        let value_limit = if self.group_by.is_empty() {
            MAX_VALUES
        } else {
            MAX_GROUPED_VALUES
        };
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
                    coverage.skip(kind, false, baseline, line_no, raw);
                }
                Ok(value) => {
                    // A missing key on an unrelated event does not matter. Once the watched
                    // scalar exists, silently dropping an unassignable group could hide a change.
                    let group = match selected_group
                        .as_ref()
                        .expect("selected from the same object")
                    {
                        Ok(group) => group,
                        Err(kind) => {
                            coverage.skip(*kind, true, baseline, line_no, raw);
                            continue;
                        }
                    };
                    coverage.matched += 1;
                    let key = (group.clone(), value.clone());
                    if value.len() > MAX_VALUE_BYTES
                        || group.iter().map(String::len).sum::<usize>() > MAX_GROUP_BYTES
                        || (!field.values.contains_key(&key) && field.values.len() >= value_limit)
                    {
                        coverage.untracked += 1;
                        if coverage.first_problem.is_none() {
                            coverage.first_problem =
                                Some(FieldOccurrence::new(baseline, line_no, raw));
                        }
                        continue;
                    }
                    let value_count = field.baselines.len();
                    let entry = field.values.entry(key).or_insert_with(|| FieldValue {
                        value_json: value,
                        group_values_json: group.clone(),
                        group_seen_in_baseline: None,
                        baseline_counts: vec![0; value_count],
                        target_count: 0,
                        is_new: None,
                        first_baseline: None,
                        first_target: None,
                        context: None,
                        context_truncated: false,
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
                let baseline_groups: HashSet<_> = field
                    .values
                    .values()
                    .filter(|value| value.baseline_counts.iter().any(|&n| n > 0))
                    .map(|value| value.group_values_json.clone())
                    .collect();
                let values = field
                    .values
                    .into_values()
                    .map(|mut value| {
                        value.is_new = complete.then(|| {
                            value.target_count > 0 && value.baseline_counts.iter().all(|&n| n == 0)
                        });
                        value.group_seen_in_baseline = (complete && !self.group_by.is_empty())
                            .then(|| baseline_groups.contains(&value.group_values_json));
                        value
                    })
                    .collect();
                WatchedField {
                    pointer: field.pointer,
                    group_by: self.group_by.clone(),
                    complete,
                    baselines: field.baselines,
                    target: field.target,
                    values,
                    rate_comparison: None,
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
