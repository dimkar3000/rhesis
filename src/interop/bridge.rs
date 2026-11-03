#[cxx_qt::bridge]
pub mod ffi {

    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qlist.h");
        type QList_QString = cxx_qt_lib::QList<QString>;

        include!("cxx-qt-lib/qlist.h");
        type QList_i32 = cxx_qt_lib::QList<i32>;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/core/qmap/qmap_QString_QVariant.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<QVariant>;

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

        /// Replace the text range inside a document with new content.
        /// Takes a pinned reference (not a raw pointer) so this stays a safe
        /// function; callers validate the pointer once when pinning it.
        fn replaceTextInDocument(
            doc: Pin<&mut QTextDocument>,
            start: i64,
            end: i64,
            replacement: &QString,
        );

        fn appSetWindowIcon(app: Pin<&mut QApplication>, path: &QString);

        fn setBackgroundBehavior(enabled: bool);

        fn appHideRootWindow();

        /// Show + raise + activate the main window; must be called on the
        /// Qt thread (the single-instance "show" queue guarantees that).
        fn appRaiseMainWindow();

        fn setupIconTheme();

        fn applyLanguage(dir: &QString, preferredLocale: &QString) -> bool;

        fn retranslateForObject(object: Pin<&mut LanguageManager>);
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
        // The client mirror of the daemon-owned state (Refactor.md §5):
        // status codes 0-5 are daemon-reported (WorkerStatus::code),
        // 6 = client-local Disconnected. The settings fields mirror the
        // wire snapshot (rhesis_protocol::Settings) field-for-field;
        // `default_port` is the factory hint. Written via the generated
        // setters from the fan-out (auto change signals), read everywhere.
        #[qproperty(i32, server_status)]
        #[qproperty(QString, server_status_reason)]
        #[qproperty(bool, embedded)]
        #[qproperty(i32, port)]
        #[qproperty(i32, default_port)]
        #[qproperty(QString, external_address)]
        #[qproperty(bool, background)]
        #[qproperty(bool, show_tooltips)]
        #[qproperty(bool, show_debug_tooltips)]
        #[qproperty(QString, language)]
        #[qproperty(QVariant, color_settings)]
        type AsyncMessagingHelper = super::AsyncMessagingHelperRust;

        /// Apply a full settings snapshot (the settings page's Apply).
        /// Parsed from the camelCase keys the page builds, echoed into the
        /// mirror immediately, and sent to the daemon as SetSettings.
        #[qinvokable]
        fn set_settings(self: Pin<&mut AsyncMessagingHelper>, map: QMap_QString_QVariant);

        /// Quit the daemon (tray "Quit service" / final UI quit without
        /// background hosting).
        #[qinvokable]
        fn quit_service(self: Pin<&mut AsyncMessagingHelper>);

        #[qinvokable]
        fn start_server(self: Pin<&mut AsyncMessagingHelper>);

        #[qinvokable]
        fn stop_server(self: Pin<&mut AsyncMessagingHelper>);

        #[qinvokable]
        fn retry_server(self: Pin<&mut AsyncMessagingHelper>);

        /// Whether an autostart-at-login entry currently exists.
        #[qinvokable]
        fn is_autostart(self: Pin<&mut AsyncMessagingHelper>) -> bool;

        /// Create or remove the autostart entry; returns whether it worked
        /// (sandbox permission failures surface as `false` here).
        #[qinvokable]
        fn set_autostart(self: Pin<&mut AsyncMessagingHelper>, enabled: bool) -> bool;

        #[qinvokable]
        fn text_area_changed(self: Pin<&mut AsyncMessagingHelper>, text: QString);
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type LanguageManager = super::LanguageManagerRust;

        #[qinvokable]
        fn apply_language(self: Pin<&mut LanguageManager>, code: &QString) -> bool;
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
        ) -> QList_QVariant;

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

use crate::connection::Command;
use crate::interop::async_messaging_helper::{color_settings_to_variant, variant_to_settings};
use crate::interop::bridge::ffi::{newUnderlinedFormat, QList_i32, QMap_QString_QVariant};
use rhesis_protocol::{Settings, DEFAULT_PORT};

/// The Qt-facing glue for the mirror object (its pure-information side —
/// qproperty fields, snapshot parsing, [`Drop`] — lives in
/// `async_messaging_helper.rs`, next to the connection-independent logic).
/// Every method here runs on the Qt GUI thread: the invokables from QML,
/// and `apply_settings`, which the fan-out queues from the connection
/// thread.
impl ffi::AsyncMessagingHelper {
    /// Apply a daemon snapshot to the mirror. Every qproperty goes through
    /// its generated setter, so QML's change signals fire; the background
    /// flag also reaches helper.h in the same step —
    /// `quitOnLastWindowClosed` must follow the snapshot the daemon
    /// persisted, not a local preference.
    pub(crate) fn apply_settings(mut self: Pin<&mut Self>, settings: &Settings) {
        self.as_mut().set_embedded(settings.embedded);
        self.as_mut().set_port(settings.port as i32);
        self.as_mut().set_default_port(DEFAULT_PORT as i32);
        self.as_mut()
            .set_external_address(QString::from(settings.external_address.as_str()));
        self.as_mut().set_background(settings.background);
        self.as_mut().set_show_tooltips(settings.show_tooltips);
        self.as_mut()
            .set_show_debug_tooltips(settings.show_debug_tooltips);
        self.as_mut()
            .set_language(QString::from(settings.language.as_str()));
        self.as_mut()
            .set_color_settings(color_settings_to_variant(&settings.color_settings));

        let enabled = settings.background;
        ffi::setBackgroundBehavior(enabled);
        log::info!(
            "snapshot applied: embedded={} port={} background={} language={:?}",
            settings.embedded,
            settings.port,
            settings.background,
            settings.language,
        );
    }

