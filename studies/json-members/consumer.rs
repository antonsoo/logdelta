//! A separate consumer of the actual source or installed crate's public APIs.
use logdelta::analysis::{diff_lines, DiffOptions};
use logdelta::mask::tokenize_line;
use serde_json::{json, Value};

fn main() {
    let path = std::env::args().nth(1).expect("case file argument");
    let cases: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let rows: Vec<_> = cases
        .iter()
        .map(|case| {
            let baseline = case["baseline"].as_str().unwrap();
            let target = case["target"].as_str().unwrap();
            let diff = diff_lines(
                vec![(0..12).map(|_| Ok(baseline.to_owned()))],
                (0..12).map(|_| Ok(target.to_owned())),
                &[],
                &DiffOptions::default(),
            )
            .unwrap();
            json!({"id":case["id"],"baselineTokens":tokenize_line(baseline,&[]),
            "targetTokens":tokenize_line(target,&[]),"diff":diff})
        })
        .collect();
    println!("{}", serde_json::to_string(&rows).unwrap());
}
