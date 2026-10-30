// Boot-harness probes for the Rust-driven full-boot E2E (tests/e2e/ui_boot.rs).
// Compiled unconditionally by build.rs (see the QuickTest block there),
// sharing the same build block as the QuickTest wrapper. Plain extern "C"
// symbols keep the probe surface writable from Rust without touching the
// production bridge; the test owns one QQmlApplicationEngine created here.
//
// Probes evaluate QML expressions in the loaded root document's scope —
// which is where Root.qml's ids (`messagingHelper`, `highlighter`,
// `appSettings`) are visible — and report failures through a shared detail
// string so Rust assertion messages name the expression that broke.

#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQmlError>
#include <QQmlExpression>
#include <QQmlProperty>
#include <QQuickWindow>
#include <QUrl>
#include <QImage>
#include <QVariant>
#include <QtQml/qqml.h>
#include <cstring>

namespace {

// One harness engine for the process; the boot test creates exactly one.
QQmlApplicationEngine* bootEngine = nullptr;

// Everything the engine complains about while loading Root.qml. QML errors
// surface through signals (not return codes), so this is the only way the
// Rust side can name what actually failed.
QString loadProblems;

// Failure detail of the most recent probe call, read back by the Rust side
// through rhesis_boot_last_error.
char lastProbeError[1024] = "";

void recordError(const QString& message) {
    const QByteArray utf8 = message.toUtf8();
    std::strncpy(lastProbeError, utf8.constData(), sizeof(lastProbeError) - 1);
    // strncpy pads with NULs or stops early; either way the last byte is now
    // the terminator.
    lastProbeError[sizeof(lastProbeError) - 1] = '\0';
}

void appendLoadProblems(const QList<QQmlError>& errors) {
    for (const QQmlError& error : errors) {
        loadProblems += error.toString();
        loadProblems += QLatin1Char('\n');
    }
}

void appendCreationFailure(const QUrl& url) {
    loadProblems += QStringLiteral("object creation failed: ");
    loadProblems += url.toString();
    loadProblems += QLatin1Char('\n');
}

QObject* asObject(void* pointer) {
    return static_cast<QObject*>(pointer);
}

}  // namespace

extern "C" {

// Creates the harness engine (once) and hooks the reporting signals. The
// Rust side must call rhesis_boot_engine_delete before dropping the
// QApplication, so the QML teardown — and with it the worker Kill — runs
// while Qt is still alive.
void* rhesis_boot_engine_new() {
    bootEngine = new QQmlApplicationEngine;
    loadProblems.clear();
    // Capture-less receivers keep this a plain static-callback connect.
    QObject::connect(bootEngine, &QQmlEngine::warnings, &appendLoadProblems);
    QObject::connect(
        bootEngine,
        &QQmlApplicationEngine::objectCreationFailed,
        &appendCreationFailure);
    return bootEngine;
}

// Loads the root document and returns the first root object (the Kirigami
// application window), or null when the load produced errors; the collected
// engine warnings are reported through rhesis_boot_last_error.
void* rhesis_boot_engine_load(void* engine, const char* url) {
    QQmlApplicationEngine* engineHandle =
        static_cast<QQmlApplicationEngine*>(engine);
    if (!engineHandle) {
        recordError(QStringLiteral("the engine pointer is null"));
        return nullptr;
    }
    loadProblems.clear();
    engineHandle->load(QUrl(QString::fromUtf8(url)));
    if (engineHandle->rootObjects().isEmpty()) {
        if (loadProblems.isEmpty()) {
            loadProblems = QStringLiteral("no root object and no recorded warnings");
        }
        recordError(loadProblems);
        return nullptr;
    }
    // Warnings co-existing with a created root object stay recorded in
    // loadProblems as diagnostics; they do not fail the load itself.
    return engineHandle->rootObjects().first();
}

void rhesis_boot_engine_delete(void* engine) {
    QQmlApplicationEngine* engineHandle =
        static_cast<QQmlApplicationEngine*>(engine);
    if (!engineHandle) {
        return;
    }
    delete engineHandle;
    if (bootEngine == engineHandle) {
        bootEngine = nullptr;
    }
}

// First descendant (recursively) whose objectName matches. A miss is a valid
// answer rather than a probe failure, so the error string stays untouched.
void* rhesis_boot_find_by_object_name(void* root, const char* name) {
    return asObject(root)->findChild<QObject*>(QString::fromUtf8(name));
}

// Expression evaluation happens in the object's component scope, so
// Root.qml ids are visible when the scope object is the loaded window.
bool rhesis_boot_eval_bool(void* object, const char* expr, bool* out) {
    QObject* obj = asObject(object);
    QQmlExpression expression(qmlContext(obj), obj, QString::fromUtf8(expr));
    if (expression.hasError()) {
        recordError(expression.error().description());
        return false;
    }
    *out = expression.evaluate().toBool();
    return true;
}

bool rhesis_boot_eval_int(void* object, const char* expr, int* out) {
    QObject* obj = asObject(object);
    QQmlExpression expression(qmlContext(obj), obj, QString::fromUtf8(expr));
    if (expression.hasError()) {
        recordError(expression.error().description());
        return false;
    }
    bool ok = false;
    *out = expression.evaluate().toInt(&ok);
    return ok;
}

// The result text is copied into the caller-owned buffer; an oversized
// result fails the probe instead of truncating silently.
bool rhesis_boot_eval_string(void* object, const char* expr, char* out, size_t cap) {
    QObject* obj = asObject(object);
    QQmlExpression expression(qmlContext(obj), obj, QString::fromUtf8(expr));
    if (expression.hasError()) {
        recordError(expression.error().description());
        return false;
    }
    const QByteArray utf8 = expression.evaluate().toString().toUtf8();
    if (static_cast<size_t>(utf8.size()) + 1 > cap) {
        recordError(QStringLiteral(
            "the probe result string does not fit into the caller's buffer"));
        return false;
    }
    std::memcpy(out, utf8.constData(), utf8.size() + 1);
    return true;
}

// Writes a property value through QQmlProperty, so assignments reach
// production handlers exactly like QML-side writes do (onTextChanged fires
// for a TextArea text write).
bool rhesis_boot_write_string(void* object, const char* name, const char* value) {
    QObject* obj = asObject(object);
    const QQmlProperty property(obj, QString::fromUtf8(name), qmlContext(obj));
    if (!property.isValid()) {
        recordError(QStringLiteral("no such property at this object"));
        return false;
    }
    return property.write(QVariant(QString::fromUtf8(value)));
}

// Best-effort evidence capture: renders the root window and saves it. A
// failure here reports a probe error, but the boot test treats a null grab
// as diagnostic-only, not as a behavior contract.
bool rhesis_boot_grab_window(void* root, const char* path) {
    QQuickWindow* window = qobject_cast<QQuickWindow*>(asObject(root));
    if (!window) {
        recordError(QStringLiteral("the loaded root is not a QQuickWindow"));
        return false;
    }
    const QImage grabbed = window->grabWindow();
    if (grabbed.isNull()) {
        recordError(QStringLiteral("grabWindow produced a null image"));
        return false;
    }
    return grabbed.save(QString::fromUtf8(path));
}

const char* rhesis_boot_last_error() {
    return lastProbeError;
}

}  // extern "C"
