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

    #[arg(long)]
    pub json: bool,

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

    #[arg(long, default_value_t = logdelta::drain::DEFAULT_SIMILARITY_THRESHOLD)]
    pub threshold: f64,

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
