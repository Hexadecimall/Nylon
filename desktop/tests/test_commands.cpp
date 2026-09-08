#include "CommandPalette.h"
#include "MainWindow.h"
#include "PreferencesDialog.h"
#include "ProjectBridge.h"
#include "Shortcuts.h"
#include "ThemeManager.h"

#include <QAction>
#include <QKeySequenceEdit>
#include <QLineEdit>
#include <QSettings>
#include <QStandardPaths>
#include <QTableWidget>
#include <QtTest>

using namespace nylon;

class TestCommands : public QObject {
    Q_OBJECT
private slots:
    void initTestCase();
    void fuzzyMatchAndScore();
    void paletteFiltersRanksAndTriggers();
    void paletteSkipsDisabledActions();
    void paletteOpensFromTheWindow();
    void shortcutOverridesPersistAndApply();
    void shortcutConflictsAreRefused();
    void preferencesShortcutPageEdits();

private:
    ThemeManager m_themes;
};

void TestCommands::initTestCase()
{
    QStandardPaths::setTestModeEnabled(true);
    QSettings().clear();
    QVERIFY(m_themes.load(QStringLiteral("nylon")));
}

void TestCommands::fuzzyMatchAndScore()
{
    QVERIFY(CommandPalette::fuzzyMatch(QStringLiteral("iat"), QStringLiteral("Insert Audio Track")));
    QVERIFY(CommandPalette::fuzzyMatch(QStringLiteral("INSERT"), QStringLiteral("insert audio track")));
    QVERIFY(!CommandPalette::fuzzyMatch(QStringLiteral("kct"), QStringLiteral("Insert Audio Track")));
    QVERIFY(CommandPalette::fuzzyMatch(QString(), QStringLiteral("anything")));
    QCOMPARE(CommandPalette::matchScore(QStringLiteral("ins"), QStringLiteral("Insert Audio Track")), 0);
    QVERIFY(CommandPalette::matchScore(QStringLiteral("audio"), QStringLiteral("Insert Audio Track")) > 0);
    QVERIFY(CommandPalette::matchScore(QStringLiteral("audio"), QStringLiteral("Insert Audio Track"))
        < CommandPalette::matchScore(QStringLiteral("iat"), QStringLiteral("Insert Audio Track")));
}

void TestCommands::paletteFiltersRanksAndTriggers()
{
    QAction insertAudio(QStringLiteral("Insert &Audio Track"));
    insertAudio.setObjectName(QStringLiteral("actionAddTrack"));
    QAction insertMidi(QStringLiteral("Insert &MIDI Track"));
    insertMidi.setObjectName(QStringLiteral("actionAddMidiTrack"));
    QAction about(QStringLiteral("&About Nylon"));
    about.setObjectName(QStringLiteral("actionAbout"));
    QAction unnamed(QStringLiteral("No object name"));
    int fired = 0;
    connect(&insertMidi, &QAction::triggered, [&fired] { ++fired; });

    CommandPalette palette({&about, &insertAudio, &insertMidi, &unnamed}, &m_themes.theme());
    QCOMPARE(palette.visibleActions().size(), 3);
    QCOMPARE(palette.visibleActions().first(), &about);
    palette.setFilter(QStringLiteral("midi"));
    QCOMPARE(palette.visibleActions(), QList<QAction*>{&insertMidi});
    palette.setFilter(QStringLiteral("insert"));
    QCOMPARE(palette.visibleActions().size(), 2);
    palette.setFilter(QStringLiteral("imt"));
    QCOMPARE(palette.currentAction(), &insertMidi);
    QSignalSpy triggered(&palette, &CommandPalette::triggered);
    QTest::keyClick(&palette, Qt::Key_Return);
    QCOMPARE(fired, 1);
    QCOMPARE(triggered.count(), 1);
    QVERIFY(!palette.isVisible());
}

void TestCommands::paletteSkipsDisabledActions()
{
    QAction save(QStringLiteral("&Save"));
    save.setObjectName(QStringLiteral("actionSave"));
    save.setEnabled(false);
    int fired = 0;
    connect(&save, &QAction::triggered, [&fired] { ++fired; });
    CommandPalette palette({&save}, &m_themes.theme());
    palette.show();
    QCOMPARE(palette.visibleActions().size(), 1);
    QTest::keyClick(&palette, Qt::Key_Return);
    QCOMPARE(fired, 0);
    QVERIFY(palette.isVisible());
    QTest::keyClick(&palette, Qt::Key_Escape);
    QVERIFY(!palette.isVisible());
}

void TestCommands::paletteOpensFromTheWindow()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    QAction* open = w.action(QStringLiteral("actionCommandPalette"));
    QVERIFY(open);
    QVERIFY(open->shortcuts().contains(QKeySequence(Qt::CTRL | Qt::Key_K)));
    QVERIFY(w.namedActions().size() > 20);
    open->trigger();
    auto* palette = w.findChild<CommandPalette*>();
    QVERIFY(palette);
    QVERIFY(palette->isVisible());
    palette->setFilter(QStringLiteral("insert audio"));
    QTest::keyClick(palette, Qt::Key_Return);
    QCOMPARE(bridge.trackCount(), 1ull);
}

