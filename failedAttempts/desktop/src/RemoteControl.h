#pragma once
#include <QLocalServer>
#include <QJsonObject>
namespace nylon {
class MainWindow;
class RemoteControl : public QLocalServer {
public:
    explicit RemoteControl(MainWindow* window);
    QJsonObject execute(const QJsonObject& request);
private:
    MainWindow* m_window;
};
}
