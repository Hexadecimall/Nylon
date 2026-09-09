#include "Shortcuts.h"

#include <QAction>
#include <QSettings>
#include <QVariant>

namespace nylon {

namespace {
const char* const kDefaultProperty = "nylonDefaultShortcut";
}

const char* Shortcuts::settingsGroup()
{
    return "shortcuts";
}

QKeySequence Shortcuts::defaultFor(const QAction* action)
{
    const QVariant v = action->property(kDefaultProperty);
    return v.isValid() ? v.value<QKeySequence>() : action->shortcut();
}

void Shortcuts::apply(const QList<QAction*>& actions)
{
    QSettings settings;
    settings.beginGroup(QLatin1String(settingsGroup()));
    for (QAction* a : actions) {
        if (a->objectName().isEmpty()) {
            continue;
        }
        if (!a->property(kDefaultProperty).isValid()) {
            a->setProperty(kDefaultProperty, QVariant::fromValue(a->shortcut()));
        }
        if (settings.contains(a->objectName())) {
            a->setShortcut(QKeySequence::fromString(settings.value(a->objectName()).toString(), QKeySequence::PortableText));
        } else {
            a->setShortcut(defaultFor(a));
        }
    }
    settings.endGroup();
}

void Shortcuts::setOverride(QAction* action, const QKeySequence& sequence)
{
    if (!action->property(kDefaultProperty).isValid()) {
        action->setProperty(kDefaultProperty, QVariant::fromValue(action->shortcut()));
    }
    QSettings settings;
    settings.beginGroup(QLatin1String(settingsGroup()));
    if (sequence == defaultFor(action)) {
        settings.remove(action->objectName());
    } else {
        settings.setValue(action->objectName(), sequence.toString(QKeySequence::PortableText));
    }
    settings.endGroup();
    action->setShortcut(sequence);
}

void Shortcuts::resetAll(const QList<QAction*>& actions)
{
    QSettings settings;
    settings.remove(QLatin1String(settingsGroup()));
    for (QAction* a : actions) {
        if (a->property(kDefaultProperty).isValid()) {
            a->setShortcut(defaultFor(a));
        }
    }
}

QAction* Shortcuts::conflict(const QList<QAction*>& actions, const QKeySequence& sequence, const QAction* except)
{
    if (sequence.isEmpty()) {
        return nullptr;
    }
    for (QAction* a : actions) {
        if (a != except && !a->shortcut().isEmpty() && a->shortcut() == sequence) {
            return a;
        }
    }
    return nullptr;
}

} // namespace nylon
