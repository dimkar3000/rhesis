use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recommendation {
    pub range: Range,
    pub value: String,
    pub color: String,
    pub rule_id: String,
    pub category_id: String,
    pub tooltip: String,
    pub language: String,
}

/// Half-open text range the recommendation applies to.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: i32,
    pub length: i32,
}
