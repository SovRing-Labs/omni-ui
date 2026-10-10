# omni-ui — Dossier

*History, specification, metrics, tests and limits. Companion to Technical Disclosure Part 7 §3 and `README.md`. The research record:
github.com/SovRing-Labs/omniring-research (docs 32–33, essay 14).*

> *"modern, futuristic, fluid, intuitive, highly accessible, capable, lightweight, ternary."* — the author's brief for the browser

## What it is

The app toolkit on the omni-render engine, and the browser built with it: **Omni Web**, whose page *and* window are drawn by our own
engine. A broker owns the window and the network and never parses a page; one locked-down process per page parses, runs scripts and
paints; one widget tree is both drawn and read by assistive technology. In the stack it is the **face**. Stage (essay 07): tool → system.

## Creation history

| Date (2026) | Milestone |
|---|---|
| 10-04 | The toolkit's first apps on the engine (find, reader, calendar, mail, notes, library) |
| 10-06 | Browser core on document pages: history, scroll, find, reader view, zoom, forms; memory 2.9 % of Firefox's on 10 pages |
| 10-07 | Connectivity fixed to the standards (13/13 routes); first-party-only cookies with a three-way switch; a 106-check platform probe |
| 10-07 | First chrome design rejected ("the opposite of what we are looking for"); Orbit designed and approved the same day; nine engine paint capabilities built for it; a contrast law |
| 10-07 | Merge gate and engine-pin rule written after an old binary was measured and installed |
| 10-08 | 24-site census; a compiled-script store for heavy pages; the video site's music front end not made usable |
| 10-09 | Licence decided: MIT OR Apache-2.0; the T25 app ships as a separate PolyForm binary behind a feature |

## Specification (summary)

* Frame protocol between broker and page process: one tag byte, a 4-byte length, the payload; at most 3 live page processes.
* Widget tree: 22 bytes per widget; every input ends in one `act(verb, arg, source)` call.
* Cookies: first-party only, in memory by default, kept across restarts only when the person chooses.

## Metrics (one laptop, i5-8365U)

| Property | Result |
|---|---|
| Memory, 10 pages, all processes | 211.3 MiB against Firefox 7,297.6 MiB (2.9 %); scripts on 4.2 % |
| Connectivity routes | 13 / 13 in three runs |
| 24-site census, run 3 | frames 24/24; scripts 402 ran / 39 failed; median page process 112 MB; three pages ≥ 500 MB |

## Limits

* Not light on script-heavy pages (up to 2.23 GB on a video site).
* Transforms and keyframes (compositor) not built; plug-ins, developer tools, the tracker head designed only.
* No screen-reader session and no daily-use test recorded.

## Lineage and credits

AccessKit and AT-SPI; Landlock and seccomp-BPF; RFC 9110 / 9112 / 6265 / 6455; WCAG 2.x; the multi-process browser design (Grier et al.;
Reis and Gribble). `PROVENANCE.md`. Updates: weekly, recorded here with dates.
