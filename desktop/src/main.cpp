#include "MainWindow.h"
#include "RemoteControl.h"
#include "ProjectBridge.h"
#include "ThemeManager.h"

#include <QApplication>
#include <QCommandLineParser>
#include <QMessageBox>
#include <QSettings>
#include <QTimer>

#include <cstdio>

int main(int argc, char** argv)
{
    QApplication app(argc, argv);
    QCoreApplication::setOrganizationDomain(QStringLiteral("nylon.app"));
    QCoreApplication::setOrganizationName(QStringLiteral("Nylon"));
    QCoreApplication::setApplicationName(QStringLiteral("Nylon"));
    QCoreApplication::setApplicationVersion(QStringLiteral(NYLON_VERSION_STRING));

    QCommandLineParser parser;
    parser.setApplicationDescription(QStringLiteral("Nylon digital audio workstation"));
    parser.addHelpOption();
    parser.addVersionOption();
    const QCommandLineOption themeOption(QStringLiteral("theme"),
        QStringLiteral("Theme to load at startup (default: the last one chosen)."), QStringLiteral("name"));
    const QCommandLineOption workspaceOption(QStringLiteral("workspace"),
        QStringLiteral("Skip the start screen and open an empty project."));
    const QCommandLineOption startOption(QStringLiteral("start"),
        QStringLiteral("Stay on the start screen (with --screenshot, captures it)."));
    const QCommandLineOption tracksOption(QStringLiteral("tracks"),
        QStringLiteral("Add this many tracks to the new project."), QStringLiteral("count"), QStringLiteral("0"));
    const QCommandLineOption screenshotOption(QStringLiteral("screenshot"),
        QStringLiteral("Write a PNG of the main window to <file> and exit."), QStringLiteral("file"));
    const QCommandLineOption viewOption(QStringLiteral("view"),
        QStringLiteral("Initial view: session or arrangement."), QStringLiteral("name"), QStringLiteral("session"));
    parser.addOption(themeOption);
    parser.addOption(workspaceOption);
    parser.addOption(startOption);
    parser.addOption(tracksOption);
    parser.addOption(viewOption);
    parser.addOption(screenshotOption);
    const QCommandLineOption controlOption(QStringLiteral("control"),
        QStringLiteral("Enable local terminal control at the named endpoint."), QStringLiteral("name"));
    parser.addOption(controlOption);
    parser.process(app);

    nylon::ThemeManager themes;
    QString themeName = parser.value(themeOption);
    if (themeName.isEmpty()) {
        themeName = QSettings().value(QStringLiteral("look/theme"), QStringLiteral("nylon")).toString();
    }
    if (!themes.load(themeName) && !themes.load(QStringLiteral("nylon"))) {
        QMessageBox::critical(nullptr, QStringLiteral("Nylon"),
            QStringLiteral("The default theme failed to load:\n%1")
                .arg(themes.lastErrors().join(QStringLiteral("\n"))));
        return 1;
    }

    nylon::ProjectBridge bridge;
    if (!bridge.isValid()) {
        QMessageBox::critical(nullptr, QStringLiteral("Nylon"),
            QStringLiteral("The core library could not create a project."));
        return 1;
    }

    bool tracksOk = false;
    const int tracks = parser.value(tracksOption).toInt(&tracksOk);
    if (!tracksOk || tracks < 0) {
        std::fprintf(stderr, "--tracks expects a non-negative integer\n");
        return 2;
    }
    nylon::MainWindow window(&bridge, &themes);
    if (!parser.isSet(startOption) && (parser.isSet(workspaceOption) || parser.isSet(screenshotOption) || tracks > 0)) {
        window.newProject();
        for (int i = 0; i < tracks; ++i) {
            if (!bridge.addTrack()) {
                std::fprintf(stderr, "the core rejected adding track %d\n", i + 1);
                return 1;
            }
        }
    }
    const QString view = parser.value(viewOption).toLower();
    if (view == QLatin1String("arrangement")) {
        window.showArrangement();
    } else if (view != QLatin1String("session")) {
        std::fprintf(stderr, "--view expects 'session' or 'arrangement'\n");
        return 2;
    }
    nylon::RemoteControl control(&window);
    if (parser.isSet(controlOption) &&
        (parser.value(controlOption).isEmpty() || !control.listen(parser.value(controlOption)))) {
        std::fprintf(stderr, "Could not open the local control endpoint\n");
        return 1;
    }
    window.show();

    if (parser.isSet(screenshotOption)) {
        const QString file = parser.value(screenshotOption);
        // Grab after the first event-loop pass so layouts have settled.
        QTimer::singleShot(0, &window, [&window, &app, file] {
            const bool ok = window.grab().save(file, "PNG");
            if (!ok) {
                std::fprintf(stderr, "could not write screenshot\n");
            }
            app.exit(ok ? 0 : 1);
        });
    }
    return app.exec();
}
