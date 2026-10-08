# logdelta's template miner on Loghub, the log-parsing benchmark

logdelta turns each log line into a template and compares templates between
runs. Which lines it puts together decides everything after that: lines of one
kind split into many templates are noise in every diff, and two kinds merged
into one hide the new line among the old.

Loghub-2k is the benchmark log parsers are scored on: 2,000 lines from each of
16 real systems (HDFS, Hadoop, Spark, Linux, Android, Apache, OpenSSH and nine
more), with the template of every line labelled by hand. logdelta had never
been run on it.

**What came out**

- **The released 0.3.4 scored 0.73, and 0.43 on the lines as the systems wrote
  them.** The reference implementation of Drain, the algorithm logdelta's miner
  is built on, publishes 0.865 on the same data.
- **Three causes, none of them a matter of tuning.** A line was filed under its
  first token, and in too many logs the first token is a value. A wildcard
  counted as agreement, so a template got easier to join with every line it
  absorbed, until one of them held 343 Android lines of 27 different templates.
  And the timestamp of Apache's error log, the most common one there is, was
  not masked: `[Sun` and `[Mon` were two different first tokens.
- **Reworked, it scores 0.82 and 0.71, with one configuration for all sixteen
  systems.** The reference's 0.865 uses a log format, masking regexes and a
  similarity threshold chosen for each system. On eight of the sixteen logdelta
  now matches or passes it without being told where the header ends.
- **The benchmark's best setting is not the default.** At a threshold of 0.6
  logdelta scores 0.88 and 0.75, above the reference's average. On the task the
  tool exists for, [diffing failed builds against passing ones](../kubernetes-ci/README.md),
  0.6 and 0.5 are within noise of each other, and on this repository's small
  examples 0.6 reports more. The default stays at the paper's 0.5.

## The measure

Grouping accuracy, as the benchmark defines it (Zhu et al., ICSE-SEIP 2019): a
line is correctly parsed when the set of lines it was grouped with is exactly
the set of lines that share its labelled template. It is unforgiving on
purpose. Merge one stray line into a template of 608 and all 609 are wrong.

Each system is scored twice, because logdelta is used differently from the way
the benchmark feeds a parser:

- **Message**: the `Content` column, what is left after the benchmark cuts
  each line's header off with a per-system format string. Every published
  number is on this.
- **Whole line**: the line as the system wrote it, timestamp, host, process,
  level and all. This is what logdelta gets. The labels still describe the
  message only, so a parser that tells `node status` from `partition status` in
  the header is marked wrong for it; read this column as a lower bound.

## Results

| System | Templates | Message: 0.3.4 | now | Drain, tuned | Whole line: 0.3.4 | now |
|---|---:|---:|---:|---:|---:|---:|
| HDFS | 14 | 0.998 | 0.998 | 0.998 | 0.930 | 0.998 |
| Hadoop | 114 | 0.960 | 0.962 | 0.948 | 0.444 | 0.932 |
| Spark | 36 | 0.907 | 0.922 | 0.920 | 0.920 | 0.920 |
| Zookeeper | 50 | 0.967 | 0.967 | 0.967 | 0.955 | 0.960 |
| OpenStack | 43 | 0.288 | 0.881 | 0.733 | 0.117 | 0.232 |
| BGL | 120 | 0.794 | 0.951 | 0.963 | 0.227 | 0.744 |
| HPC | 46 | 0.741 | 0.889 | 0.887 | 0.062 | 0.433 |
| Thunderbird | 149 | 0.944 | 0.953 | 0.955 | 0.545 | 0.941 |
| Windows | 50 | 0.565 | 0.692 | 0.997 | 0.562 | 0.567 |
| Linux | 118 | 0.685 | 0.686 | 0.690 | 0.658 | 0.677 |
| Mac | 341 | 0.722 | 0.744 | 0.786 | 0.599 | 0.689 |
| Android | 166 | 0.706 | 0.753 | 0.911 | 0.291 | 0.535 |
| HealthApp | 75 | 0.712 | 0.900 | 0.780 | 0.015 | 0.971 |
| Apache | 6 | 1.000 | 1.000 | 1.000 | 0.000 | 1.000 |
| OpenSSH | 27 | 0.718 | 0.718 | 0.787 | 0.526 | 0.718 |
| Proxifier | 8 | 0.000 | 0.025 | 0.526 | 0.002 | 0.000 |
| **Average** | | **0.732** | **0.815** | **0.865** | **0.428** | **0.707** |

