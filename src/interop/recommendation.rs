use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QVariant};

#[derive(Default, Debug, Clone, PartialEq)]
#[repr(C)]
pub struct Recommendation {
    pub range: Range,
    pub value: QString,
    pub color: QString,
    pub rule_id: QString,
    pub category_id: QString,
    pub tooltip: QString,
    pub language: QString,
}

#[derive(Default, Debug, Clone, PartialEq, Copy)]
#[repr(C)]
pub struct Range {
    pub start: i32,
    pub length: i32,
}

// Implementing this convert the type to some compatible with qml.
impl From<&Recommendation> for QVariant {
    fn from(r: &Recommendation) -> Self {
        let mut map = QMap::<QMapPair_QString_QVariant>::default();
        map.insert(QString::from("value"), QVariant::from(&r.value));
        map.insert(QString::from("tooltip"), QVariant::from(&r.tooltip));
        map.insert(QString::from("color"), QVariant::from(&r.color));
        map.insert(QString::from("rule_id"), QVariant::from(&r.rule_id));
        map.insert(QString::from("category_id"), QVariant::from(&r.category_id));
        map.insert(QString::from("language"), QVariant::from(&r.language));
        map.insert(QString::from("range_start"), QVariant::from(&r.range.start));
        map.insert(
            QString::from("range_length"),
            QVariant::from(&r.range.length),
        );
        QVariant::from(&map)
    }
}
