// SPDX-License-Identifier: MIT OR Apache-2.0
// The engine pin is part of the build (PROCEDURES "When releasing an engine pin", author 2026-10-07): a new `.engine/REV`
// re-runs this script, the changed OMNI_ENGINE_REV recompiles the crate (archived engine files keep their old mtimes, so
// cargo would otherwise keep the previous binary), and `omni-ui --engine-rev` reports the revision the binary was built from.
fn main() {
    println!("cargo:rerun-if-changed=.engine/REV");
    let rev = std::fs::read_to_string(".engine/REV").map(|s| s.trim().to_string()).unwrap_or_else(|_| "unpinned".into());
    println!("cargo:rustc-env=OMNI_ENGINE_REV={rev}");
}