void TestCommands::shortcutOverridesPersistAndApply()
{
    QSettings().remove(QLatin1String(Shortcuts::settingsGroup()));
    QAction a(QStringLiteral("Test"));
    a.setObjectName(QStringLiteral("actionTestOverride"));
    a.setShortcut(QKeySequence(Qt::CTRL | Qt::Key_1));
    Shortcuts::apply({&a});
    QCOMPARE(a.shortcut(), QKeySequence(Qt::CTRL | Qt::Key_1));
    Shortcuts::setOverride(&a, QKeySequence(Qt::CTRL | Qt::Key_2));
    QCOMPARE(a.shortcut(), QKeySequence(Qt::CTRL | Qt::Key_2));

    // A fresh action with the same name picks the override up.
    QAction b(QStringLiteral("Test"));
    b.setObjectName(QStringLiteral("actionTestOverride"));
    b.setShortcut(QKeySequence(Qt::CTRL | Qt::Key_1));
    Shortcuts::apply({&b});
    QCOMPARE(b.shortcut(), QKeySequence(Qt::CTRL | Qt::Key_2));
    QCOMPARE(Shortcuts::defaultFor(&b), QKeySequence(Qt::CTRL | Qt::Key_1));

    // Setting the default again clears the stored override.
    Shortcuts::setOverride(&b, QKeySequence(Qt::CTRL | Qt::Key_1));
    QSettings settings;
    settings.beginGroup(QLatin1String(Shortcuts::settingsGroup()));
    QVERIFY(!settings.contains(QStringLiteral("actionTestOverride")));
    settings.endGroup();

    Shortcuts::setOverride(&b, QKeySequence());
    QVERIFY(b.shortcut().isEmpty());
    Shortcuts::resetAll({&b});
    QCOMPARE(b.shortcut(), QKeySequence(Qt::CTRL | Qt::Key_1));
}

void TestCommands::shortcutConflictsAreRefused()
{
    QAction a(QStringLiteral("A"));
    a.setObjectName(QStringLiteral("actionA"));
    a.setShortcut(QKeySequence(Qt::CTRL | Qt::Key_1));
    QAction b(QStringLiteral("B"));
    b.setObjectName(QStringLiteral("actionB"));
    b.setShortcut(QKeySequence(Qt::CTRL | Qt::Key_2));
    QCOMPARE(Shortcuts::conflict({&a, &b}, QKeySequence(Qt::CTRL | Qt::Key_1), &b), &a);
    QCOMPARE(Shortcuts::conflict({&a, &b}, QKeySequence(Qt::CTRL | Qt::Key_1), &a), nullptr);
    QCOMPARE(Shortcuts::conflict({&a, &b}, QKeySequence(), &a), nullptr);
}

void TestCommands::preferencesShortcutPageEdits()
{
    QSettings().remove(QLatin1String(Shortcuts::settingsGroup()));
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    PreferencesDialog dialog(&m_themes, w.namedActions());
    QTableWidget* table = dialog.shortcutTable();
    QVERIFY(table->rowCount() > 20);
    int undoRow = -1, redoRow = -1;
    for (int r = 0; r < table->rowCount(); ++r) {
        const QString name = table->item(r, 0)->data(Qt::UserRole).toString();
        if (name == QLatin1String("actionUndo")) {
            undoRow = r;
        } else if (name == QLatin1String("actionRedo")) {
            redoRow = r;
        }
    }
    QVERIFY(undoRow >= 0 && redoRow >= 0);
    QAction* undo = w.action(QStringLiteral("actionUndo"));
    QAction* redo = w.action(QStringLiteral("actionRedo"));
    // Taking redo's key for undo is refused; a free key is accepted.
    QVERIFY(!dialog.assignShortcut(undoRow, redo->shortcut()));
    QCOMPARE(undo->shortcut(), QKeySequence(QKeySequence::Undo));
    QVERIFY(dialog.assignShortcut(undoRow, QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_U)));
    QCOMPARE(undo->shortcut(), QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_U));
    auto* edit = qobject_cast<QKeySequenceEdit*>(table->cellWidget(undoRow, 1));
    QVERIFY(edit);

    // A new window applies the stored override.
    MainWindow w2(&bridge, &m_themes);
    QCOMPARE(w2.action(QStringLiteral("actionUndo"))->shortcut(), QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_U));
    Shortcuts::resetAll(w2.namedActions());
    QCOMPARE(w2.action(QStringLiteral("actionUndo"))->shortcut(), QKeySequence(QKeySequence::Undo));
}

QTEST_MAIN(TestCommands)
#include "test_commands.moc"
