// SPDX-License-Identifier: MIT OR Apache-2.0
//! The two load-time hooks page.rs calls from the engine's doctor (`crate::repair::doctor::fix`), without mounting the doctor:
//! a site's fix sheet from OMR_FIXES/<slug>.css (doctor-core P2, applied last in cascade order). Route-2 stored geometry
//! (<slug>.geo.json) is not armed in the browser yet. Same slug as the engine's doctor_fix.rs::slug.
pub mod doctor {
    pub mod fix {
        pub fn slug(url: &str) -> String {
            let s: String = url.trim_start_matches("https://").trim_start_matches("http://").chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
            s.trim_matches('-').chars().take(80).collect()
        }
        pub fn sheet_for(url: &str) -> Option<String> {
            let d = std::env::var("OMR_FIXES").ok()?;
            std::fs::read_to_string(std::path::Path::new(&d).join(format!("{}.css", slug(url)))).ok()
        }
        pub fn arm_geo_for(_url: &str, _dom: &crate::dom::Dom) {}
    }
}
