# Project working rules

## Required workflow

- Apply `pstack-codex` to every task in this repository, including investigation, code, tests, documentation, review, and release work.
- Start with `$pstack-codex:poteto-mode`, read its installed `SKILL.md`, and follow the matching playbook and applicable leaf skills. Locate it through the active Codex skill catalog or the `pstack-codex` plugin installation under `${CODEX_HOME:-$HOME/.codex}/plugins/cache`.
- Keep design and implementation review independent. Verify observable behavior before reporting completion. Record checks that could not run on the current machine.
- Install or update the workflow using [pstack-codex](https://github.com/ColdTbrew/pstack-codex). Do not copy its entire skill collection into this repository.
- Use `uv` for any Python work.
- Use the user's MacBook Pro on the tailnet, `coldbrew@macbookpro`, for macOS builds and app execution. The current DGX has the dedicated SSH alias `light-usage-bar-mac`. The DGX Spark Ubuntu ARM64 machine can verify the pure Rust core. Keep remote work in an isolated task directory and record the actual Mac environment and checks. Do not substitute Linux checks for macOS runtime evidence.

## Scope

- Build a macOS menu bar app with Rust and GPUI for Claude, Codex, Grok Bot subscription usage, and Antigravity.
- Windows and Linux are outside the current project scope. Do not add portability work as a prerequisite.
- The repository contains Rust data/cache, provider and scheduling libraries, an AppKit host, a GPUI panel, and packaging scripts. Implementation does not establish runtime or release completion. Check docs/release.md and docs/providers.md for observed verification and remaining gates. Never report unverified UI, provider support, releases, or benchmarks as complete.
- Keep project documentation in Korean, with technical identifiers and source names unchanged where useful.

## Correctness

- Separate subscription quota from API billing and local token estimates.
- Represent unknown, stale, unauthenticated, unsupported, and failed states explicitly. Missing data is not zero usage.
- Preserve each provider's units, windows, account semantics, timestamps, and data provenance.
- Verify provider behavior against primary sources and real authorized account observations. Record the date and remaining uncertainty.

## Resource use

- Optimize the state after the panel has been used and closed, as well as cold idle.
- Prefer event-driven work, bounded requests, coalesced scheduling, cancellation, and explicit process cleanup.
- Do not introduce busy polling, permanent render loops, or always-running collection browsers or provider CLIs.
- The separate GPUI helper is a candidate, not a finalized architecture. Follow the decision record and measurements.
- Compare release builds on the same Mac with equivalent provider sets. Include child processes and state the memory metric.
- Never invent measurements or claim Rust/GPUI alone guarantees lower memory use.

## Credentials and changes

- Do not commit tokens, cookies, account data, private logs, or live credential responses.
- Do not silently overwrite credentials shared with an official client.
- Pass display snapshots rather than credentials across the UI boundary.
- Update relevant plans when implementation changes their assumptions.
- Prefer focused validation of data parsing, lifecycle, and concrete performance risks. Avoid tests that only duplicate documentation or implementation structure.
