use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QVariant};
use rhesis::interop::recommendation::{Range, Recommendation};

fn sample_recommendation() -> Recommendation {
    Recommendation {
        range: Range {
            start: 4,
            length: 3,
        },
        value: QString::from("the"),
        color: QString::from("#112233"),
        rule_id: QString::from("RULE_1"),
        category_id: QString::from("TYPOS"),
        tooltip: QString::from("Possible spelling mistake"),
        language: QString::from("EN-US"),
    }
}

#[test]
fn converts_to_qml_variant_map() {
    let recommendation = sample_recommendation();

    let variant = QVariant::from(&recommendation);
    let map = variant
        .value::<QMap<QMapPair_QString_QVariant>>()
        .expect("conversion must produce a map");

    let string_field = |key: &str| -> String {
        map.get(&QString::from(key))
            .and_then(|value| value.value::<QString>())
            .unwrap_or_default()
            .to_string()
    };

    assert_eq!(string_field("value"), "the");
    assert_eq!(string_field("color"), "#112233");
    assert_eq!(string_field("rule_id"), "RULE_1");
    assert_eq!(string_field("category_id"), "TYPOS");
    assert_eq!(string_field("tooltip"), "Possible spelling mistake");
    assert_eq!(string_field("language"), "EN-US");

    let start = map
        .get(&QString::from("range_start"))
        .and_then(|value| value.value::<i32>())
        .expect("range_start must be an integer");
    let length = map
        .get(&QString::from("range_length"))
        .and_then(|value| value.value::<i32>())
        .expect("range_length must be an integer");
    assert_eq!(start, 4);
    assert_eq!(length, 3);
}

#[test]
fn all_fields_are_exposed() {
    let variant = QVariant::from(&sample_recommendation());
    let map = variant
        .value::<QMap<QMapPair_QString_QVariant>>()
        .expect("conversion must produce a map");

    let expected_keys = [
        "value",
        "tooltip",
        "color",
        "rule_id",
        "category_id",
        "language",
        "range_start",
        "range_length",
    ];
    let mut seen: Vec<String> = map.iter().map(|(key, _)| key.to_string()).collect();
    seen.sort();

    let mut expected: Vec<String> = expected_keys.iter().map(|k| k.to_string()).collect();
    expected.sort();

    assert_eq!(seen, expected);
}

#[test]
fn defaults_are_zero_value() {
    let recommendation = Recommendation::default();
    assert_eq!(recommendation.range, Range::default());
    assert_eq!(recommendation.value.to_string(), "");
    assert_eq!(recommendation.range.start, 0);
    assert_eq!(recommendation.range.length, 0);
}
