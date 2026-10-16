> **Status: under development — pre-alpha.** Interfaces, numbers and file layout can change without notice.

# omni-ui — status

Last checked: 2026-10-16, Linux laptop (Intel i5-8365U, 8 threads), on a clean copy of this repo. 5 of 11 suite tests work today; 6 of 11 can run without special hardware. Each line below names its test id in `suite.json` and the evidence file it rests on.

## Works today

- **U1** Browser memory: 211.3 MiB vs Firefox 7,297.6 MiB on 10 pages — PASS 2026-10-16 (`211.3 MiB against 7,297.6 MiB`); evidence: `TECHNICAL-DISCLOSURE.md`, `DOSSIER.md`.
- **U2** Connectivity: 13 of 13 routes pass in each of three runs — PASS 2026-10-16 (`13 of 13 routes`); evidence: `TECHNICAL-DISCLOSURE.md`, `DOSSIER.md`.
- **U4** Merge gate: layout baseline unchanged, JS probe 106/106, connectivity 13/13, ECMAScript tests 54.5 % — PASS 2026-10-16 (`24,032 of 28,040; 106 of 106; 29,203 of 53,607`); evidence: `TECHNICAL-DISCLOSURE.md`.
- **U5** Contrast by arithmetic: 1.02:1 fixed to at least 4.5:1; one failed design disclosed — PASS 2026-10-16 (`1.02 : 1; 4.5 : 1; nine of eleven`); evidence: `TECHNICAL-DISCLOSURE.md`, `DOSSIER.md`.
- **U6** Vault link-resolver fixture holds 20 expected resolutions — PASS 2026-10-16 (`20 expected resolutions`); evidence: `tests/fixtures/vault-expect.json`, `tests/verify_vault.py`.

## Not yet tested

- **U1b** Re-run the memory bench against Firefox — external: needs something the test laptop does not have; needs: cargo (Rust 2024 edition) to build omni-ui, firefox, network access to the page list, Linux (PSS from /proc)
- **U7** Residue-clock recurrence check: 10,000 random rule pairs, 0 mismatches required — external: needs something the test laptop does not have; needs: cargo (Rust 2024 edition) to build target/release/omni-ui, python3 >= 3.8, bin/reason-gen from the author's stack for part 2 (not in this repo)
- **U8** Calendar: 12-event .ics agenda equals python-dateutil — external: needs something the test laptop does not have; needs: cargo (Rust 2024 edition) to build omni-ui, python3 with python-dateutil, a desktop session with AT-SPI (pyatspi), espeak-ng, whisper.cpp for the window part, access-pack (the author's input bus, not in this repo)
- **U9** Reader: OCR word accuracy >= 0.95 overall, >= 0.90 worst page; reflow keeps boxes inside the window — external: needs something the test laptop does not have; needs: cargo (Rust 2024 edition) to build omni-ui, the access-pack module and its bench_read scans (not in this repo), AT-SPI desktop session, espeak-ng, whisper.cpp
- **U10** Notes and Library apps read the vault and corpus without writing — external: needs something the test laptop does not have; needs: cargo (Rust 2024 edition) to build omni-ui, strace, a real Obsidian vault and document shelves (the author's, not in this repo), AT-SPI desktop session
- Quick test `python3 -c "import json;d=json.load(open('tests/fixtures/vault-expect.json'));assert len(d)==20;print(len(d),'fixture rows ok')"` — not run on the laptop (long Rust or model build).

## Known failures and honest negatives

- **U3** Honest negatives: 24-site census, 'lightweight' fails on script-heavy pages. Transforms and keyframes (compositor) are not built; the video site's music front end was not made usable; paint error against Chrome, frame times and screen-reader sessions were logged but their results were not retained (TECHNICAL-DISCLOSURE.md section 3). Recorded and re-checked 2026-10-16 (PASS); evidence: `TECHNICAL-DISCLOSURE.md`, `DOSSIER.md`
- U7–U10 need a cargo-built omni-ui plus the author's access pack and data; the quick test was not run on the laptop (long cargo build).

## Corrections

- No corrections touch this repo.

## How to re-check

- One file, any python3 >= 3.8: download [`sovring-suite.pyz`](https://github.com/SovRing-Labs/SovRing/releases/latest/download/sovring-suite.pyz), then `python3 sovring-suite.pyz --run omni-ui` (headless) or `python3 sovring-suite.pyz` (local page on 127.0.0.1).
- From this checkout: `bash install.sh` (checks python, prepares a venv, runs the suite headless, then opens the page; `--no-gui` to skip the page).
- Quick test only: `python3 -c "import json;d=json.load(open('tests/fixtures/vault-expect.json'));assert len(d)==20;print(len(d),'fixture rows ok')"`.

## Defensive publication

- `TECHNICAL-DISCLOSURE.md` — on public GitHub since 2026-10-10
- DOI: [10.5281/zenodo.23282746](https://doi.org/10.5281/zenodo.23282746) (Zenodo, see `CITATION.cff`)
- Licence: MIT OR Apache-2.0 for all source; the separate omni-ui-t25 binary is PolyForm Small Business 1.0.0 OR PolyForm Noncommercial 1.0.0; text CC BY 4.0 (LICENSING.md). Every verification script needs a cargo build of omni-ui and an omni-render checkout pinned by tools/pin-engine.sh; the quicktest only checks the answer-key fixture.
