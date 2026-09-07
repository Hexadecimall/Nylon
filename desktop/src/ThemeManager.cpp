#include "ThemeManager.h"

#include <QDir>
#include <QFileInfo>
#include <QGuiApplication>
#include <QStandardPaths>
#include <QTimer>

// The resource file is compiled into a static library, so the linker only
// keeps its initializer when something references it explicitly. The
// declaration Q_INIT_RESOURCE expands to must sit at global scope.
static void initThemeResources()
{
    Q_INIT_RESOURCE(themes);
}

namespace nylon {

ThemeManager::ThemeManager(QObject* parent)
    : QObject(parent)
{
    initThemeResources();
    connect(&m_watcher, &QFileSystemWatcher::fileChanged, this, &ThemeManager::onFileChanged);
}

QStringList ThemeManager::builtinNames()
{
    QStringList names;
    const QDir dir(QStringLiteral(":/themes"));
    const QStringList files = dir.entryList({QStringLiteral("*.theme")}, QDir::Files, QDir::Name);
    for (const QString& f : files) {
        names.append(QFileInfo(f).completeBaseName());
    }
    return names;
}

QString ThemeManager::userThemeDirectory()
{
    return QStandardPaths::writableLocation(QStandardPaths::AppConfigLocation)
        + QStringLiteral("/themes");
}

bool ThemeManager::load(const QString& name)
{
    const QString lower = name.toLower();
    const QString userPath = userThemeDirectory() + QLatin1Char('/') + lower + QStringLiteral(".theme");
    if (QFileInfo::exists(userPath)) {
        if (loadFrom(lower, userPath, true)) {
            return true;
        }
        // Fall through to the built-in copy so a broken override does not
        // leave the application without a theme.
    }
    const QString builtin = QStringLiteral(":/themes/") + lower + QStringLiteral(".theme");
    if (!QFileInfo::exists(builtin)) {
        m_errors = {QStringLiteral("no theme named '%1'").arg(name)};
        emit loadFailed(name, m_errors);
        return false;
    }
    return loadFrom(lower, builtin, false);
}

bool ThemeManager::reload()
{
    if (m_name.isEmpty()) {
        return false;
    }
    return load(m_name);
}

bool ThemeManager::loadFrom(const QString& name, const QString& path, bool fromUser)
{
    QStringList errors;
    Theme theme = Theme::fromFile(path, &errors);
    const QStringList missing = theme.missingKeys();
    for (const QString& key : missing) {
        errors.append(QStringLiteral("missing required '%1'").arg(key));
    }
    if (!errors.isEmpty()) {
        m_errors = errors;
        emit loadFailed(name, errors);
        return false;
    }
    m_theme = std::move(theme);
    m_name = name;
    m_path = path;
    m_fromUser = fromUser;
    m_errors.clear();
    watch(fromUser ? path : QString());
    // Install the font before listeners build widgets so nothing is laid
    // out with the platform placeholder font.
    if (qobject_cast<QGuiApplication*>(QCoreApplication::instance())) {
        QGuiApplication::setFont(m_theme.resolvedFont());
    }
    emit themeChanged(m_theme);
    return true;
}

void ThemeManager::watch(const QString& path)
{
    const QStringList current = m_watcher.files();
    if (!current.isEmpty()) {
        m_watcher.removePaths(current);
    }
    if (!path.isEmpty()) {
        m_watcher.addPath(path);
    }
}

void ThemeManager::onFileChanged(const QString& path)
{
    // Editors commonly replace the file rather than write in place, which
    // drops the watch. Re-arm after a short delay so the new inode is seen
    // and a half-written file is not parsed.
    QTimer::singleShot(100, this, [this, path] {
        if (QFileInfo::exists(path)) {
            loadFrom(m_name, path, true);
            watch(path);
        } else {
            reload();
        }
    });
}

} // namespace nylon
