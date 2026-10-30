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

#[cfg(test)]
pub(crate) mod test_support {
    //! Shared recommendation fixtures for the sink/spy, worker-loop and
    //! provider suites, so every module's tests speak the same shape.

    use super::{Range, Recommendation};
    use cxx_qt_lib::QString;

    /// A one-word recommendation at `start` — the minimal suggestion shape
    /// the rendering pipeline consumes (a range to paint + a replacement).
    pub(crate) fn one_word(start: i32, value: &str) -> Recommendation {
        Recommendation {
            value: QString::from(value),
            range: Range { start, length: 1 },
            ..Default::default()
        }
    }
}
