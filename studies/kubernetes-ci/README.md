# 264 failed builds, each against a passing build of the same commit

logdelta exists for one question: what is in the failing run's log that was not
in a good one? This study asks it 264 times on real logs with a known answer.

Kubernetes publishes every build of its CI jobs: the commit, the result, the
JUnit report and the console log. Two periodic jobs build `master` about once
an hour, so one commit is often built several times, and sometimes one of those
builds fails on a flaky test. The failed build and a passing build of the same
commit ran the same code. What differs between their logs is the failure, plus
whatever differs between any two runs; and the JUnit report says which test it
was.

**What came out**

**Excerpt update, 8 October 2026.** The source checkout now favors source diagnostics
and error markers in the existing 12-representative budget. A fresh run of every case
shows the recorded reason in **212 of 239**, up from **159**, in a median of **40.5**
report lines rather than **34**. All 159 previously shown reasons remain visible and
53 more appear. With multiple baselines the change is 141 to 183 of 206, with no lost
cases. The raw native JSON reports are identical for all 264 single-baseline cases.

This rule was developed on these logs. It is an in-sample presentation result, and a
source-location message is not necessarily an assertion: the study's last-message
convention can also select a test's ordinary diagnostic output. The remaining **27**
misses matter: 11 are still outside the abbreviated excerpt, and 16 are absent even
from a report showing every template representative. Source locations can match while
the first representative contains different values from the later failing occurrence.
[Per-case rerun](excerpt-results.json), [behavior and verification](../../docs/diagnostic-excerpts.md).

The miner comparison below records the earlier checkout at `2f8fb65`, before this
excerpt change. Its measurements and baseline results are retained for comparison.

- **Naming the failed test is not the achievement.** The diff names a test
  the JUnit report lists as failed in all 264 cases, with the released 0.3.4
  and with this checkout. So does `grep -- '--- FAIL'`, in two lines.
- **What a diff adds is the reason, and logdelta shows it two times in
  three.** The JUnit report holds each failed test's own assertion or panic
  line. That line is among the lines logdelta prints in 159 of the 239 cases
  that have one (67%; 61% to 72% resampling the 121 commits), in a report of
  34 lines at the median. The grep never shows it. A plain set difference of
  the two logs always does, in 1,275 lines.
- **The rework of the miner did not change that.** 0.3.4 also showed the
  reason in 159 of 239, in 39 lines. The reason is inside a block logdelta
  reports in 223 of 239 cases with both versions; in 64 of them the terminal
  report cuts the block short before the line that matters.
- **Fewer findings, not less to read.** The median report went from 6 findings
  to 4. That is grouping: the checkout's templates are finer, and more of them
  fold into one block. Counted ungrouped, the mean went up, from 63 to 70.
- **A passing build against passing builds is silent in 127 of 179 cases**
  (0.3.4: 122), and has three findings or more in 3 (0.3.4: 11).

The miner was reworked on [the Loghub benchmark](../loghub/README.md), and
these logs are a check on those changes, with one exception that makes them
less than a blind test. Kubernetes logs with klog, whose lines begin `I1005`
for an info line on 5 October. That 0.3.4 filed every such line as new the day
after its baseline was noticed in these logs, and the fix has a test written
from them. The rows below for a baseline of another day measure that fix on
the format it was made for.

## The cases

`collect.py --rescan` lists both jobs' builds in the public `kubernetes-ci-logs`
bucket and keeps every failed build for which

- the build's JUnit report names at least one failed Go test, and
- at least one other build of the same commit passed.

For each it records up to three passing builds of that commit as baselines,
nearest in time, and one more as a control when the commit has a fourth.
[`manifest.json`](manifest.json) is the list, as of 8 October 2026.

| | integration | unit | both |
|---|---:|---:|---:|
| Job | `ci-kubernetes-integration-master` | `ci-kubernetes-unit` | |
| Failed builds in about 91 days | 159 | 167 | 326 |
| Cases: a failed test, and a passing build of the commit | 134 | 130 | 264 |
| with two or three baselines | 118 | 111 | 229 |
| with a control | 97 | 82 | 179 |
| Lines in a failed build's log, median | 2,626 | 2,643 | |

1,023 logs, 518 MB. No Kubernetes code is run.

## What is measured

`evaluate.py` runs `logdelta diff` and reads the report.

- **Names a failed test**: some NEW finding, or NEW VALUE finding, holds the
  name of a test the JUnit report lists as failed, in its line or its template.
- **Shows the reason**: the JUnit report's failure text for a failed test ends
  with what the test said, `plugins_test.go:2247: Didn't expect the first pod
  to be scheduled`, or a panic. The case counts when that line is among the
  lines the default terminal report prints. 239 of the 264 failed builds have
  such a line.
- **Lines to read**: the lines that report prints.
- **Findings**: grouped, as reported (lines that belong together are one
  finding), and ungrouped.
- **Control**: a passing build as the target, the same baselines. Every finding
  there is something that differs between passing runs.
- **Without a log parser**, `evaluate.py --plain`: `grep -- '--- FAIL'` on the
  failed log; a set difference, the failed log's lines that are not in the
  passing one; and a grep for failure markers (`--- FAIL`, `_test.go:<line>:`,
  `panic:`).

The 264 cases come from 121 commits, and failures on one commit are not
independent. Intervals resample commits.

## Results

Failed build against one passing build of the same commit, 264 cases:

