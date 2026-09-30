//! A plain C-ABI wrapper around the logdelta library for the browser demo, so the page runs
//! the real masking, template mining and diff code rather than a JavaScript re-creation of it.
//! No wasm-bindgen: requests and results cross the boundary as JSON in linear memory.
//!
//! Protocol: the page calls `ld_alloc(len)`, writes a UTF-8 JSON request into that buffer,
//! calls `ld_diff` or `ld_templates` with the pointer and length (which takes ownership of the
//! buffer), and reads the UTF-8 JSON result from `ld_result_ptr()` / `ld_result_len()`. The
//! return value is 0 on success and 1 when the result is `{"error": "..."}`.

use std::alloc::{alloc, dealloc, Layout};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io;

use logdelta::analysis::{diff_lines, mine_lines, DiffOptions};
use logdelta::context::collect_context_from_lines;
use logdelta::drain::DEFAULT_SIMILARITY_THRESHOLD;
use logdelta::mask::CustomMask;
use logdelta::scoring::DEFAULT_SIGNIFICANCE;
use serde::Deserialize;

thread_local! {
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[derive(Deserialize)]
struct DiffRequest {
    baselines: Vec<String>,
    target: String,
    #[serde(default)]
    context: usize,
    #[serde(default)]
    masks: Vec<String>,
    threshold: Option<f64>,
    significance: Option<f64>,
}

#[derive(Deserialize)]
struct TemplatesRequest {
    text: String,
    #[serde(default)]
    masks: Vec<String>,
    threshold: Option<f64>,
}

fn lines(text: &str) -> impl Iterator<Item = io::Result<String>> + '_ {
    text.lines().map(|l| Ok(l.to_string()))
}

fn masks(patterns: &[String]) -> Result<Vec<CustomMask>, String> {
    patterns
        .iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| CustomMask::from_config_line(p.trim()).map_err(|e| format!("mask {p:?}: {e}")))
        .collect()
}

fn diff(request: &[u8]) -> Result<String, String> {
    let req: DiffRequest =
        serde_json::from_slice(request).map_err(|e| format!("bad request: {e}"))?;
    if req.baselines.is_empty() {
        return Err("at least one baseline log is required".into());
    }
    let custom = masks(&req.masks)?;
    let opts = DiffOptions {
        threshold: req.threshold.unwrap_or(DEFAULT_SIMILARITY_THRESHOLD),
        significance: req.significance.unwrap_or(DEFAULT_SIGNIFICANCE),
    };
    let baselines: Vec<_> = req.baselines.iter().map(|b| lines(b)).collect();
    let mut result =
        diff_lines(baselines, lines(&req.target), &custom, &opts).map_err(|e| e.to_string())?;
    if req.context > 0 {
        let wanted: BTreeSet<usize> = result.wanted_line_numbers();
        let ctx = collect_context_from_lines(lines(&req.target), &wanted, req.context)
            .map_err(|e| e.to_string())?;
        result.attach_context(&ctx);
    }
    serde_json::to_string(&result).map_err(|e| e.to_string())
}

fn templates(request: &[u8]) -> Result<String, String> {
    let req: TemplatesRequest =
        serde_json::from_slice(request).map_err(|e| format!("bad request: {e}"))?;
    let custom = masks(&req.masks)?;
    let summary = mine_lines(
        lines(&req.text),
        &custom,
        req.threshold.unwrap_or(DEFAULT_SIMILARITY_THRESHOLD),
    )
    .map_err(|e| e.to_string())?;
    serde_json::to_string(&summary).map_err(|e| e.to_string())
}

fn respond(outcome: Result<String, String>) -> i32 {
    let (code, body) = match outcome {
        Ok(json) => (0, json),
        Err(message) => (1, serde_json::json!({ "error": message }).to_string()),
    };
    RESULT.with(|r| *r.borrow_mut() = body.into_bytes());
    code
}

/// Takes back ownership of a buffer from `ld_alloc` and hands its bytes to `f`.
///
/// # Safety
/// `ptr`/`len` must come from one `ld_alloc(len)` call, not yet freed.
unsafe fn with_request(ptr: *mut u8, len: usize, f: fn(&[u8]) -> Result<String, String>) -> i32 {
    let bytes = if len == 0 {
        Vec::new()
    } else {
        Vec::from_raw_parts(ptr, len, len)
    };
    respond(f(&bytes))
}

#[no_mangle]
pub extern "C" fn ld_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::NonNull::dangling().as_ptr();
    }
    // Layout::array::<u8> never fails for a length the page could have allocated memory for.
    unsafe { alloc(Layout::array::<u8>(len).expect("request size")) }
}

/// # Safety
/// `ptr`/`len` must come from `ld_alloc(len)` and not have been passed to `ld_diff`/`ld_templates`.
#[no_mangle]
pub unsafe extern "C" fn ld_free(ptr: *mut u8, len: usize) {
    if len != 0 {
        dealloc(ptr, Layout::array::<u8>(len).expect("request size"));
    }
}

/// # Safety
/// See the module docs: `ptr`/`len` must come from `ld_alloc(len)`.
#[no_mangle]
pub unsafe extern "C" fn ld_diff(ptr: *mut u8, len: usize) -> i32 {
    with_request(ptr, len, diff)
}

/// # Safety
/// See the module docs: `ptr`/`len` must come from `ld_alloc(len)`.
#[no_mangle]
pub unsafe extern "C" fn ld_templates(ptr: *mut u8, len: usize) -> i32 {
    with_request(ptr, len, templates)
}

#[no_mangle]
pub extern "C" fn ld_result_ptr() -> *const u8 {
    RESULT.with(|r| r.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn ld_result_len() -> usize {
    RESULT.with(|r| r.borrow().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_reports_a_new_line_and_rejects_bad_input() {
        let request = serde_json::json!({
            "baselines": ["start 1\nok 2\nok 3\n"],
            "target": "start 1\nok 2\nboom: disk full\n",
            "context": 1
        });
        let out: serde_json::Value =
            serde_json::from_str(&diff(request.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(out["target_total"], 3);
        assert!(out["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "new" && f["first_target_raw"] == "boom: disk full"));
        assert!(diff(br#"{"baselines": [], "target": "x"}"#)
            .unwrap_err()
            .contains("baseline"));
        assert!(
            diff(br#"{"baselines": ["a"], "target": "b", "masks": ["("]}"#)
                .unwrap_err()
                .starts_with("mask")
        );
    }
}
