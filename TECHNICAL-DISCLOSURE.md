# omni-ui — Technical Disclosure, Part 1 (2026-10-09): one tree for pixels and accessibility, a broker-and-page-process browser, measured against Firefox

> Copy notice. Licence of this text: CC BY 4.0. The software carries its own licence (`LICENSING.md`).

Dated prior art. Covers omni-ui work measured 2026-10-06 to 2026-10-09; the same material is in the research corpus as Technical Disclosure Part 7 section 3 and docs 32-33. Laptop = i5-8365U
(4 cores / 8 threads); wall times are noisy. The author's brief for the browser, verbatim: "modern, futuristic, fluid, intuitive, highly accessible, capable,
lightweight, ternary". Other browsers were observed as black boxes (output, DOM dump, process memory), never read. **Intent:** published so that everything it
describes is prior art; the authors claim no patent and ask that none be granted to anyone on it. Failures are disclosed as measured.

## 1. Summary
1. **One tree, two outputs.** The chrome is a packed widget tree (22 bytes per widget) that emits both the markup the engine draws and an AccessKit tree; every input ends in one `act(verb, arg, source)` call.
2. **Broker and page processes.** A broker owns the window, network, cookie jar, cache, history and tabs and never parses page content; one confined page process per live tab parses, styles, lays out, runs scripts and rasters, over a framed pipe (one tag byte, a 4-byte length, the payload). Page processes run with no_new_privs, Landlock and a seccomp-BPF allow-list.
3. **Memory.** Ten pages, summed PSS over all processes: 211.3 MiB against 7,297.6 MiB for Firefox (2.9 %), script off; 308.8 against 7,423.6 MiB (4.2 %), script on; 2 processes against 13 to 16.
4. **Connectivity.** 13 of 13 routes pass in each of three runs (gzip, chunked, redirects, 404 and 500 pages, 204 without waiting, cookies, latin-1, close-delimited body, `document.cookie`); the cookie jar is first-party only and in memory unless the person chooses to keep it.
5. **Census.** 24 real sites: a frame on 24 of 24, scripts ran / failed 402 / 39 in the third run; "24 of 24" means a frame, not a usable page (8 pages built 821 px or less). Renderer RSS median 112 MB; three pages at or above 500 MB (up to 2.23 GB). "Lightweight" holds for document pages and fails for script-heavy pages.
6. **Language conformance.** 29,203 of 53,607 executed ECMAScript conformance tests pass (54.5 %); two later rounds 55.33 % and 56.85 %. A 106-check platform probe passes 106 of 106 on eight saved gate builds.
7. **Contrast by arithmetic.** Dark glass at alpha 0.58 over white gave the third text tone 1.02 : 1; glass alpha 0.88 with re-chosen tones gives every text colour at least 4.5 : 1 against the worst backdrop.
8. **A design that failed, and its remedy.** The first design was rejected by the author ("the opposite of what we are looking for") because it was limited to what the renderer could already draw. The remedy is a procedure: design the target in full, then turn each capability it needs into engine work. The second design asked for eleven engine capabilities; nine were built in the engine's paint module (rounded boxes by signed distance, closed-form Gaussian box shadows, gradients, backdrop blur, SVG strokes and others), each gated so that layout does not move; transforms and keyframes (compositor work) were not built.

## 2. How the engine is used
omni-ui mounts the omni-render source at a pinned commit (`.engine/REV`; `omni-ui --engine-rev` prints it), so a measurement names the engine it ran on. Before main moves to a new pin, the exact commit is gated: layout baseline unchanged (24,032 of 28,040 boxes agree with Chrome, 85.7 %), compiled and interpreted knob lookups identical (2,866,688), binary gate 0 lost, JavaScript probe 106 of 106, connectivity 13 of 13.

## 3. Open / unmeasured
Paint error against Chrome, frame times, screen-reader sessions and the keyboard-form gate were logged but their results were not retained, so no number is claimed. The video site's music front end was not made usable. Transforms and keyframes are not built.

## 4. Provenance (borrowed / ours)
**Borrowed:** the AccessKit accessibility schema; Landlock and seccomp-BPF as kernel mechanisms; the broker-and-renderer process split as a published architecture idea; WCAG contrast ratio; ECMAScript conformance tests as the measuring stick. **Ours:** the packed widget tree and its two emitters, the single `act` entry, the framed pipe, the contrast procedure, the memory and connectivity benches, the pinned-engine gate. No browser source was read.
