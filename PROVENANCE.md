# omni-ui — Provenance

**Licence:** MIT OR Apache-2.0 (`LICENSING.md`); the T25 app is a separate binary built with the `t25` feature under PolyForm Small Business
1.0.0 OR PolyForm Noncommercial 1.0.0. **Disclosure:** published as dated prior art (Technical Disclosure Part 7 §3); no patents filed.

## How this code was written
From our own design, the published standards named below and the documentation of the libraries it links; no code from any browser, toolkit
or note application was read or copied. Format modules follow their standard text (`ics.rs` from RFC 5545; the accessibility mapping from the
W3C specifications). The engine it draws with is mounted from omni-render at a pinned revision and carries that repository's statement.

## Ideas this builds on (cited, not copied)
| Idea | Source |
|---|---|
| Privileged broker and confined page processes | Grier, Tang and King, IEEE S&P 2008; Reis and Gribble, EuroSys 2009 |
| Kernel confinement | Landlock (Linux 5.13) and seccomp-BPF documentation |
| Accessibility tree for a self-drawn toolkit | WAI-ARIA 1.2, HTML-AAM, Accessible Name Computation 1.2; AccessKit |
| HTTP and cookie behaviour | RFC 9110, 9112, 6265, 6455 |
| Contrast | WCAG 2.x success criterion 1.4.3 |
| Recurrence on residue clocks | Szabo and Tanaka, 1967 |
| Compact, keyboard-first chrome (design references, observed only) | Zen Browser, Arc, Safari compact tabs, qutebrowser, Nyxt |

## Used, not copied
winit, softbuffer, accesskit, accesskit_winit (MIT OR Apache-2.0 / Apache-2.0) plus the engine's crates (see omni-render's `PROVENANCE.md`);
omni-spec (Apache-2.0). No PolyForm code in the default build. Fonts come from the operating system.

## Checks to run on the export tree (before release)
`cargo metadata` licence list for the lock file; ScanCode with snippet matching and gitleaks; `omni-ui --engine-rev` equal to the pinned
engine revision.