    /// Send a settings snapshot to the daemon and reflect it locally right
    /// away: the echo through the daemon (`Settings` frame) is a no-op
    /// mirror of what this just wrote, so the page never waits a round
    /// trip for its own Apply.
    fn set_settings(mut self: Pin<&mut Self>, map: QMap_QString_QVariant) {
        let settings = variant_to_settings(&map);
        log::info!("applying settings snapshot: {settings:?}");
        self.as_mut().apply_settings(&settings);
        if let Some(connection) = &self.rust().connection {
            connection.send(Command::SetSettings(settings));
        } else {
            log::warn!("settings snapshot dropped, no connection handle");
        }
    }

    fn start_server(self: Pin<&mut Self>) {
        log::info!("start_server requested");
        self.send_command(Command::Start);
    }

    fn stop_server(self: Pin<&mut Self>) {
        log::info!("stop_server requested");
        self.send_command(Command::Stop);
    }

    fn retry_server(self: Pin<&mut Self>) {
        log::info!("retry_server requested");
        self.send_command(Command::Retry);
    }

    /// A text edit landed: the debounce lives on the daemon side now, so
    /// every keystroke just goes onto the channel.
    fn text_area_changed(self: Pin<&mut Self>, text: QString) {
        self.send_command(Command::Check(text.to_string()));
    }

    /// Quit the daemon (tray menu / final UI quit): best-effort, fire and
    /// forget; the connection loop honors it as soon as it observes the
    /// command, connected or not.
    fn quit_service(self: Pin<&mut Self>) {
        log::info!("quit_service requested");
        self.send_command(Command::Shutdown);
    }

    /// Whether an autostart-at-login entry currently exists (local OS
    /// state; not part of the daemon's settings store).
    fn is_autostart(self: Pin<&mut Self>) -> bool {
        rhesis_platform::autostart::is_enabled()
    }

    /// Create or remove the autostart entry; returns whether it worked
    /// (sandbox permission failures surface as `false` here).
    fn set_autostart(self: Pin<&mut Self>, enabled: bool) -> bool {
        match rhesis_platform::autostart::set_enabled(enabled) {
            Ok(()) => true,
            Err(message) => {
                log::error!("set_autostart({enabled}) failed: {message}");
                false
            }
        }
    }

    /// One enqueue path: a dead loop is already logged by the Connection.
    fn send_command(self: Pin<&mut Self>, command: Command) {
        if let Some(connection) = &self.rust().connection {
            connection.send(command);
        } else {
            log::warn!("command dropped, no connection handle");
        }
    }
}

impl ffi::LanguageManager {
    fn apply_language(mut self: Pin<&mut Self>, code: &QString) -> bool {
        let dir = QString::from(self.as_mut().rust_mut().translations_dir.as_str());
        if !ffi::applyLanguage(&dir, code) {
            return false;
        }
        ffi::retranslateForObject(self);
        true
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

    pub fn get_suggestions(self: Pin<&mut Self>, start: i32, length: i32) -> ffi::QList_QVariant {
        use crate::interop::custom_highlighter::recommendation_to_variant;

        self.recommendations
            .iter()
            .filter(|r| r.range.start >= start && r.range.length <= length)
            .take(5)
            .map(recommendation_to_variant)
            .collect()
    }

    pub fn replace_word(mut self: Pin<&mut Self>, start: i64, end: i64, replacement: &QString) {
        log::debug!("replacing word at [{start}, {end}] with {replacement:?}");
        self.as_mut().rust_mut().recommendations.clear();
        // SAFETY: `document()` returns this highlighter's own live QTextDocument
        // (or null when detached). The null check plus Qt's object lifetime
        // guarantee make pinning sound here, on the GUI thread.
        let doc = unsafe { self.document() };
        if !doc.is_null() {
            let pinned = unsafe { Pin::new_unchecked(&mut *doc) };
            ffi::replaceTextInDocument(pinned, start, end, replacement);
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

    /// Attach the QML text document to the highlighter.
    ///
    /// # Safety
    ///
    /// `doc` must be a valid, non-null pointer to a live `QQuickTextDocument`.
    /// Must be called on the Qt GUI thread owning the document.
    pub unsafe fn set_text_document(self: Pin<&mut Self>, doc: *mut ffi::QQuickTextDocument) {
        let text_doc = unsafe { (*doc).textDocument() };
        unsafe { self.set_document(text_doc) };
    }

    /// Wire the highlighter to the messaging helper's threads.
    ///
    /// # Safety
    ///
    /// `helper` must be a valid, non-null pointer to a live
    /// `AsyncMessagingHelper` for the duration of this call. Must be called
    /// on the Qt thread that owns both objects.
    pub unsafe fn start_message_thread(
        self: Pin<&mut Self>,
        helper: *mut ffi::AsyncMessagingHelper,
    ) {
        let helper_pin = unsafe { Pin::new_unchecked(&mut *helper) };
        let helper_thread = helper_pin.qt_thread();
        let helper_rust = helper_pin.rust_mut();
        let highlighter_thread = self.qt_thread();
        self.rust_mut()
            .start_message_thread(helper_rust, helper_thread, highlighter_thread);
    }
}

pub(super) type AsyncMessagingHelperRust =
    crate::interop::async_messaging_helper::AsyncMessagingHelperRust;
pub(super) type CustomHighlighterRust = crate::interop::custom_highlighter::CustomHighlighterRust;
pub(super) type LanguageManagerRust = crate::interop::language::LanguageManagerRust;
