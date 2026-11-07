use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Match {
    pub message: String,
    pub replacements: Vec<Replacement>,
    pub offset: i32,
    pub length: i32,
    pub rule: Rule,
}

#[derive(Debug, Deserialize)]
pub struct Replacement {
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct Rule {
    pub id: String,
    pub category: Category,
}

#[derive(Debug, Deserialize)]
pub struct Category {
    pub id: String,
}