"Drain, tuned" is the accuracy column of the reference implementation's own
benchmark table (`logparser/Drain/README.md` at logpai/logparser `d9d4180`), run
with the settings in its `benchmark.py`: for each system a log format, a list of
regular expressions for that system's values, a tree depth and a similarity
threshold between 0.2 and 0.7. logdelta's two columns use its defaults.

The 16 samples hold 1,363 labelled templates. 0.3.4 found 1,934 in the messages
and 2,291 in the whole lines; it now finds 1,269 and 1,228.

No system got worse on the message. On the whole line one did, Proxifier, from
3 correct lines in 2,000 to none.

## What was wrong

**The first token is often a value.** The paper's Drain files a line under its
length and its first tokens, with a token that has a digit in it treated as a
wildcard. logdelta kept the length and the first token and dropped the digit
rule, on the grounds that masking had already replaced the values. It had not:

```
proxy.cse.cuhk.edu.hk:5070 open through proxy proxy.cse.cuhk.edu.hk:5070 HTTPS     Proxifier
attempt_1445144423722_0020_m_000000_0 TaskAttempt Transitioned from NEW to …        Hadoop
1005 floating point alignment exceptions                                            BGL
20171223-22:15:29:606|Step_LSC|30002312|onStandStepChanged 3579                     HealthApp
```

Each of those was a template of its own, one per host, per attempt, per count.
Even the numbers split three ways: `8` stayed a literal, `1005` became `<NUM>`
and `1365301360`, ten digits starting with 1, became `<TS>`. A line is now
filed under its first *constant word*: the first token with no digit and no
placeholder in it.

**A wildcard counted as agreement.** The paper scores a line against a template
by the share of positions where the two hold the same token, and a position
that has become `<*>` is not one of them. logdelta counted it as a match. Every
line a template absorbed gave it another wildcard and made the next line easier
to absorb:

```
I PhoneStatusBar: suspendAutohide                 joins        I PhoneStatusBar: resumeSuspendedAutohide
V PhoneStatusBar: setLightsOn(true)               then joins   I PhoneStatusBar: <*>
I NotificationManager: cancelNotification,index:-1   then joins   <*> PhoneStatusBar: <*>
```

(Android's logcat, shown without the timestamp and process ids each line starts
with.) After those three steps the template is `<*> <*> <*>` and takes anything
of its length.

A template's evidence is now fixed when it is created: the number of positions
that held a word in its first line. A line joins when it still agrees on half
of *those*, so a template can lose at most half its words to wildcards, as in
the paper.

**Formats nobody had tried.**

- `[Sun Dec 04 04:47:44 2005]`, the C library's `ctime` form, which Apache's
  error log, `date` and `git log` all use; and beside it the Common Log Format
  of every access log, `04/Dec/2005:04:47:44 +0000`, and RFC 2822 dates. None
  was masked. On the Apache sample every line was wrong.
- A thread name in brackets: `[main]`, `[RMCommunicator Allocator]`,
  `[IPC Server handler 14 on 62270]`. Split at spaces, one Hadoop log statement
  had one, two or six tokens there, and a template for each. A short
  square-bracketed field is now one token.
- The same field masked two ways. `22:15:29` matched the duration pattern and
  `22:16:0` did not; a byte count was `<NUM>` until it reached ten digits. Tokens
  are now compared by *shape*, with every number in them written `#`:
  `blk_38865049064139660` and `blk_-7128370237687728475` are both `blk_#`.

## What is left