| | Names a failed test | Shows the reason, of 239 | Lines to read, median |
|---|---:|---:|---:|
| `grep -- '--- FAIL'` | 264 | 0 | 2 |
| grep for failure markers | 264 | 239 | 258 |
| Set difference of lines | 264 | 239 | 1,275 |
| logdelta 0.3.4 | 264 | 159 (61% to 72%) | 39 |
| logdelta, this checkout | 264 | 159 (61% to 72%) | 34 |
| this checkout, blocks printed whole | 264 | 223 | 58 |

The last row uses the existing `--block-lines 0` option: no representative is omitted
from a block. It does not show every occurrence of a repeated template, and long lines
are still clipped to the terminal width. `--json` retains the full recorded strings.

By job the reason is shown in 98 of 125 unit-test cases (78%) and 61 of 114
integration cases (54%), where one failed test prints hundreds of lines.

| | 0.3.4 | this checkout |
|---|---:|---:|
| **Failed build against one passing build** (264) | | |
| findings: median, mean, 90th percentile | 6, 6.7, 11 | 4, 5.0, 8 |
| ungrouped findings: median, mean | 43, 63.0 | 39.5, 70.4 |
| **Failed build against up to three** (229) | | |
| findings: median, mean, 90th percentile | 5, 6.6, 11 | 4, 4.9, 9 |
| shows the reason, of 206 | 141 | 141 |
| **Passing build against passing builds** (179) | | |
| no findings | 122 | 127 |
| one, two, three or more | 28, 18, 11 | 29, 20, 3 |
| **Failed build against a passing build of another day** (14) | | |
| findings: median | 9 | 4 |
| ungrouped findings: median | 79 | 94 |
| shows the reason, of 12 | 7 | 8 |
| **Templates found in a pair of logs, median** (264) | 103 | 606 |

The grouped count fell in 258 of the 264 cases and the ungrouped count rose in
134. Both follow from the last row. 0.3.4 found about a hundred templates in
two logs of more than 2,000 lines each, because its templates grew: a wildcard
counted as agreement, so `I1005 <DUR> <NUM> <*> <*> <*> <*> <*> <*> <*>` took
in every klog line of ten tokens. This checkout finds about six hundred, one
per log statement, and its blocks hold more of them. "This checkout" is also
more than the miner: it reports values that went missing, which adds findings.

The other-day rows rest on 14 cases.

## One case

Build 2106901443054145536 of the integration job failed in `TestCPGScheduling`;
other builds of the commit passed. Against one of them
(2106735347374231552):

```
2,117 → 2,586 lines · 337 templates · 12 findings (83 with --flat)

NEW        44 templates · 342 lines
        156 | === RUN   TestCPGScheduling/TestCPGBasicWithGangChildren  ×2
        157 | W1005 00:40:44.152211   44882 registry.go:342] setting componentGlobalsRegistry in SetFallback. …
        …
NEW        19 templates · 58 lines
        352 |     cpg_test.go:932: Test failed: step 8 (Verify root CPG and pg1 conditions are Scheduled) failed…  ×2
        …
        383 | --- FAIL: TestCPGScheduling/TestCPGBasicWithGangChildren (32.82s)  ×2
NEW        5 templates · 5 lines
       2569 | DONE 12322 tests, 626 skipped, 2 failures in 2107.718s
       2572 | make[1]: *** [Makefile:192: test] Error 1
```

The failed test's own output is new because gotestsum prints a test's log only
when it fails. The second block opens on the assertion.

## What this does not show

- **Hard cases.** These logs are friendly to a diff: the runner prints a failed
  test's output and nothing for a passing one, so the failure is hundreds of
  lines that exist in no baseline, and it is announced with `--- FAIL`. A
  failure that only changes a count, or one line among thousands that look
  like it, in a log with no marker to grep for, is what a template diff is
  for, and it is not tested here.
- **Two jobs of one project**, both Go, both printed by gotestsum, both logging
  with klog.
- **Precision by hand.** A finding is counted, not judged.
- **That the control findings are wrong.** A line some passing runs print and
  others do not is a real difference. More baselines are how logdelta learns it
  is not news; three were not always enough.

## The threshold

The same build with `--threshold 0.6`, one of the two settings that score best
on Loghub: the failed test named in all 264, the controls silent in the same
127 of 179, a mean of 4.7 findings instead of 5.0, and the reason shown in 139
of 239 cases instead of 159. On the measure that matters here it is worse. The
default is unchanged.

## Reproduce

```sh
git clone https://github.com/antonsoo/logdelta && cd logdelta && cargo build --release
python3 studies/kubernetes-ci/collect.py        # 518 MB into studies/kubernetes-ci/cache/
python3 studies/kubernetes-ci/evaluate.py       # this checkout
python3 studies/kubernetes-ci/evaluate.py --threshold 0.6 --label "checkout --threshold 0.6"
python3 studies/kubernetes-ci/evaluate.py --plain   # grep and a set difference

cargo install --root /tmp/logdelta-0.3.4 logdelta@0.3.4
python3 studies/kubernetes-ci/evaluate.py --binary /tmp/logdelta-0.3.4/bin/logdelta --label 0.3.4
```

`collect.py` fetches the builds the manifest lists. The bucket keeps about
three months, so the oldest will age out; `collect.py --rescan` chooses cases
from the builds that are there now and rewrites the manifest, and
`collect.py --reasons` reads each failed test's reason line from its JUnit
report into the manifest.
[`results.json`](results.json) has every case under each label.

The same builds, read for their JUnit reports rather than their logs, are the
subject of a [study in flakemap](https://github.com/antonsoo/flakemap/tree/main/studies/kubernetes-ci):
which tests failed, how often, and which had stopped.
