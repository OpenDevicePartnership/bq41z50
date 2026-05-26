# AGENTS.md

Guidance for AI coding assistants (and humans) working in the
`openDevicePartnership/bq41z50` repository. This file is the canonical
source for project conventions, build/test commands, and structural
information. Read it before making changes.

If anything in this file conflicts with what you observe in the
repository, the repository wins — please update this file in the same
change.

## What this crate is

At the time of writing, this repository is an **early-stage scaffold**
derived from the
[`openDevicePartnership/embedded-rust-template`](https://github.com/OpenDevicePartnership/embedded-rust-template)
template. The intent of the repository (per its name) is to host a
no_std Rust driver for the **Texas Instruments BQ41Z50 battery fuel
gauge / battery management IC**.

Current state, observed directly from the source tree:

- `Cargo.toml` still declares `name = "embedded-rust-template"`
  (`Cargo.toml:2`). Renaming to a `bq41z50` crate is expected before
  publishing.
- The crate is a **binary** (`src/main.rs`), not yet a library. The
  template `README.md` documents how to convert it to a library when
  needed.
- There are no driver modules, no `embedded-hal` dependency, and no
  `defmt` usage yet. Those will be added as the driver is implemented.

Treat AGENTS.md as a living document: when you add real driver code,
update the relevant sections (especially *Driver specifics*,
*Repository layout*, and *Building and testing*).

## Repository layout

```
.
├── AGENTS.md                       <- this file
├── Cargo.toml                      package manifest (name: embedded-rust-template)
├── Cargo.lock                      committed lockfile (CI uses --locked)
├── rust-toolchain.toml             pins rustfmt + clippy components
├── rustfmt.toml                    formatting config (max_width = 120)
├── deny.toml                       cargo-deny config (licenses, advisories, sources, bans)
├── CODEOWNERS                      review routing
├── CODE_OF_CONDUCT.md
├── CONTRIBUTING.md                 commit + PR etiquette
├── SECURITY.md
├── LICENSE                         MIT
├── README.md                       template-derived; describes how to retarget / convert to lib
├── .github/
│   └── workflows/
│       ├── check.yml               fmt, doc, hack-clippy, deny, test, msrv, machete
│       ├── nostd.yml               cargo check on thumbv8m.main-none-eabihf
│       ├── cargo-vet.yml           cargo vet --locked
│       └── cargo-vet-pr-comment.yml
├── .vscode/
│   └── settings.json               rust-analyzer.cargo.target = thumbv8m.main-none-eabihf
├── src/
│   ├── main.rs                     entry; switches between hosted and no_std
│   └── baremetal/
│       └── mod.rs                  no_std panic handler
└── supply-chain/                   cargo-vet state (audits, config, imports)
```

There is currently no `tests/`, `examples/`, or `benches/` directory.

## Building and testing

The toolchain is pinned only by component (`rust-toolchain.toml` adds
`rustfmt` and `clippy`); the rust version itself is controlled by the
host toolchain and CI matrix.

- **MSRV:** `1.85` (declared in `Cargo.toml:7` and re-asserted in the
  `msrv` job of `.github/workflows/check.yml`).
- **Edition:** `2021`.
- **License:** `MIT`.

All of the following commands have been run successfully against the
current `main` from this repository's working directory.

### Format

```sh
cargo fmt --check        # CI: check.yml / fmt
cargo fmt                # to auto-fix
```

`rustfmt.toml` sets `max_width = 120`. No other settings — defaults
otherwise.

### Hosted build / check

```sh
cargo check --locked            # CI: check.yml / msrv (with toolchain 1.85)
cargo build --locked
```

When compiled for the host (anything other than `target_os = "none"`),
`src/main.rs` produces a tiny `println!("Hello, world!")` binary. This
is intentional — it exists so `cargo test` works on the host.

### no_std / embedded build

The crate gates `no_std` and `no_main` on `target_os = "none"` (see
`src/main.rs:1-2`). To exercise the embedded path you must pass a
bare-metal target:

```sh
rustup target add thumbv8m.main-none-eabihf
cargo check --target thumbv8m.main-none-eabihf --locked   # CI: nostd.yml
```

To change the default target, see `README.md` — `.vscode/settings.json`,
`.github/workflows/nostd.yml`, and (eventually) a `.cargo/config.toml`
all need to be kept in sync.

### Tests

```sh
cargo test --locked
```

Only one trivial host test exists today (`src/main.rs:13-18`). CI runs
tests through `cargo-hack` to cover the feature powerset — the crate
currently has no features, so this is equivalent to plain `cargo
test`:

```sh
cargo hack --feature-powerset test --locked   # CI: check.yml / test
```

### Lints (clippy)

Local equivalent of the CI clippy invocation:

```sh
cargo clippy --locked -- \
    -Dwarnings \
    -D clippy::suspicious \
    -D clippy::correctness \
    -D clippy::perf \
    -D clippy::style
```

CI runs the same flags through `cargo-hack` over the feature powerset
and over both `x86_64-unknown-linux-gnu` and
`thumbv8m.main-none-eabihf` (see `check.yml` jobs `hack-clippy` and
`test`).

`Cargo.toml` already declares these lints as `forbid` at the package
level (`Cargo.toml:15-19`), so plain `cargo clippy` will also reject
them — the explicit CLI flags exist to defend against the lint config
being weakened.

### Docs

```sh
RUSTDOCFLAGS=--cfg docsrs cargo +nightly doc --no-deps --all-features
```

(Or, without nightly:)

```sh
cargo doc --no-deps --all-features
```

CI uses nightly so that features like `#[doc(cfg(...))]` work; locally,
stable is fine if you do not use those attributes.

### Supply-chain checks

```sh
cargo install cargo-deny    # one-time
cargo deny --all-features --locked check       # CI: check.yml / deny

cargo install cargo-machete # one-time
cargo machete                                  # CI: check.yml / machete

cargo install --version 0.10.1 cargo-vet       # one-time, pin matches CI
cargo vet --locked                              # CI: cargo-vet.yml / vet
```

`deny.toml` allows licenses `MIT`, `Apache-2.0`, `Unicode-3.0`,
`BSD-3-Clause`; allows git sources from the `embassy-rs` GitHub org
only; and pins two advisory ignores (`RUSTSEC-2024-0370`,
`RUSTSEC-2024-0436`).

`supply-chain/` holds the cargo-vet state (`audits.toml`,
`config.toml`, `imports.lock`). Update it via `cargo vet`, not by
hand.

## Code conventions

- **Edition 2021, MSRV 1.85.** Do not use language or stdlib features
  introduced after 1.85 without bumping `rust-version` in
  `Cargo.toml` and the `msrv` matrix in `check.yml`.
- **Formatting:** rustfmt with `max_width = 120`. Run `cargo fmt`
  before committing. Do not hand-format around rustfmt.
- **Lints:** the four `clippy` groups `suspicious`, `correctness`,
  `perf`, `style` are `forbid` (not just `deny`) at the crate root.
  Do not weaken or `#[allow(...)]` past them without a written
  justification — `forbid` exists precisely to make local overrides
  impossible.
- **`no_std` posture:** the crate is currently `no_std` *only* when
  `target_os = "none"`. The hosted target retains `std` so that
  testing infrastructure works. Preserve this dual-mode shape:

  ```rust
  #![cfg_attr(target_os = "none", no_std)]
  #![cfg_attr(target_os = "none", no_main)]
  ```

  Driver code that should run on-target must be `no_std`-clean (no
  `std::`, no allocations unless an `alloc` feature is added later).
- **Panic handler:** lives in `src/baremetal/mod.rs` and is only
  compiled on `target_os = "none"`. Keep it minimal; do not pull in
  panic-printing crates without a clear reason.
- **Features:** none today. If you add features, keep them additive
  (the `cargo hack --feature-powerset` jobs in CI assume this).
- **Dependencies:** every new dependency must (a) be license-compatible
  per `deny.toml`, (b) come from crates.io or an allowed git org, and
  (c) be vetted via `cargo vet`. Prefer well-established `embedded-hal`
  ecosystem crates for driver work.
- **Unsafe:** there is currently no `unsafe` in the crate. Any
  `unsafe` block added later should carry a `// SAFETY:` comment
  explaining the invariant being upheld.

## Driver specifics (when you start writing the driver)

The repository name implies a driver for the TI BQ41Z50 battery
management IC. When real driver code lands, prefer the conventions
common across the OpenDevicePartnership ecosystem:

- Use `embedded-hal` (sync) and/or `embedded-hal-async` traits for I²C
  / SMBus access rather than depending on a specific HAL.
- Keep register definitions in a dedicated module (e.g.
  `src/registers.rs`) with `#[repr(u8)]` enums or `const` addresses,
  cross-referenced to the BQ41Z50 datasheet sections.
- Model the public API as a generic `Bq41z50<I2C>` struct constructed
  from an HAL bus handle, plus typed getter/setter methods. Keep
  blocking and async variants behind features if both are needed.
- Use `defmt` (behind a feature flag) for on-target logging rather
  than `log` or `println!`. Do not add `defmt` unconditionally — keep
  the crate usable in environments that do not pull in a global
  logger.
- Convert from binary to library per the steps in `README.md`
  (`[lib]` stanza, move `main.rs` → `lib.rs`, keep the
  `#![cfg_attr(target_os = "none", no_std)]` shape).

None of the above exists in the tree today — they are recommendations
to follow when the driver is implemented, so AGENTS.md does not lie
about what is on disk.

## Commit & PR conventions

From `CONTRIBUTING.md` and observed history:

- Use meaningful commit messages (`CONTRIBUTING.md:19`). Follow the
  classic Tim Pope style: imperative-mood subject ≤ 50 chars, blank
  line, wrapped body.
- A `<type>: <subject>` prefix (`docs:`, `feat:`, `fix:`, `chore:`,
  …) is encouraged but not strictly enforced — the only commit
  currently in `main` is `Initial commit` (`git log --pretty=%s`).
- **Open PRs as drafts first** (`CONTRIBUTING.md:23`). Verify that all
  workflows in `.github/` pass on the draft before requesting review.
- Submitting code under any license other than MIT requires calling
  that out explicitly in the PR description
  (`CONTRIBUTING.md:14-15`).
- When reporting a regression, run `git bisect` first and cite the
  first offending commit (`CONTRIBUTING.md:28`).
- `CODEOWNERS` controls required reviewers — let GitHub auto-assign
  rather than @-mentioning individuals.

## What not to do

- **Do not** commit changes that fail `cargo fmt --check`, `cargo
  clippy` (with the four `-D` groups), or `cargo check --target
  thumbv8m.main-none-eabihf --locked`. CI will reject them.
- **Do not** weaken the `forbid` clippy lints in `Cargo.toml`.
- **Do not** use `std`, `alloc`, or heap allocation in code that runs
  on `target_os = "none"`.
- **Do not** modify `supply-chain/` by hand; use `cargo vet`.
- **Do not** edit `Cargo.lock` manually; let Cargo regenerate it. CI
  uses `--locked` everywhere — keep the committed lockfile in sync
  with `Cargo.toml`.
- **Do not** add new git dependencies from organizations outside the
  `allow-org` list in `deny.toml` without first adding them there.
- **Do not** introduce a global `panic_handler` that pulls in heavy
  dependencies; the `loop {}` handler in `src/baremetal/mod.rs` is
  intentionally minimal.
- **Do not** rename the crate or change its license without updating
  `Cargo.toml`, `deny.toml`, and any in-tree references in the same
  commit.

## How to find more context

- **`README.md`** — how to retarget the template and convert it to a
  library.
- **`CONTRIBUTING.md`** — commit message + PR etiquette.
- **`.github/workflows/*.yml`** — authoritative source for which
  commands CI runs. If a command in this file diverges from CI, CI is
  right; update this file.
- **`deny.toml`** — license allowlist, advisory ignores, git source
  allowlist.
- **`supply-chain/`** — cargo-vet audit state and imports.
- **TI BQ41Z50 datasheet & technical reference manual** — required
  reading before touching register-level code.
- **Sibling repos under `openDevicePartnership/`** — other battery /
  fuel-gauge drivers in the org are a good reference for idiomatic
  driver shape (HAL trait bounds, register module layout, defmt
  feature gating).

## Incorporated from copilot-instructions.md

There is no `copilot-instructions.md` (and no
`.github/copilot-instructions.md` or `.copilot/instructions.md`) in
this repository at present. This AGENTS.md is therefore the first and
only authoritative guidance file for AI coding assistants here. If a
`copilot-instructions.md` is added later, it must be a thin pointer to
this file — substantive guidance belongs here, not there, to avoid
drift.

## Model selection & cost discipline

Premium models (Opus, GPT-5 family, "high"/"xhigh" reasoning variants)
cost an order of magnitude more than standard models (Sonnet, Haiku,
mini). Most steps in a typical task do not need premium reasoning,
and over-using premium models wastes credits without improving
outcomes. The rules below apply to *all* model selection: your own
session, sub-agents launched via the `task` tool, and parallel work
launched via `/fleet`.

### Default posture

- **Default to the cheapest model that can do the job.** Reach for a
  premium model only when one of the escalation triggers below is hit.
- **Plan with premium, execute with cheap.** Spend at most one or two
  premium turns on design / planning, then downshift to a cheaper
  model for mechanical execution of the plan.
- **Never bump the model "just in case."** If you cannot articulate
  *why* a cheaper model would fail, use the cheaper model.

### Escalation triggers (use a premium model)

Reach for a premium model when *any* of these are true:

- Cross-module refactor, architectural design, or API design from
  scratch.
- Subtle correctness reasoning: concurrency, lifetimes, `unsafe`,
  FFI ABI, cryptography, safety-critical control paths.
- Debugging a failure that survived one prior cheap-model attempt.
- Reviewing code on a safety-, security-, or money-critical path.
- The diff cannot be predicted in advance — i.e. there is genuine
  creative or design work to do, not just typing.

### De-escalation triggers (use a cheap model)

Use the cheapest available model when *any* of these are true:

- Searching, reading, summarising files or docs.
- Single-file mechanical edits: rename, format, lint fix, dependency
  bump, boilerplate, scaffolding from a known template.
- Generating tests for code that already works.
- Running builds, tests, linters, or other commands where the model
  only needs to report success/failure.
- Routine commits, PR descriptions, changelog entries.
- The diff is essentially predictable before generation.

### Sub-agent routing (the `task` tool)

When delegating with the `task` tool, set `model:` explicitly. Do not
let sub-agents inherit a premium default for cheap work.

| Sub-agent type    | Default model             | Override to                                     |
|-------------------|---------------------------|-------------------------------------------------|
| `explore`         | cheap                     | keep cheap (`claude-haiku-4.5` or `gpt-5-mini`) |
| `task` (run cmd)  | cheap                     | keep cheap                                      |
| `research`        | cheap for breadth         | premium only for the final synthesis            |
| `general-purpose` | match task                | cheap for mechanical work; premium for design   |
| `rubber-duck`     | premium                   | keep premium — this is where reasoning pays off |
| `code-review`     | premium on critical paths | cheap on cosmetic / mechanical diffs            |

### `/fleet` (parallel sub-agents) rules

- Fleet mode multiplies cost by the fleet width. Apply the rules
  above *per worker*, not in aggregate.
- Split a fleet job along complexity lines: route the cheap,
  parallelisable workers (file edits, test runs, doc updates) to a
  cheap model; reserve premium models for the small number of
  workers that need real reasoning.
- If every worker in a fleet would need a premium model, the work is
  probably not a good fit for fleet mode — reconsider the
  decomposition before paying N× premium.

### Session hygiene

- Keep sessions short and focused. Long premium sessions are the
  single largest source of waste because every turn re-processes the
  full history.
- Use `/compact` when the conversation grows long, and `/new` for
  unrelated work.
- Prefer `/ask` for one-off side questions so they don't extend the
  main session.

### When in doubt

Ask: *"If a cheaper model produced the wrong answer here, would I
catch it in seconds (compiler, tests, my own review) or in
weeks (production incident)?"* If the former, use the cheap model
and let the feedback loop do its job.
