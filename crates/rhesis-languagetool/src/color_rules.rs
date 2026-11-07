use std::collections::HashMap;

#[derive(Default, Debug)]
pub(crate) struct ColorRules {
    by_rule: HashMap<String, String>,
    by_category: HashMap<String, String>,
}

impl ColorRules {
    /// Create a ColorRules object by a list of rules and categories. Rules are prefixed with `RULE:` and categories are prefixed `CATEGORY:`
    pub(crate) fn new(rules: Vec<(String, String)>) -> Self {
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

    /// Get a color by rule or category. Rule has precedence if both are provided.
    pub(crate) fn get_color(&self, rule: Option<&str>, category: Option<&str>) -> Option<&str> {
        rule.and_then(|rule| self.by_rule.get(rule))
            .or_else(|| category.and_then(|category| self.by_category.get(category)))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Little builder mirroring the wire shape (`RULE:` / `CATEGORY:` keys).
    fn wire(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(key, color)| (key.to_string(), color.to_string()))
            .collect()
    }

    #[test]
    fn rule_color_still_wins_when_both_are_given() {
        let rules = ColorRules::new(wire(&[
            ("RULE:HE_LIKE", "#00AA00"),
            ("CATEGORY:GRAMMAR", "#0000AA"),
        ]));

        // Both given: the specific rule override wins.
        assert_eq!(
            rules.get_color(Some("HE_LIKE"), Some("GRAMMAR")),
            Some("#00AA00")
        );
        // ...and a rule without its own entry falls back to its category.
        assert_eq!(
            rules.get_color(Some("UNKNOWN_RULE"), Some("GRAMMAR")),
            Some("#0000AA")
        );
        // No table hit at all: the caller keeps its default red.
        assert_eq!(rules.get_color(Some("UNKNOWN"), Some("UNKNOWN")), None);
    }

    #[test]
    fn one_side_searches_hit_only_that_table() {
        let rules = ColorRules::new(wire(&[
            ("RULE:HE_LIKE", "#00AA00"),
            ("CATEGORY:GRAMMAR", "#0000AA"),
        ]));

        // Search by rule: pass None for the category — rule table only.
        assert_eq!(rules.get_color(Some("HE_LIKE"), None), Some("#00AA00"));
        assert_eq!(rules.get_color(Some("UNKNOWN_RULE"), None), None);

        // Search by category: pass None for the rule — category table only.
        assert_eq!(rules.get_color(None, Some("GRAMMAR")), Some("#0000AA"));
        assert_eq!(rules.get_color(None, Some("UNKNOWN_CATEGORY")), None);

        // Nothing to search at all: no color either way.
        assert_eq!(rules.get_color(None, None), None);
    }

    #[test]
    fn unknown_prefixes_are_dropped() {
        let rules = ColorRules::new(wire(&[
            ("RULE:HE_LIKE", "#00AA00"),
            ("TYPO:HE_LIKE", "#FF00FF"),
            ("", "no color"),
        ]));

        assert_eq!(rules.get_color(Some("HE_LIKE"), None), Some("#00AA00"));
        // The TYPO-keyed entry and the prefixless one were never indexed,
        // so no search shape can reach them.
        assert_eq!(rules.get_color(Some("TYPO:HE_LIKE"), None), None);
        assert_eq!(rules.get_color(None, Some("TYPO:HE_LIKE")), None);
    }
}
