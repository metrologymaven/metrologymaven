// Synthetic fixture (CC-BY-4.0): resolves colours and spacing by token name only.
pub struct ThemeTokens;

impl ThemeTokens {
    pub fn focus_ring(&self) -> &'static str {
        "sem.focus.ring"
    }

    pub fn gap(&self) -> &'static str {
        "space.2"
    }

    pub fn transition(&self) -> &'static str {
        "motion.fast"
    }
}
