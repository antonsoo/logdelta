//! Recognize a Docker json-file wrapper without discarding application fields.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DockerEnvelope {
    pub log: String,
    pub stream: Stream,
    #[serde(default, rename = "time")]
    _time: String,
    #[serde(default, rename = "attrs")]
    _attrs: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

pub(crate) fn is_docker_key(key: &str) -> bool {
    matches!(key, "log" | "stream" | "time" | "attrs")
}

/// The stream is required, while time/attrs may be absent in copied wrappers.
/// A `log`/`time` application record, extra application keys, or an unknown
/// stream stays an ordinary JSON object. Content alone cannot prove provenance.
/// An otherwise recognized wrapper with repeated attribute names is ambiguous.
pub(crate) fn docker(text: &str) -> Result<Option<DockerEnvelope>, ()> {
    let Ok(envelope) = serde_json::from_str(text) else {
        return Ok(None);
    };
    // Derived maps would choose the last duplicate attribute. Keep the masker's
    // existing whole-record ambiguity policy, including nested metadata.
    crate::json_value::parse(text).map_err(|_| ())?;
    Ok(Some(envelope))
}
