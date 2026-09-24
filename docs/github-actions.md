# Recipe: diff a failed job against the last successful run (GitHub Actions)

**This is a worked example, not a published action.** Copy the parts you need into your own
workflow; there is no `antonsoo/logdelta-action` to reference. It also assumes a tagged
`logdelta` release with prebuilt binaries exists (`.github/workflows/release.yml` builds
one on every `v*` tag) — until the first tag is pushed, swap the `curl .../releases/latest`
line below for `cargo install --git https://github.com/antonsoo/logdelta`.

The idea: when a job fails, fetch its raw log, find the last run of the *same workflow +
job* on the default branch that succeeded, fetch that log too, and run `logdelta diff`
between them. Post the result to the job summary (and optionally as a PR comment).

## 1. Add a step that runs on failure

```yaml
# .github/workflows/ci.yml (excerpt)
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      # ... your normal build/test steps ...

      - name: Diff against last successful run
        if: failure()
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          set -euo pipefail

          # This run's own log (the job that just failed).
          gh run view "${GITHUB_RUN_ID}" --log > current.log

          # Find the most recent successful run of the same workflow on the default branch.
          BASELINE_RUN_ID=$(gh run list \
            --workflow "${GITHUB_WORKFLOW}" \
            --branch "${{ github.event.repository.default_branch }}" \
            --status success \
            --limit 1 \
            --json databaseId \
            --jq '.[0].databaseId')

          if [ -z "${BASELINE_RUN_ID}" ]; then
            echo "No prior successful run found; skipping logdelta." >&2
            exit 0
          fi

          gh run view "${BASELINE_RUN_ID}" --log > baseline.log

          curl -fsSL "https://github.com/antonsoo/logdelta/releases/latest/download/logdelta-x86_64-unknown-linux-gnu.tar.gz" \
            | tar xz -C /usr/local/bin logdelta

          logdelta diff baseline.log --target current.log --markdown \
            >> "$GITHUB_STEP_SUMMARY"
```

`gh run view --log` interleaves every step and every matrix job into one stream, each line
prefixed with the step name — that prefix becomes part of the masked line, which is fine:
identical steps produce identical prefixes in both runs, so it still clusters correctly. If
you want a tighter diff, pull a single job's log instead with the
[`GET /repos/{owner}/{repo}/actions/jobs/{job_id}/logs`](https://docs.github.com/en/rest/actions/workflow-jobs#download-job-logs-for-a-workflow-run)
REST endpoint (`gh api .../jobs/$JOB_ID/logs`), matching job name to job name between runs.

## 2. Post it as a PR comment instead of (or as well as) a job summary

```yaml
      - name: Comment on the PR
        if: failure() && github.event_name == 'pull_request'
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          logdelta diff baseline.log --target current.log --markdown > delta.md
          gh pr comment "${{ github.event.pull_request.number }}" --body-file delta.md
```

## 3. Multiple baselines, to ignore flaky lines

If your suite has a known-flaky retry or timing line, pass several recent successful runs as
multiple baselines instead of one; `logdelta` down-weights anything whose frequency already
varies across them (see `README.md#how-it-works`):

```console
$ logdelta diff run-142.log run-141.log run-139.log --target run-143-failed.log --markdown
```

## Notes

- `gh run view --log` requires the run to still have logs retained (GitHub's default
  retention is 90 days); for older baselines, archive logs as workflow artifacts instead and
  download those.
- `logdelta`'s exit code is `1` when it reports any finding and `0` when it finds nothing
  significant, so `logdelta diff ... || true` (as used implicitly by `if: failure()`
  already running only on an already-failed job) is usually what you want — don't let a
  logdelta finding fail an otherwise-passing job.
