# AGENTS.md — omni-ui

For coding assistants (and people) setting this repository up. Plain steps; every command runs from the repo root.

**Status: pre-alpha, under development.** Interfaces, numbers and file layout can change without notice. Read `STATUS.md` first: what works today (each line names its test and evidence file), what is not yet tested, the known failures and the corrections.

## What it is
Native apps and a light-use web browser drawn by the omni-render engine. One packed widget tree (22 bytes per widget) emits both the markup the engine draws and an accessibility tree for assistive technology, and every input ends in one `act(verb, arg, source)` call, so what is drawn and what is read cannot disagree. Licence: MIT OR Apache-2.0 (the separate `omni-ui-t25` binary is PolyForm; `LICENSING.md`). Disclosure: dated prior art, `TECHNICAL-DISCLOSURE.md`. Check out omni-render beside this repository, then: Apps are chosen on the command line (`omni-ui run calendar file.ics`, `run …

## Install and check (3 commands)
```bash
git clone --depth 1 https://github.com/SovRing-Labs/omni-ui && cd omni-ui
git clone --depth 1 https://github.com/SovRing-Labs/omni-render ../omni-render && tools/pin-engine.sh $(git -C ../omni-render rev-parse HEAD)
cargo build --release && python3 tests/verify_calendar.py   # expect: exit 0
```
Needs: cargo, omni-render checkout beside it.
Whole suite instead: `bash install.sh --no-gui` (python check → venv → every runnable `suite.json` test, headless; without `--no-gui` it then opens the local results page on 127.0.0.1).
Note: pins the omni-render engine source first, then a full Rust release build (several minutes of CPU).

## What the tests mean
`suite.json` lists every claim this repository makes with its verify command, the expected output, what passing means, how the number was obtained and how to use it in your own work (11 tests, in `suite.json` at the repository root). `STATUS.md` says what works today, what is not yet tested and the known failures; the umbrella's `STATUS.md` has one row per repository.

## Where the evidence is
- `DOSSIER.md` — history, metrics and limits
- `TECHNICAL-DISCLOSURE.md` — the method, as dated prior art
- `CITATION.cff` — DOI 10.5281/zenodo.23282746

## Honest limits (from DOSSIER.md)
Not light on script-heavy pages (up to 2.23 GB on a video site). Transforms and keyframes (compositor) not built; plug-ins, developer tools, the tracker head designed only. No screen-reader session and no daily-use test recorded.

## Licence
MIT OR Apache-2.0 (omni-ui-t25 binary PolyForm). Details: `LICENSING.md`.

## Guided setup through MCP
The SovRing umbrella ships an MCP server that explains, checks readiness, clones and runs these tests: https://github.com/SovRing-Labs/SovRing (README → "Paste this to your assistant"). Zero-install docs endpoint: https://gitmcp.io/SovRing-Labs/omni-ui
