# Project working rules

## Scope

- Build a macOS menu bar app with Rust and GPUI for Claude, Codex, Grok Bot subscription usage, and Antigravity.
- Windows and Linux are outside the current project scope. Do not add portability work as a prerequisite.
- The repository currently contains plans only. Do not imply that a build, provider integration, release, or benchmark exists before it does.
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
