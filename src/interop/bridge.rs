#[cxx_qt::bridge]
pub mod ffi {

    #[derive(Default, Debug, Clone, PartialEq)]
    pub struct Recommendation {
        range: Range,
        value: String,
        color: String,
    }

    #[derive(Default, Debug, Clone, PartialEq, Copy)]
    pub struct Range {
        start: i32,
        length: i32,
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qlist.h");
        type QList_QString = cxx_qt_lib::QList<QString>;

        include!("cxx-qt-lib/qlist.h");
        type QList_i32 = cxx_qt_lib::QList<i32>;
        type QGuiApplication = cxx_qt_lib::QGuiApplication;

        include!("cxx-qt-lib-extras/gui/qapplication.h");
        type QApplication = cxx_qt_lib_extras::QApplication;
    }

    unsafe extern "C++Qt" {
        include!(<QtGui/QSyntaxHighlighter>);
        #[qobject]
        type QSyntaxHighlighter;
    }

    unsafe extern "C++Qt" {
        include!(<QtQuick/QQuickTextDocument>);
        #[qobject]
        type QQuickTextDocument;
    }

    unsafe extern "C++" {
        fn textDocument(self: &QQuickTextDocument) -> *mut QTextDocument;
    }

    unsafe extern "C++" {
        include!("helper.h");
        include!(<QtGui/QTextCharFormat>);
        type QTextCharFormat;
        fn newUnderlinedFormat(colorName: &QString) -> UniquePtr<QTextCharFormat>;

        unsafe fn replaceTextInDocument(
            doc: *mut QTextDocument,
            start: i64,
            end: i64,
            replacement: &QString,
        );

        fn appSetWindowIcon(app: Pin<&mut QApplication>, path: &QString);

        fn setupIconTheme();
    }

    unsafe extern "C++" {
        include!(<QtGui/QTextDocument>);
        type QTextDocument;
    }

    extern "RustQt" {
        #[qobject]
        #[base = QSyntaxHighlighter]
        #[qml_element]
        type CustomHighlighter = super::CustomHighlighterRust;

        #[qobject]
        #[qml_element]
        type AsyncMessagingHelper = super::AsyncMessagingHelperRust;

        #[qinvokable]
        fn restart(
            self: Pin<&mut AsyncMessagingHelper>,
            embedded: bool,
            address: &QString,
        );

        #[qinvokable]
        fn text_area_changed(self: Pin<&mut AsyncMessagingHelper>, text: QString);

    }

    impl cxx_qt::Threading for AsyncMessagingHelper {}
    impl cxx_qt::Threading for CustomHighlighter {}

    extern "RustQt" {
        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "highlightBlock"]
        fn highlight_block(self: Pin<&mut CustomHighlighter>, text: &QString);

        #[qinvokable]
        #[cxx_name = "getSuggestions"]
        fn get_suggestions(
            self: Pin<&mut CustomHighlighter>,
            start: i32,
            length: i32,
        ) -> QList_QString;

        #[qinvokable]
        #[cxx_name = "findRecommendation"]
        fn find_recommendation(self: Pin<&mut CustomHighlighter>, pos: i32) -> QList_i32;

        #[qinvokable]
        #[cxx_name = "replaceWord"]
        fn replace_word(
            self: Pin<&mut CustomHighlighter>,
            start: i64,
            end: i64,
            replacement: &QString,
        );

        #[qinvokable]
        #[cxx_name = "startMessageThread"]
        unsafe fn start_message_thread(
            self: Pin<&mut CustomHighlighter>,
            helper: *mut AsyncMessagingHelper,
        );

        #[qinvokable]
        #[cxx_name = "setTextDocument"]
        unsafe fn set_text_document(
            self: Pin<&mut CustomHighlighter>,
            doc: *mut QQuickTextDocument,
        );
    }

    unsafe extern "RustQt" {
        #[inherit]
        #[cxx_name = "rehighlight"]
        fn rehighlight(self: Pin<&mut CustomHighlighter>);

        #[inherit]
        #[cxx_name = "setFormat"]
        fn set_format(
            self: Pin<&mut CustomHighlighter>,
            start: i32,
            length: i32,
            format: &QTextCharFormat,
        );

        #[inherit]
        #[cxx_name = "setDocument"]
        unsafe fn set_document(self: Pin<&mut CustomHighlighter>, doc: *mut QTextDocument);

        #[inherit]
        #[cxx_name = "document"]
        unsafe fn document(self: Pin<&mut CustomHighlighter>) -> *mut QTextDocument;
    }
}

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::pin::Pin;

use crate::interop::bridge::ffi::{newUnderlinedFormat, QList_i32};
use crate::languagetool::service::Message;

impl ffi::AsyncMessagingHelper {
    fn restart(self: Pin<&mut Self>, embedded: bool, address: &QString) {
        self.rust_mut()
            .restart(embedded, &address.to_string());
    }

    fn text_area_changed(self: Pin<&mut Self>, text: QString) {
        let _ = self.message_sender.send(Message(text));
    }
}

impl ffi::CustomHighlighter {
    pub fn highlight_block(mut self: Pin<&mut Self>, _text: &QString) {
        let ranges = self.as_mut().rust_mut().highlight_block();

        for (start, length, color) in ranges {
            let format = newUnderlinedFormat(&color);
            self.as_mut().set_format(start, length, &format);
        }
    }

    pub fn get_suggestions(self: Pin<&mut Self>, start: i32, length: i32) -> ffi::QList_QString {
        self.recommendations
            .iter()
            .filter(|r| r.range.start >= start && r.range.length <= length)
            .take(5)
            .map(|x| QString::from(&x.value))
            .collect::<ffi::QList_QString>()
    }

    pub fn replace_word(mut self: Pin<&mut Self>, start: i64, end: i64, replacement: &QString) {
        self.as_mut().rust_mut().recommendations.clear();
        unsafe {
            let doc = self.document();
            if !doc.is_null() {
                ffi::replaceTextInDocument(doc, start, end, replacement);
            }
        }
    }

    pub fn find_recommendation(self: Pin<&mut Self>, pos: i32) -> QList_i32 {
        for r in &self.recommendations {
            let start = r.range.start;
            let end = r.range.start + r.range.length;
            if pos >= start && pos < end {
                return QList_i32::from([start, end]);
            }
        }
        QList_i32::default()
    }

    pub fn set_text_document(self: Pin<&mut Self>, doc: *mut ffi::QQuickTextDocument) {
        let text_doc = unsafe { (*doc).textDocument() };
        unsafe { self.set_document(text_doc) };
    }

    pub fn start_message_thread(self: Pin<&mut Self>, helper: *mut ffi::AsyncMessagingHelper) {
        let helper = unsafe { &mut *helper };
        let qt_thread = self.qt_thread();
        self.rust_mut().start_message_thread(helper, qt_thread);
    }
}

pub(super) type AsyncMessagingHelperRust =
    crate::interop::async_messaging_helper::AsyncMessagingHelperRust;
pub(super) type CustomHighlighterRust = crate::interop::custom_highlighter::CustomHighlighterRust;
