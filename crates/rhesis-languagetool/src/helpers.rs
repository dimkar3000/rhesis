use std::collections::HashMap;

pub fn normalize_address(address: &str) -> String {
    let address = address.trim().trim_end_matches('/');
    if address.contains("://") {
        address.to_string()
    } else {
        format!("http://{address}")
    }
}

/// The user's recolor rules: a `RULE:<id>` / `CATEGORY:<id>` → color map as
/// it arrives from settings, indexed once on load so that mapping a response
/// looks colors up directly instead of re-formatting prefixed keys per match.
#[derive(Default, Debug)]
pub(crate) struct ColorRules {
    by_rule: HashMap<String, String>,
    by_category: HashMap<String, String>,
}

impl ColorRules {
    /// Split the wire form (`Vec<(String, String)>` with prefixed keys) into
    /// the two lookup tables; keys with an unknown prefix are dropped, as
    /// they could never have been matched by the old `format!` lookups.
    pub(crate) fn from_wire(rules: Vec<(String, String)>) -> Self {
        let mut parsed = ColorRules::default();
        for (key, color) in rules {
            if let Some(id) = key.strip_prefix("RULE:") {
                parsed.by_rule.insert(id.to_string(), color);
            } else if let Some(id) = key.strip_prefix("CATEGORY:") {
                parsed.by_category.insert(id.to_string(), color);
            }
        }
        parsed
    }

    /// Color for one match's rule: a specific rule override wins over its
    /// category's color (the pre-`ColorRules` code applied the category and
    /// then overwrote it with the rule), `None` = default red.
    pub(crate) fn color(&self, rule: &str, category: &str) -> Option<&str> {
        self.by_rule
            .get(rule)
            .or_else(|| self.by_category.get(category))
            .map(|color| color.as_str())
    }
}

mod unit_tests {
    #[test]
    fn normalize_address_keeps_and_adds_schemes() {
        assert_eq!(
            super::normalize_address(" http://localhost:2689/ "),
            "http://localhost:2689"
        );
        assert_eq!(
            super::normalize_address("localhost:2689//"),
            "http://localhost:2689"
        );
        assert_eq!(
            super::normalize_address("https://lt.example.com"),
            "https://lt.example.com"
        );
        assert_eq!(super::normalize_address("unix:"), "http://unix:");
    }
}
