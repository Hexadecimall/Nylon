#include "MainWindow.h"
#include "ProjectBridge.h"
#include "ThemeManager.h"

#include <QApplication>
#include <QMessageBox>

int main(int argc, char** argv)
{
    QApplication app(argc, argv);
    QCoreApplication::setOrganizationDomain(QStringLiteral("nylon.app"));
    QCoreApplication::setOrganizationName(QStringLiteral("Nylon"));
    QCoreApplication::setApplicationName(QStringLiteral("Nylon"));
    QCoreApplication::setApplicationVersion(QStringLiteral(NYLON_VERSION_STRING));

    nylon::ThemeManager themes;
    if (!themes.load(QStringLiteral("nylon"))) {
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

    nylon::MainWindow window(&bridge, &themes);
    window.show();
    return app.exec();
}
