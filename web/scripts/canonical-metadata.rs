// A scoped rustc wrapper: canonicalize Cargo's crate disambiguator for WASM,
// preserving caller metadata, every other compiler argument and outer wrappers.
use std::{
    collections::{hash_map::DefaultHasher, BTreeSet},
    env,
    ffi::OsString,
    hash::{Hash, Hasher},
    process::{self, Command},
};

fn metadata(args: &[OsString]) -> Vec<(usize, String)> {
    let mut values = Vec::new();
    for (index, argument) in args.iter().enumerate() {
        let text = argument.to_string_lossy();
        let value = text
            .strip_prefix("-Cmetadata=")
            .or_else(|| text.strip_prefix("--codegen=metadata="))
            .or_else(|| {
                (index > 0 && (args[index - 1] == "-C" || args[index - 1] == "--codegen"))
                    .then(|| text.strip_prefix("metadata="))
                    .flatten()
            });
        if let Some(value) = value {
            values.push((index, value.to_owned()));
        }
    }
    values
}

fn value(args: &[OsString], option: &str) -> Option<OsString> {
    for (index, argument) in args.iter().enumerate() {
        if argument == option {
            return args.get(index + 1).cloned();
        }
        if let Some(value) = argument
            .to_string_lossy()
            .strip_prefix(&format!("{option}="))
        {
            return Some(value.into());
        }
    }
    None
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("//?/")
        .to_owned()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.is_empty() {
        return Err("missing compiler command".into());
    }
    let owned = metadata(&args);
    // Probes and host build scripts/proc macros keep Cargo's ordinary metadata.
    if value(&args, "--target").as_deref() == Some("wasm32-unknown-unknown".as_ref())
        && !owned.is_empty()
        && env::var_os("CARGO_MANIFEST_DIR").is_some()
    {
        let manifest = normalize(&env::var("CARGO_MANIFEST_DIR")?);
        let identities = env::var("LOGDELTA_CRATE_IDENTITIES")?;
        let identity = identities
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .find(|(path, _)| normalize(path) == manifest)
            .map(|(_, identity)| identity)
            .ok_or("missing stable identity for compiled crate")?;
        let flags: Vec<OsString> = env::var("LOGDELTA_ORIGINAL_RUSTFLAGS")?
            .split('\x1f')
            .map(OsString::from)
            .collect();
        let caller = metadata(&flags);
        if owned.len() != caller.len() + 1
            || owned
                .iter()
                .skip(1)
                .map(|(_, value)| value)
                .ne(caller.iter().map(|(_, value)| value))
        {
            return Err("cannot distinguish Cargo metadata from caller metadata".into());
        }
        let mut cfg = BTreeSet::new();
        for (index, argument) in args.iter().enumerate() {
            if argument == "--cfg" {
                cfg.insert(args.get(index + 1).ok_or("missing cfg argument")?.clone());
            } else if let Some(value) = argument.to_string_lossy().strip_prefix("--cfg=") {
                cfg.insert(value.into());
            }
        }
        let mut hasher = DefaultHasher::new();
        identity.hash(&mut hasher);
        value(&args, "--crate-name").hash(&mut hasher);
        cfg.hash(&mut hasher);
        let (index, _) = &owned[0];
        let prefix = if args[*index].to_string_lossy().starts_with("-C") {
            "-Cmetadata="
        } else if args[*index].to_string_lossy().starts_with("--codegen=") {
            "--codegen=metadata="
        } else {
            "metadata="
        };
        args[*index] = format!("{prefix}logdelta_{:016x}", hasher.finish()).into();
    }
    let previous = env::var_os("LOGDELTA_PREVIOUS_RUSTC_WRAPPER").filter(|value| !value.is_empty());
    let mut command = if let Some(previous) = previous {
        let mut command = Command::new(previous);
        command.args(&args);
        command
    } else {
        let mut command = Command::new(&args[0]);
        command.args(&args[1..]);
        command
    };
    process::exit(command.status()?.code().unwrap_or(1));
}
