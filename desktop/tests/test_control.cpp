#include "RemoteControl.h"
#include "MainWindow.h"
#include "ProjectBridge.h"
#include "ThemeManager.h"
#include <QJsonArray>
#include <QStatusBar>
#include <QAction>
#include "TitleBar.h"
#include <QLabel>
#include <QMenuBar>
#include <QMenu>
#include <QDir>
#include <QtTest>

class ControlTest : public QObject {
    Q_OBJECT
private slots:
    void titleAndControlsRemainAligned()
    {
        nylon::ThemeManager themes;
        QVERIFY(themes.load("nylon"));
        nylon::ProjectBridge bridge;
        nylon::MainWindow window(&bridge, &themes);
        window.resize(1440, 900);
        window.show();
        QTest::qWait(1);
        auto* bar = window.titleBar();
        auto* title = bar->findChild<QLabel*>("windowTitle");
        QVERIFY(title);
        QVERIFY(title->isVisible());
        QVERIFY(qAbs(title->geometry().center().x() - bar->rect().center().x()) <= 1);
        QVERIFY(bar->menuBar()->geometry().right() < title->geometry().left());
        const auto close = bar->closeRect();
        QCOMPARE(close.center().y(), bar->minimizeRect().center().y());
        QCOMPARE(close.center().y(), bar->zoomRect().center().y());
        QSignalSpy zoom(bar, &nylon::TitleBar::zoomRequested);
        QTest::mouseMove(bar, bar->zoomRect().center());
        QCOMPARE(bar->closeRect(), close);
        const QString captures = qEnvironmentVariable("NYLON_CAPTURE_DIR");
        if (!captures.isEmpty()) {
            QVERIFY(bar->grab().save(QDir(captures).filePath("title-hover.png")));
        }
        QTest::mouseClick(bar, Qt::LeftButton, Qt::NoModifier, bar->zoomRect().center());
        QCOMPARE(zoom.count(), 1);
        QTest::mouseClick(bar, Qt::RightButton, Qt::NoModifier, bar->zoomRect().center());
        QCOMPARE(zoom.count(), 1);
        auto* menu = bar->menuBar()->actions().first()->menu();
        QVERIFY(menu);
        menu->popup(bar->mapToGlobal(QPoint(0, bar->height())));
        QTest::qWait(1);
        for (auto* action : menu->actions()) {
            if (!action->isSeparator() && action->isVisible())
                QCOMPARE(menu->actionAt(menu->actionGeometry(action).center()), action);
        }
        if (!captures.isEmpty()) {
            QVERIFY(menu->grab().save(QDir(captures).filePath("file-menu.png")));
        }
        menu->hide();
    }
    void editsShareHistory()
    {
        nylon::ThemeManager themes;
        QVERIFY(themes.load("nylon"));
        nylon::ProjectBridge bridge;
        nylon::MainWindow window(&bridge, &themes);
        nylon::RemoteControl control(&window);
        QVERIFY(control.execute({{"command", "add-track"}})["ok"].toBool());
        QCOMPARE(bridge.trackCount(), quint64(1));
        QVERIFY(control.execute({{"command", "undo"}})["ok"].toBool());
        QCOMPARE(bridge.trackCount(), quint64(0));
        QVERIFY(control.execute({{"command", "redo"}})["ok"].toBool());
        QCOMPARE(bridge.trackCount(), quint64(1));
        QVERIFY(control.execute({{"command", "set-tempo"}, {"args", QJsonArray{"135"}}})["ok"].toBool());
        QCOMPARE(bridge.tempo(), 135.0);
        QVERIFY(!control.execute({{"command", "set-tempo"}, {"args", QJsonArray{"invalid"}}})["ok"].toBool());
        QCOMPARE(bridge.tempo(), 135.0);
        QVERIFY(!control.execute({{"command", "unsupported"}})["ok"].toBool());
    }
    void emptyStatusDoesNotReserveSpace()
    {
        nylon::ThemeManager themes;
        QVERIFY(themes.load("nylon"));
        nylon::ProjectBridge bridge;
        nylon::MainWindow window(&bridge, &themes);
        window.show();
        QTest::qWait(1);
        QVERIFY(!window.statusBar()->isVisible());
        QCOMPARE(window.centralWidget()->geometry().bottom(), window.height() - 1);
        window.statusBar()->showMessage("Status");
        QVERIFY(window.statusBar()->isVisible());
        window.statusBar()->clearMessage();
        QVERIFY(!window.statusBar()->isVisible());
        window.action("actionAddTrack")->trigger();
        QVERIFY(!window.isStartScreenVisible());
        QCOMPARE(bridge.trackCount(), quint64(1));
    }
    void switchesViews()
    {
        nylon::ThemeManager themes;
        QVERIFY(themes.load("nylon"));
        nylon::ProjectBridge bridge;
        nylon::MainWindow window(&bridge, &themes);
        nylon::RemoteControl control(&window);
        QVERIFY(window.isStartScreenVisible());
        QVERIFY(control.execute({{"command", "view"}, {"args", QJsonArray{"arrangement"}}})["ok"].toBool());
        QVERIFY(!window.isSessionVisible());
        QVERIFY(!window.isStartScreenVisible());
        QVERIFY(control.execute({{"command", "view"}, {"args", QJsonArray{"session"}}})["ok"].toBool());
        QVERIFY(window.isSessionVisible());
    }
};
QTEST_MAIN(ControlTest)
#include "test_control.moc"
