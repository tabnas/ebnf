# ci/

Staging area for GitHub Actions workflow changes.

This directory exists because session credentials cannot write
`.github/workflows/*` — see admin `DECISIONS.md` ADR-8. To change CI:

1. Put the intended workflow file in `workflows/`.
2. A maintainer promotes it with the admin `rollout/apply-ci-folders.sh`
   script.

## Pending

- **`workflows/rust.yml`** — the Rust gate: formatting, build, tests,
  doctests, clippy at `-D warnings`, and the `Cargo.lock` check, all of
  it inside `ci/rust/run.sh` so a contributor's local run and the hosted
  one cannot say different things.

  It is standalone rather than an arm of `ci.yml`, because `ci.yml`
  calls the org-shared polyglot workflow and that takes no Rust input,
  so promoting this file needs no change in `tabnas/.github`. It needs
  no secrets, and it clones the three sibling checkouts the crate
  depends on by path (`parser`, `bnf`, `support`); nothing else is
  fetched, because the oracle this port's parity is held to is committed
  under `rs/tests/oracle/`.
