mod cli;

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use clap::Parser;

use logdelta::analysis::{diff_runs, mine_run, DiffOptions};
use logdelta::context::collect_context;
use logdelta::io::{open_source, read_line_from};
use logdelta::mask::{mask_line, CustomMask};
use logdelta::output::{human, json, markdown};

use cli::{Cli, ColorChoice, Command, CommonArgs, DiffArgs, NovelArgs, TemplatesArgs};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Diff(args) => run_diff(args),
        Command::Templates(args) => run_templates(args),
        Command::Novel(args) => run_novel(args),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("logdelta: {e}");
            ExitCode::FAILURE
        }
    }
}

fn resolve_color(choice: ColorChoice) -> bool {
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => io::stdout().is_terminal(),
    }
}

fn load_masks(common: &CommonArgs) -> anyhow::Result<Vec<CustomMask>> {
    let mut masks = Vec::new();
    for pattern in &common.masks {
        masks.push(
            CustomMask::from_cli(pattern)
                .map_err(|e| anyhow::anyhow!("--mask {pattern:?}: {e}"))?,
        );
    }
    if let Some(path) = &common.mask_file {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("--mask-file {path:?}: {e}"))?;
        for (i, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            masks.push(
                CustomMask::from_config_line(line)
                    .map_err(|e| anyhow::anyhow!("{path}:{}: {e}", i + 1))?,
            );
        }
    }
    Ok(masks)
}

fn run_diff(args: DiffArgs) -> anyhow::Result<ExitCode> {
    if args.json && args.markdown {
        anyhow::bail!("--json and --markdown are mutually exclusive");
    }
    let (baselines, target) = match &args.target {
        Some(t) => (args.inputs.clone(), t.clone()),
        None => {
            if args.inputs.len() < 2 {
                anyhow::bail!(
                    "diff needs at least 2 files (baseline + target), or one baseline plus --target"
                );
            }
            let mut inputs = args.inputs.clone();
            let target = inputs.pop().unwrap();
            (inputs, target)
        }
    };
    let baseline_refs: Vec<&str> = baselines.iter().map(String::as_str).collect();

    let masks = load_masks(&args.common)?;
    let opts = DiffOptions {
        threshold: args.threshold,
        significance: args.significance,
    };
    let mut result = diff_runs(&baseline_refs, &target, &masks, &opts)?;

    if args.context > 0 && target != "-" {
        let wanted = result.wanted_line_numbers();
        let ctx = collect_context(&target, &wanted, args.context)?;
        result.attach_context(&ctx);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    if args.json {
        json::write_diff(&mut out, &result)?;
    } else if args.markdown {
        markdown::write_diff(&mut out, &result, &baseline_refs, &target)?;
    } else {
        let use_color = resolve_color(args.common.color);
        human::write_diff(
            &mut out,
            &result,
            &baseline_refs,
            &target,
            args.context,
            use_color,
        )?;
    }

    let has_new_or_gone = result
        .findings
        .iter()
        .any(|f| f.kind != logdelta::analysis::FindingKind::Changed)
        || !result.findings.is_empty();
    Ok(if has_new_or_gone {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn run_templates(args: TemplatesArgs) -> anyhow::Result<ExitCode> {
    let masks = load_masks(&args.common)?;
    let summary = mine_run(&args.input, &masks, args.threshold)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    if args.json {
        json::write_templates(&mut out, &summary)?;
    } else {
        let use_color = resolve_color(args.common.color);
        human::write_templates(&mut out, &summary, args.top, use_color)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn run_novel(args: NovelArgs) -> anyhow::Result<ExitCode> {
    let masks = load_masks(&args.common)?;
    let mut drain = logdelta::drain::Drain::new(args.threshold);
    for path in &args.baselines {
        for (i, line) in logdelta::io::read_lines(path)?.enumerate() {
            let line = line?;
            let masked = mask_line(&line, &masks);
            drain.add_line(&masked, i + 1, &line);
        }
    }

    let mut src = open_source(&args.target)?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut buf = Vec::new();
    let mut found_any = false;
    loop {
        match read_line_from(&mut src, &mut buf)? {
            None => break,
            Some(line) => {
                let masked = mask_line(&line, &masks);
                let tokens: Vec<String> = masked.split_whitespace().map(str::to_owned).collect();
                if !drain.contains_matching_template(&tokens) {
                    found_any = true;
                    writeln!(out, "{line}")?;
                    out.flush()?;
                }
            }
        }
    }
    Ok(if found_any {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