**Labels that turn on one word.** Windows' `Session: … initialized by client
WindowsUpdateAgent.` has 608 lines and `… by client SPP.` has one; the labels
make them two templates, logdelta makes them one, and all 609 are wrong. The
reference keeps them apart with a threshold of 0.7 chosen for Windows. Linux's
`authentication failure; … user=root`, `user=guest` and `user=test` are three
templates in the labels. OpenSSH's `Received disconnect from <IP>: 11: Bye Bye
[preauth]` and `… 11: disconnected by user` agree on three words of six. For a
diff these are mostly harmless: a value that appears only in the failing run at
such a position is what logdelta's NEW VALUE finding reports.

**Headers.** On whole lines, the words every line shares (`RAS KERNEL INFO`, a
level and a component) count as agreement between two different messages. BGL,
HPC, OpenStack and Android lose most of their whole-line accuracy to this. A
parser that is told the header format does not have the problem; logdelta
would need to work the header out, and does not.

**Proxifier.** `close, 403 bytes sent, 426 bytes received` and `close, 1190
bytes (1.16 KB) sent, 1671 bytes (1.63 KB) received` are one template in the
labels and different lengths in the log. The reference gets 0.53 on it.

## The threshold

Everything above is at the default similarity threshold, 0.5, the paper's. The
same build at 0.6:

| | Message | Whole line |
|---|---:|---:|
| threshold 0.5 (default) | 0.815 | 0.707 |
| threshold 0.6 | 0.879 | 0.749 |
| Drain, tuned per system | 0.865 | |

0.6 wins here because the labels reward splitting on a single word. Whether it
is better for diffing is a separate question, asked in the
[Kubernetes study](../kubernetes-ci/README.md): the failing test is named in
every case at both settings, a passing build diffed against passing ones is
silent equally often, and the mean report is 4.7 findings instead of 5.0. On
this repository's two small examples 0.6 reports one finding more each. That is
not enough to move a default; `--threshold 0.6` is there for a log whose
templates differ by one word.

## What this does not show

- **That the miner is good at logs in general.** Sixteen systems, 2,000 lines
  each, most of them from the 2000s and 2010s. The fixes were made looking at
  these same lines, so the scores after are not a prediction for a seventeenth
  system; the Kubernetes build logs, which were not looked at while the miner
  was changed, are the check on that.
- **Template text.** Grouping accuracy scores which lines are together, not
  what the template says. logdelta shows `<*>` where the labels show
  `blk_<*>`.
- **A like-for-like race with Drain.** The published numbers were not rerun
  here. They are on the message with per-system settings; logdelta's are with
  none.

## Reproduce

```sh
git clone https://github.com/antonsoo/logdelta && cd logdelta
python3 studies/loghub/fetch.py                       # 11 MB from logpai/loghub, pinned
cargo build --release --example cluster_ids
python3 studies/loghub/score.py                       # this checkout
python3 studies/loghub/score.py --threshold 0.6 --label "checkout --threshold 0.6"

# 0.3.4: the same example program built against the released library
git worktree add /tmp/logdelta-0.3.4 v0.3.4
cp bench/cluster_ids.rs /tmp/logdelta-0.3.4/bench/
printf '\n[[example]]\nname = "cluster_ids"\npath = "bench/cluster_ids.rs"\n' >> /tmp/logdelta-0.3.4/Cargo.toml
(cd /tmp/logdelta-0.3.4 && cargo build --release --example cluster_ids)
python3 studies/loghub/score.py --label 0.3.4 \
    --binary /tmp/logdelta-0.3.4/target/release/examples/cluster_ids
```

[`results.json`](results.json) has every score, and for each system the
templates split apart and merged together with example lines.
`bench/cluster_ids.rs` prints the template id logdelta gives each line of a
file; `score.py` compares those ids with the labels.

## References

- Zhu, He, He, Liu and Lyu, "Loghub: A Large Collection of System Log Datasets
  for AI-driven Log Analytics", ISSRE 2023. The data:
  <https://github.com/logpai/loghub>, free for research and academic work with
  attribution. The datasets are not redistributed here; this page and
  `results.json` quote a few lines per system as examples of what was split or
  merged.
- Zhu, He, Liu, He, Xie, Zheng and Lyu, "Tools and Benchmarks for Automated Log
  Parsing", ICSE-SEIP 2019. Grouping accuracy and the benchmark.
- He, Zhu, Zheng and Lyu, "Drain: An Online Log Parsing Approach with Fixed
  Depth Tree", ICWS 2017.
