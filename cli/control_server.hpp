#ifndef NYLON_CONTROL_SERVER_HPP
#define NYLON_CONTROL_SERVER_HPP

#include <QString>
#include <QStringList>
#include <cstdint>

class QCoreApplication;

struct ServerOptions {
    QString endpoint;
    QString project;
    bool audioEnabled{true};
    bool useDefaultDevice{true};
    std::uint64_t device{};
    std::uint32_t sampleRate{48'000};
    std::uint32_t blockFrames{256};
    QString pluginWorker;
    QStringList pluginRoots;
};

int runControlServer(QCoreApplication& application, const ServerOptions& options);

#endif
