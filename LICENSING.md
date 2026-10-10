# LICENSING — omni-ui

`MIT OR Apache-2.0` for all source in this repository (`LICENSE-MIT`, `LICENSE-APACHE`); disclosure text and documentation CC BY 4.0.

The engine omni-ui draws with is mounted from an omni-render checkout at a pinned revision (`tools/pin-engine.sh`, `.engine/REV`) and keeps
omni-render's licences (`omni-render/LICENSING.md`). A default build mounts only omni-render's MIT OR Apache-2.0 files plus its no-op stand-in
for the T25 core. `cargo build --release --features t25` additionally builds `omni-ui-t25`, a separate binary that mounts the T25 core and the
T25 app (`src/t25_app.rs`); those files are `PolyForm-Small-Business-1.0.0 OR PolyForm-Noncommercial-1.0.0` and that binary is distributed under
those terms (texts in omni-render: `LICENSE.md`, `LICENSE-NONCOMMERCIAL.md`).
