# omni-ui

<!-- status-badges (REPO-ACCESS-C) -->
[![status: pre-alpha (under development)](https://img.shields.io/badge/status-pre--alpha%20%28under%20development%29-orange)](STATUS.md) **Status: pre-alpha, under development** — what works today, what is untested and what failed: [`STATUS.md`](STATUS.md)

[![defensive publication: DOI 10.5281/zenodo.23282746](https://img.shields.io/badge/defensive%20publication-DOI%2010.5281%2Fzenodo.23282746-blue)](https://doi.org/10.5281/zenodo.23282746) **Defensive publication:** [`TECHNICAL-DISCLOSURE.md`](TECHNICAL-DISCLOSURE.md) · DOI [10.5281/zenodo.23282746](https://doi.org/10.5281/zenodo.23282746)
<!-- /status-badges -->

Native apps and a light-use web browser drawn by the omni-render engine. One packed widget tree (22 bytes per widget) emits both the markup
the engine draws and an accessibility tree for assistive technology, and every input ends in one `act(verb, arg, source)` call, so what is
drawn and what is read cannot disagree. **Licence:** MIT OR Apache-2.0 (the separate `omni-ui-t25` binary is PolyForm; `LICENSING.md`).
**Disclosure:** dated prior art, `TECHNICAL-DISCLOSURE.md`.

## Build
Check out omni-render beside this repository, then:
```bash
tools/pin-engine.sh <omni-render-commit>   # copies the engine source at that commit into .engine/ (git archive)
cargo build --release                      # omni-ui, permissive code only
cargo build --release --features t25       # also omni-ui-t25 (PolyForm Small Business OR Noncommercial)
./target/release/omni-ui --engine-rev      # which engine commit this binary was built from
```
Apps are chosen on the command line (`omni-ui run calendar file.ics`, `run library`, `run reader`, `run notes`, `run mail`, `web <url>`); `omni-ui shot`, `tree`, `measure` and the `*-check` verbs are the verification tools.
`tests/` holds the verification scripts for each app (`python3 tests/verify_calendar.py` and so on). The browser's design and memory bench
are described in `TECHNICAL-DISCLOSURE.md`; its harnesses are in the research corpus. History and limits: `DOSSIER.md`. Sources: `PROVENANCE.md`.
Linux, Rust 2024 edition.
