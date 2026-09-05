#pragma once
#include <QColor>
#include <QTextCharFormat>
#include <QTextCursor>
#include <QTextDocument>
#include <QApplication>
#include <QPalette>
#include <QIcon>
#include <QSettings>
#include <QTranslator>
#include <QFile>
#include <QVariant>
#include <QVariantMap>
#include <QVariantList>
#include <QCoreApplication>
#include <QDebug>
#include <QLocale>
#include <QStringList>
#include <QtQml/QQmlEngine>
#include <QtQml/qqml.h>
#include <memory>

inline std::unique_ptr<QTextCharFormat> newQTextCharFormat() noexcept {
    return std::make_unique<QTextCharFormat>();
}

inline std::unique_ptr<QTextCharFormat> newUnderlinedFormat(const QString& colorName) noexcept {
    auto fmt = std::make_unique<QTextCharFormat>();
    QColor textColor = QGuiApplication::palette().color(QPalette::Text);
    fmt->setForeground(QBrush(textColor));
    fmt->setFontUnderline(true);
    fmt->setUnderlineColor(QColor(colorName));
    return fmt;
}

// Takes a reference (not a pointer) so the Rust bridge declaration stays a
// safe function: the raw pointer is validated once at the call site.
inline void replaceTextInDocument(QTextDocument& doc, int64_t start, int64_t end, const QString& replacement) {
    QTextCursor cursor(&doc);
    cursor.setPosition(start);
    cursor.setPosition(end, QTextCursor::KeepAnchor);
    cursor.insertText(replacement);
}

inline void appSetWindowIcon(QApplication& app, const QString& path) {
  app.setWindowIcon(QIcon(path));
}

inline void setupIconTheme() {
    QStringList paths = QIcon::themeSearchPaths();
    paths.prepend(":/icons");
    QIcon::setThemeSearchPaths(paths);
}

inline QStringList translationCandidates(const QString& preferred) {
    // Most specific first (e.g. "el_GR", then "el"). An empty preferred
    // locale follows the system UI languages. English is always the last
    // resort: the source language beats raw qsTr() keys.
    QStringList seeds;
    if (preferred.isEmpty()) {
        seeds = QLocale::system().uiLanguages();
    } else {
        seeds << preferred;
    }

    QStringList candidates;
    for (const QString& language : seeds) {
        QString normalized = language;
        normalized.replace('-', '_');
        candidates << normalized;
        const int sep = normalized.indexOf('_');
        if (sep > 0) {
            candidates << normalized.left(sep);
        }
    }
    candidates << QStringLiteral("en_US");
    candidates.removeDuplicates();
    return candidates;
}

inline bool applyLanguage(const QString& translationsDir, const QString& preferredLocale) {
    QObject* app = QCoreApplication::instance();
    if (!app) {
        qWarning() << "No application instance; cannot install translations";
        return false;
    }

    // qInfo() << "System locale:" << QLocale::system().name()
    //         << ", preferred:" << (preferredLocale.isEmpty() ? QStringLiteral("<system>") : preferredLocale)
    //         << ", looking for translations in:" << translationsDir;

    // Drop previously installed translators. They are parented to the
    // application, so they are discovered here instead of kept in a static.
    const QList<QTranslator*> oldTranslators = app->findChildren<QTranslator*>();
    for (QTranslator* old : oldTranslators) {
        QCoreApplication::removeTranslator(old);
        delete old;
    }

    for (const QString& candidate : translationCandidates(preferredLocale)) {
        const QString filePath = translationsDir + "/rhesis_" + candidate;
        auto* translator = new QTranslator(app);
        if (translator->load(filePath)) {
            QCoreApplication::installTranslator(translator);
            // qInfo() << "Installed translation file:" << filePath;
            return true;
        }
        // qInfo() << "Translation file not found:" << filePath;
        delete translator;
    }

    qWarning() << "Failed to install any translation from" << translationsDir;
    return false;
}

// Templated so callers don't need to name the generated peer type: any
// QObject-derived object (e.g. a QML-instantiated helper) resolves the
// engine that owns it, with no static state anywhere.
template <typename T>
inline void retranslateForObject(T& object) {
    if (QQmlEngine* engine = qmlEngine(&object)) {
        engine->retranslate();
    } else {
        qWarning() << "No QML engine for object; translation applies on next load";
    }
}
