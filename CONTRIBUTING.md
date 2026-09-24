# Contributing

```console
$ git clone https://github.com/antonsoo/logdelta
$ cd logdelta
$ cargo test
```

Before sending a change:

```console
$ cargo fmt --all
$ cargo clippy --all-targets --all-features -- -D warnings
$ cargo test --all-targets --all-features
```

A few conventions:

- Core logic lives in `src/{mask,drain,scoring,analysis,context,io}.rs` and stays free of
  I/O and terminal concerns, so it's usable as a library and easy to unit test. CLI wiring
  and rendering live in `src/cli.rs`, `src/main.rs`, and `src/output/`.
- New maskers get a unit test in `src/mask.rs` showing what they do and don't match.
- New CLI-visible behavior gets an `assert_cmd` test in `tests/integration.rs`; if it
  changes rendered output, an [`insta`](https://insta.rs) snapshot
  (`cargo insta review` after running tests with `INSTA_UPDATE=always` locally) makes the
  diff visible in review.
- Log fixtures under `examples/` are synthetic and should stay that way — don't paste in
  real log output, even sanitized.

Commit style: [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`,
`fix:`, `docs:`, `test:`, `ci:`, `chore:`).
