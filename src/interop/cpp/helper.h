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

inline void replaceTextInDocument(QTextDocument* doc, int64_t start, int64_t end, const QString& replacement) {
    QTextCursor cursor(doc);
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

inline bool installTranslation(QApplication& app, const QString& translationsDir) {
    // 1. Create a translator object
    auto* translator = new QTranslator();

    // 2. Determine the system locale and attempt to load the correct .qm file
    // "myapp" is the base name of your .qm files (e.g., myapp_fr.qm)
    // The second overload handles the list of preferred languages correctly.
    const QString translationFile = QLocale::system().name();
    if (translator->load(translationsDir + "/rhesis_" + translationFile)) {
        app.installTranslator(translator); // 3. Install the translator
        return true;
    }

    return false;
}
