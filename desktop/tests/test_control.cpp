#include "RemoteControl.h"
#include "MainWindow.h"
#include "ProjectBridge.h"
#include "ThemeManager.h"
#include <QJsonArray>
#include <QtTest>

class ControlTest : public QObject {
    Q_OBJECT
private slots:
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
    void switchesViews()
    {
        nylon::ThemeManager themes;
        QVERIFY(themes.load("nylon"));
        nylon::ProjectBridge bridge;
        nylon::MainWindow window(&bridge, &themes);
        nylon::RemoteControl control(&window);
        window.newProject();
        QVERIFY(control.execute({{"command", "view"}, {"args", QJsonArray{"arrangement"}}})["ok"].toBool());
        QVERIFY(!window.isSessionVisible());
        QVERIFY(control.execute({{"command", "view"}, {"args", QJsonArray{"session"}}})["ok"].toBool());
        QVERIFY(window.isSessionVisible());
    }
};
QTEST_MAIN(ControlTest)
#include "test_control.moc"
