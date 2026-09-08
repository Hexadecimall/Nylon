#pragma once

#include <QKeySequence>
#include <QList>
#include <QString>

class QAction;

namespace nylon {

// Persistent shortcut overrides keyed by action object name. Defaults are
// whatever the action was created with; an override replaces it and is
// stored in the application settings.
class Shortcuts {
public:
    static const char* settingsGroup();
    // Applies stored overrides to `actions` and remembers their defaults.
    static void apply(const QList<QAction*>& actions);
    // Records the default an action was created with (called by apply).
    static QKeySequence defaultFor(const QAction* action);
    // Stores and applies an override. An empty sequence removes the key.
    static void setOverride(QAction* action, const QKeySequence& sequence);
    // Removes every override and restores defaults on `actions`.
    static void resetAll(const QList<QAction*>& actions);
    // The action among `actions` (other than `except`) already using
    // `sequence`, or null.
    static QAction* conflict(const QList<QAction*>& actions, const QKeySequence& sequence, const QAction* except);
};

} // namespace nylon
