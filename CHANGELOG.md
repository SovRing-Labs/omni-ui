# omni-ui changelog

## 0.1.0 (2026-10-10)
omni-ui 0.1.0, first public release.

- Native apps (calendar, notes, mail, library, reader; a vault viewer module) and the Omni Web browser chrome ("Orbit" design study not included), drawn by
  the omni-render engine; one widget tree emits both the markup that is drawn and an AccessKit tree.
- Build: needs an omni-render checkout beside it; `tools/pin-engine.sh <rev>` copies the engine source at a commit into `.engine/`
  (`omni-ui --engine-rev` prints it). `cargo build --release` is MIT OR Apache-2.0 only; `--features t25` adds `omni-ui-t25` (PolyForm).
- Measured (see `TECHNICAL-DISCLOSURE.md`): memory 2.9 % of Firefox over ten pages (script off); connectivity 13 of 13 routes; the 24-site
  census shows frames on 24 of 24 but "lightweight" fails on script-heavy pages.
- Not shipped: design-study screenshots and HTML, run results.
