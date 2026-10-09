use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "logdelta",
    version,
    about = "Diff logs by meaning, not by bytes. See what's new in the failing run.",
    propagate_version = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Compare one or more baseline (passing) logs against a target (e.g. failing) log.
    Diff(DiffArgs),
    /// List the top log templates found in a file, with counts and an example line.
    Templates(TemplatesArgs),
    /// Stream a target log and print only lines whose template never appears in the baselines.
    Novel(NovelArgs),
}

#[derive(Parser, Debug)]
pub struct DiffArgs {
    /// Baseline log file(s). With no --target, the LAST file here is used as the target
    /// instead (so `logdelta diff good.log bad.log` works).
    #[arg(required = true, num_args = 1..)]
    pub inputs: Vec<String>,

    /// The run to compare against the baseline(s). If omitted, the last positional
    /// argument in `inputs` is used as the target and the rest are baselines.
    #[arg(long)]
    pub target: Option<String>,

    /// Lines of context to show before/after each finding's first matching line.
    /// Requires the target to be a real, re-readable file (not stdin).
    #[arg(short = 'C', long, default_value_t = 0)]
    pub context: usize,

    #[command(flatten)]
    pub common: CommonArgs,

    /// Drain similarity threshold in (0, 1]; lower merges more lines into one template.
    #[arg(long, default_value_t = logdelta::drain::DEFAULT_SIMILARITY_THRESHOLD)]
    pub threshold: f64,

    /// G-test significance cutoff for a CHANGED finding (see README for the formula).
    #[arg(long, default_value_t = logdelta::scoring::DEFAULT_SIGNIFICANCE)]
    pub significance: f64,

    /// Compare exact scalar values at a JSON Pointer before masking (e.g. /status or
    /// /http/response/status_code). Repeatable. Incomplete coverage exits 2.
    #[arg(long = "watch-field", value_name = "POINTER")]
    pub watch_fields: Vec<String>,

    /// Compare watched values within exact JSON groups (e.g. /route). Repeat for a
    /// composite key, up to 4 fields. Missing group keys on watched records exit 2.
    #[arg(long = "watch-by", value_name = "POINTER", requires = "watch_fields")]
    pub watch_by: Vec<String>,

    /// Also compare known field values' rates within each group. Require at least this many
    /// percentage points outside every observed baseline rate, plus --significance. Missing
    /// baseline/target group observations make the requested rate check incomplete (exit 2).
    #[arg(
        long = "watch-rate-change",
        value_name = "PERCENT_POINTS",
        requires = "watch_fields"
    )]
    pub watch_rate_change: Option<f64>,

    /// List every template that differs on its own. By default findings whose lines sit
    /// together (a traceback, the steps a failed job skipped) are reported as one block.
    #[arg(long)]
    pub flat: bool,

    /// The most template representatives of a block to show, prioritizing source
    /// diagnostics and error markers between its start and end (0 shows all).
    #[arg(long, value_name = "N", default_value_t = logdelta::output::DEFAULT_BLOCK_LINES)]
    pub block_lines: usize,

    /// Emit machine-readable JSON instead of colored terminal output.
    #[arg(long)]
    pub json: bool,

    /// Emit a Markdown report, e.g. for `$GITHUB_STEP_SUMMARY` or a PR comment.
    #[arg(long)]
    pub markdown: bool,
}

#[derive(Parser, Debug)]
pub struct TemplatesArgs {
    /// Log file to analyze ("-" for stdin, ".gz" decompressed transparently).
    pub input: String,

    /// Number of top templates to show.
    #[arg(short = 'n', long, default_value_t = 20)]
    pub top: usize,

    #[command(flatten)]
    pub common: CommonArgs,

    /// Drain similarity threshold in (0, 1]; lower merges more lines into one template.
    #[arg(long, default_value_t = logdelta::drain::DEFAULT_SIMILARITY_THRESHOLD)]
    pub threshold: f64,

    /// Emit machine-readable JSON instead of colored terminal output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct NovelArgs {
    /// Known-good baseline log file(s).
    #[arg(long = "baseline", required = true, num_args = 1)]
    pub baselines: Vec<String>,

    /// Target to scan; "-" (the default) reads stdin, so it works with `tail -f`.
    #[arg(default_value = "-")]
    pub target: String,

    #[command(flatten)]
    pub common: CommonArgs,

    /// Drain similarity threshold in (0, 1]; lower merges more lines into one template.
    #[arg(long, default_value_t = logdelta::drain::DEFAULT_SIMILARITY_THRESHOLD)]
    pub threshold: f64,
}

#[derive(Parser, Debug)]
pub struct CommonArgs {
    /// Additional masking regex; matches are replaced with `<CUSTOM>`. Repeatable.
    #[arg(long = "mask", value_name = "REGEX")]
    pub masks: Vec<String>,

    /// File with one `NAME=REGEX` (or bare `REGEX`) mask per line; `#` starts a comment.
    #[arg(long = "mask-file", value_name = "PATH")]
    pub mask_file: Option<String>,

    #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}
