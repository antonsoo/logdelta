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

- **The diff names a failed test in all 264 cases**, with the released 0.3.4
  and with this checkout. A failed build's log is about 2,600 lines; the test
  that failed is in the handful of findings every time.
- **There is less to read now: a median of 4 findings where 0.3.4 reported 6,**
  and 8 where it reported 11 at the 90th percentile.
- **A passing build diffed against passing builds says nothing in 127 of 179
  cases** (0.3.4: 122), and three findings or more in 3 (0.3.4: 11). What it
  does report there is real: a kernel module that loaded in one run and not the
  others, a controller's line that some passing runs print and some do not.
- **A baseline from another day cost 0.3.4 half its report.** Kubernetes logs
  with klog, whose lines begin `I1005` for an info line on 5 October. Filed
  under its first token, every such line was a new template the next day.
  Against a passing build of another date 0.3.4's median report was 9 findings;
  it is now 4, the same as for a baseline of the same day.

None of these logs was looked at while the miner was being changed. The changes
came from [the Loghub benchmark](../loghub/README.md); this is the check that
they hold on logs they were not made on.

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

`evaluate.py` runs `logdelta diff --json` and reads the report.

- **Names a failed test**: some NEW finding, or NEW VALUE finding, holds the
  name of a test the JUnit report lists as failed, in its line or its template.
  The name comes from the report, not from the log.
- **Findings**: how many things the report asks a reader to look at. Lines that
  belong together (the output of one failed test) are one finding.
- **Control**: a passing build as the target, the same baselines. Every finding
  there is something that differs between passing runs.

## Results

| | 0.3.4 | this checkout |
|---|---:|---:|
| **Failed build against one passing build** (264) | | |
| names a failed test | 264 | 264 |
| findings: median, mean, 90th percentile | 6, 6.7, 11 | 4, 5.0, 8 |
| **Failed build against up to three** (229) | | |
| names a failed test | 229 | 229 |
| findings: median, mean, 90th percentile | 5, 6.6, 11 | 4, 4.9, 9 |
| **Passing build against passing builds** (179) | | |
| no findings | 122 | 127 |
| one, two, three or more | 28, 18, 11 | 29, 20, 3 |
| **Failed build against a passing build of another day** (14) | | |
| findings: median | 9 | 4 |
| Templates found in a pair of logs, median | 103 | 606 |

By job, against one passing build: the integration job goes from a median of 9
findings to 6 and the unit job from 4 to 3. The unit job's controls are silent
in all 82 cases with both versions; the integration job's are silent in 45 of 97
(0.3.4: 40).

The last row is the reason for the others. 0.3.4 found about a hundred
templates in two logs of more than 2,000 lines each, because its templates grew: a
wildcard counted as agreement, so `I1005 <DUR> <NUM> <*> <*> <*> <*> <*> <*>
<*>` took in every klog line of ten tokens. A new line that fits a template
like that is not new. This checkout finds about six hundred, one per log
statement, and still reports less, because what it reports is grouped into the
failure it belongs to.

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
  lines that exist in no baseline. Naming the test is close to a floor. A
  failure that only changes a count, or one line among thousands that look like
  it, is not tested here.
- **Two jobs of one project**, both Go, both printed by gotestsum, both logging
  with klog.
- **Precision by hand.** A finding is counted, not judged. Whether the 4
  findings of a median report are the 4 a person would pick was not checked
  beyond reading a few dozen.
- **That the control findings are wrong.** A line some passing runs print and
  others do not is a real difference. More baselines are how logdelta learns it
  is not news; three were not always enough.

## The threshold

The same build with `--threshold 0.6`, the setting that scores best on Loghub:
the failed test named in all 264, the controls silent in the same 127 of 179,
and a mean of 4.7 findings against one passing build instead of 5.0. It finds
more templates inside the blocks it reports (a median of 50 ungrouped findings
where the default has 40). Within noise, on this evidence; the default is
unchanged.

## Reproduce

```sh
git clone https://github.com/antonsoo/logdelta && cd logdelta && cargo build --release
python3 studies/kubernetes-ci/collect.py        # 518 MB into studies/kubernetes-ci/cache/
python3 studies/kubernetes-ci/evaluate.py       # this checkout

cargo install --root /tmp/logdelta-0.3.4 logdelta@0.3.4
python3 studies/kubernetes-ci/evaluate.py --binary /tmp/logdelta-0.3.4/bin/logdelta --label 0.3.4
```

`collect.py` fetches the builds the manifest lists. The bucket keeps about
three months, so the oldest will age out; `collect.py --rescan` chooses cases
from the builds that are there now and rewrites the manifest.
[`results.json`](results.json) has every case under each label.

The same builds, read for their JUnit reports rather than their logs, are the
subject of a [study in flakemap](https://github.com/antonsoo/flakemap/tree/main/studies/kubernetes-ci):
which tests failed, how often, and which had stopped.
