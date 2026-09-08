#include "RemoteControl.h"
#include "MainWindow.h"
#include "ProjectBridge.h"
#include <QLocalSocket>
#include <QJsonDocument>
#include <QJsonArray>
#include <QTimer>
#include <cmath>

namespace nylon {
RemoteControl::RemoteControl(MainWindow* window) : QLocalServer(window), m_window(window)
{
    setSocketOptions(QLocalServer::UserAccessOption);
    setMaxPendingConnections(4);
    connect(this, &QLocalServer::newConnection, this, [this] {
        while (auto* socket = nextPendingConnection()) {
            socket->setReadBufferSize(8193);
            auto* deadline = new QTimer(socket);
            deadline->setSingleShot(true);
            connect(deadline, &QTimer::timeout, socket, &QLocalSocket::abort);
            deadline->start(2000);
            connect(socket, &QLocalSocket::disconnected, socket, &QObject::deleteLater);
            connect(socket, &QLocalSocket::readyRead, this, [this, socket] {
                if (socket->property("handled").toBool()) return;
                if (socket->bytesAvailable() > 8192) { socket->abort(); return; }
                if (!socket->canReadLine()) return;
                socket->setProperty("handled", true);
                QJsonParseError error;
                const auto document = QJsonDocument::fromJson(socket->readLine(), &error);
                const auto result = error.error == QJsonParseError::NoError && document.isObject()
                    ? execute(document.object())
                    : QJsonObject{{"ok", false}, {"error", "Invalid JSON object"}};
                socket->write(QJsonDocument(result).toJson(QJsonDocument::Compact) + '\n');
                socket->disconnectFromServer();
            });
        }
    });
}
QJsonObject RemoteControl::execute(const QJsonObject& request)
{
    if (!request.value("command").isString() ||
        (request.contains("args") && !request.value("args").isArray()))
        return {{"ok", false}, {"error", "Invalid command structure"}};
    const auto command = request.value("command").toString();
    const auto args = request.value("args").toArray();
    auto* bridge = m_window->bridge();
    auto reply = [](bool ok) { return QJsonObject{{"ok", ok}}; };
    if (command == "info" && args.isEmpty()) {
        return {{"ok", true}, {"tempo", bridge->tempo()},
            {"tracks", static_cast<double>(bridge->trackCount())},
            {"view", m_window->isStartScreenVisible() ? "start" : m_window->isSessionVisible() ? "session" : "arrangement"}};
    }
    if (command == "undo" && args.isEmpty()) return reply(bridge->undo());
    if (command == "redo" && args.isEmpty()) return reply(bridge->redo());
    if (command == "set-tempo" && args.size() == 1 && args[0].isString()) {
        bool ok = false;
        const double tempo = args[0].toString().toDouble(&ok);
        return reply(ok && std::isfinite(tempo) && bridge->setTempo(tempo));
    }
    if (command == "add-track" && args.isEmpty()) {
        const bool added = bridge->addTrack();
        if (added) m_window->selectTrack(static_cast<int>(bridge->trackCount()) - 1);
        return reply(added);
    }
    if (command == "view" && args.size() == 1) {
        const auto view = args[0].toString();
        if (view == "session") m_window->showSession();
        else if (view == "arrangement") m_window->showArrangement();
        else return reply(false);
        return reply(true);
    }
    if (command == "window" && args.size() == 1) {
        const auto mode = args[0].toString();
        if (mode == "minimize") m_window->showMinimized();
        else if (mode == "maximize") m_window->showMaximized();
        else if (mode == "restore") m_window->showNormal();
        else return reply(false);
        return reply(true);
    }
    return {{"ok", false}, {"error", "Unknown command or invalid arguments"}};
}
}
