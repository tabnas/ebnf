# ci/

Staging area for GitHub Actions workflow changes.

This directory exists because session credentials cannot write
`.github/workflows/*` — see admin `DECISIONS.md` ADR-8. To change CI:

1. Put the intended workflow file in `workflows/`.
2. A maintainer promotes it with the admin `rollout/apply-ci-folders.sh`
   script.

## Pending

- **`workflows/docs.yml`** — the prose gate: Vale over the reader-facing
  pages at the levels set in `.vale.ini`, on the file list
  `ts/scripts/gated-docs.cjs` produces. See `docs/STYLE-GUIDE.md`.

  It needs no sibling checkouts and no secrets, and pins its own Vale
  version. Errors fail the job; warnings go to the run summary as a
  report. `make prose` runs the identical check locally, and the test
  suite already runs the other half of the gate
  (`ts/test/docs.test.js`), so promoting this adds the spelling and
  Google-convention arm rather than the whole gate.

  Its `paths:` lists now carry `rs/README.md` as well, in both the push
  and the pull_request trigger. A page joins the gated set in
  `ts/scripts/gated-docs.cjs`, and a page the workflow's filters do not
  name is a page the hosted half of the gate never reads.

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
