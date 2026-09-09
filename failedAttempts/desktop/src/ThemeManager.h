#pragma once

#include "Theme.h"

#include <QDateTime>
#include <QByteArray>
#include <QFileSystemWatcher>
#include <QObject>
#include <QStringList>
#include <QTimer>

namespace nylon {

// Owns the active theme. Built-in themes are compiled in from themes/.
// A file with the same name under the user's theme directory overrides the
// built-in copy and is reloaded whenever it changes on disk.
class ThemeManager : public QObject {
    Q_OBJECT
public:
    explicit ThemeManager(QObject* parent = nullptr);

    // Names of the compiled-in themes, lower case ("nylon", "slate", ...).
    static QStringList builtinNames();
    // Directory searched for user overrides (platform config location).
    static QString userThemeDirectory();

    const Theme& theme() const { return m_theme; }
    QString currentName() const { return m_name; }
    // True when the active theme came from the user directory.
    bool isUserOverride() const { return m_fromUser; }
    // Problems reported by the most recent load.
    QStringList lastErrors() const { return m_errors; }

    // Loads a theme by name. Returns false and keeps the current theme when
    // the file cannot be parsed or lacks required keys. On success the
    // theme's font becomes the application font and themeChanged fires.
    bool load(const QString& name);
    // Reloads the current theme from its source.
    bool reload();

signals:
    void themeChanged(const Theme& theme);
    void loadFailed(const QString& name, const QStringList& errors);

private:
    bool loadFrom(const QString& name, const QString& path, bool fromUser);
    void watch(const QString& path);
    void onFileChanged(const QString& path);

    Theme m_theme;
    QString m_name;
    QString m_path;
    bool m_fromUser = false;
    QStringList m_errors;
    QFileSystemWatcher m_watcher;
    // Watcher notifications are not delivered uniformly on every platform,
    // so an active override is also polled by content fingerprint.
    QTimer m_poll;
    QDateTime m_lastModified;
    qint64 m_lastSize = -1;
    QByteArray m_lastDigest;
    void pollOverride();
};

} // namespace nylon
