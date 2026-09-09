#include "control_server.hpp"

#include "nylon.hpp"

#include <QCoreApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLocalServer>
#include <QLocalSocket>
#include <cmath>
#include <cstdio>
#include <utility>

namespace {
constexpr qsizetype ProtocolLimit = 8'192;

QJsonObject error(const QString& message)
{
    return {{"ok", false}, {"error", message}};
}

bool indexValue(const QJsonValue& value, std::uint64_t& result)
{
    if (!value.isString()) return false;
    bool parsed = false;
    result = value.toString().toULongLong(&parsed);
    return parsed;
}

bool numberValue(const QJsonValue& value, double& result)
{
    if (!value.isString()) return false;
    bool parsed = false;
    result = value.toString().toDouble(&parsed);
    return parsed && std::isfinite(result);
}

bool flagValue(const QJsonValue& value, bool& result)
{
    if (!value.isString()) return false;
    const QString text = value.toString();
    if (text == "on" || text == "true" || text == "1") result = true;
    else if (text == "off" || text == "false" || text == "0") result = false;
    else return false;
    return true;
}

bool signedValue(const QJsonValue& value, int& result)
{
    if (!value.isString()) return false;
    bool parsed = false;
    result = value.toString().toInt(&parsed);
    return parsed;
}

bool byteValue(const QJsonValue& value, std::uint8_t& result)
{
    if (!value.isString()) return false;
    bool parsed = false;
    const uint parsedValue = value.toString().toUInt(&parsed);
    if (!parsed || parsedValue > 255) return false;
    result = static_cast<std::uint8_t>(parsedValue);
    return true;
}

class ControlHost {
public:
    bool open(const ServerOptions& options, QString& message)
    {
        m_bundle = options.project;
        if (!m_project || !m_project.open(m_bundle.toStdString())) {
            message = "Could not open the project bundle";
            return false;
        }
        if (!options.audioEnabled) return true;
        std::uint64_t device = options.device;
        if (options.useDefaultDevice && !nylon::AudioEngine::defaultOutput(device)) {
            message = "No default output device is available";
            return false;
        }
        if (!m_audio.open(m_project, device, options.sampleRate, options.blockFrames)) {
            message = "Could not open the audio output";
            return false;
        }
        return true;
    }

    QJsonObject execute(const QJsonObject& request, bool& quit)
    {
        const QString command = request.value("command").toString();
        const QJsonArray args = request.value("args").toArray();
        if (command.isEmpty()) return error("The command is missing");
        if (command == "status" && args.isEmpty()) return status();
        if (command == "notes" && args.size() == 2) return notes(args);
        if (command == "quit" && args.isEmpty()) {
            quit = true;
            return {{"ok", true}};
        }
        if (command == "save" && args.isEmpty()) {
            if (!m_project.save(m_bundle.toStdString())) return error("Could not save the project");
            return {{"ok", true}};
        }
        if (command == "sync" && args.isEmpty()) {
            if (!m_audio.isOpen()) return error("The audio output is closed");
            if (!m_audio.sync(m_project)) return error("Could not synchronize the audio engine");
            return {{"ok", true}};
        }
        if (command == "reload" && args.isEmpty()) {
            nylon::Project loaded;
            if (!loaded || !loaded.open(m_bundle.toStdString()))
                return error("Could not reload the project bundle");
            if (m_audio.isOpen() && !m_audio.sync(loaded))
                return error("Could not synchronize the reloaded project");
            m_project = std::move(loaded);
            return {{"ok", true}};
        }
        if (command == "play" && args.isEmpty()) return audioResult(m_audio.play(), "start playback");
        if (command == "stop" && args.isEmpty()) return audioResult(m_audio.stop(), "stop playback");
        if (command == "locate" && args.size() == 1) {
            double beats = 0.0;
            if (!numberValue(args[0], beats) || beats < 0.0) return error("Invalid beat position");
            return audioResult(m_audio.locate(beats), "move the transport");
        }
        if (command == "launch-clip" && args.size() == 3) {
            std::uint64_t track = 0;
            std::uint64_t scene = 0;
            double quantization = 0.0;
            if (!indexValue(args[0], track) || !indexValue(args[1], scene)
                || !numberValue(args[2], quantization) || quantization < 0.0)
                return error("Invalid track, scene, or quantization");
            return audioResult(
                m_audio.launchClip(m_project, track, scene, quantization), "launch the clip");
        }
        if (command == "launch-scene" && args.size() == 2) {
            std::uint64_t scene = 0;
            double quantization = 0.0;
            if (!indexValue(args[0], scene) || !numberValue(args[1], quantization)
                || quantization < 0.0)
                return error("Invalid scene or quantization");
            return audioResult(
                m_audio.launchScene(m_project, scene, quantization), "launch the scene");
        }
        if (command == "stop-clip" && args.size() == 1) {
            std::uint64_t track = 0;
            if (!indexValue(args[0], track)) return error("Invalid track index");
            return audioResult(
                m_audio.stopSessionTrack(m_project, track), "stop the Session clip");
        }
        if (command == "set-tempo" && args.size() == 1) {
            double tempo = 0.0;
            if (!numberValue(args[0], tempo) || !m_project.setTempo(tempo))
                return error("Invalid tempo");
            return commitEdit();
        }
        if (command == "create-midi-clip" && args.size() == 3) {
            std::uint64_t track = 0;
            std::uint64_t scene = 0;
            double length = 0.0;
            if (!indexValue(args[0], track) || !indexValue(args[1], scene)
                || !numberValue(args[2], length)
                || !m_project.createMidiClip(track, scene, length))
                return error("Invalid MIDI clip");
            return commitEdit();
        }
        if (command == "add-note" && args.size() == 6) {
            std::uint64_t track = 0;
            std::uint64_t scene = 0;
            std::uint8_t pitch = 0;
            std::uint8_t velocity = 0;
            double start = 0.0;
            double length = 0.0;
            if (!indexValue(args[0], track) || !indexValue(args[1], scene)
                || !byteValue(args[2], pitch) || !byteValue(args[3], velocity)
                || !numberValue(args[4], start) || !numberValue(args[5], length)
                || !m_project.addClipNote(track, scene, {pitch, velocity, start, length}))
                return error("Invalid MIDI note");
            return commitEdit();
        }
        if (command == "quantize-notes" && args.size() == 4) {
            std::uint64_t track = 0;
            std::uint64_t scene = 0;
            double grid = 0.0;
            double strength = 0.0;
            if (!indexValue(args[0], track) || !indexValue(args[1], scene)
                || !numberValue(args[2], grid) || !numberValue(args[3], strength)
                || !m_project.quantizeClipNotes(track, scene, grid, strength))
                return error("Invalid quantize settings");
            return commitEdit();
        }
        if (command == "transpose-notes" && args.size() == 3) {
            std::uint64_t track = 0;
            std::uint64_t scene = 0;
            int semitones = 0;
            if (!indexValue(args[0], track) || !indexValue(args[1], scene)
                || !signedValue(args[2], semitones)
                || !m_project.transposeClipNotes(track, scene, semitones))
                return error("Invalid transpose settings");
            return commitEdit();
        }
        if (command == "set-note-velocity" && args.size() == 3) {
            std::uint64_t track = 0;
            std::uint64_t scene = 0;
            std::uint8_t velocity = 0;
            if (!indexValue(args[0], track) || !indexValue(args[1], scene)
                || !byteValue(args[2], velocity)
                || !m_project.setClipNoteVelocity(track, scene, velocity))
                return error("Invalid velocity");
            return commitEdit();
        }
        if (command == "humanize-notes" && args.size() == 5) {
            std::uint64_t track = 0;
            std::uint64_t scene = 0;
            std::uint8_t velocityRange = 0;
            std::uint64_t seed = 0;
            double timing = 0.0;
            if (!indexValue(args[0], track) || !indexValue(args[1], scene)
                || !numberValue(args[2], timing) || !byteValue(args[3], velocityRange)
                || !indexValue(args[4], seed)
                || !m_project.humanizeClipNotes(track, scene, timing, velocityRange, seed))
                return error("Invalid humanize settings");
            return commitEdit();
        }
        if (command == "set-track-volume" && args.size() == 2) {
            std::uint64_t track = 0;
            double value = 0.0;
            if (!indexValue(args[0], track) || !numberValue(args[1], value)
                || !m_project.setTrackVolumeDb(track, value))
                return error("Invalid track volume");
            return commitEdit();
        }
        if (command == "set-track-pan" && args.size() == 2) {
            std::uint64_t track = 0;
            double value = 0.0;
            if (!indexValue(args[0], track) || !numberValue(args[1], value)
                || !m_project.setTrackPan(track, value))
                return error("Invalid track pan");
            return commitEdit();
        }
        if (command == "set-track-mute" && args.size() == 2)
            return setTrackFlag(args, &nylon::Project::setTrackMuted, "mute state");
        if (command == "set-track-solo" && args.size() == 2)
            return setTrackFlag(args, &nylon::Project::setTrackSolo, "solo state");
        if (command == "set-track-arm" && args.size() == 2)
            return setTrackFlag(args, &nylon::Project::setTrackArmed, "arm state");
        if (command == "undo" && args.isEmpty()) {
            if (!m_project.undo()) return error("Nothing can be undone");
            return commitEdit();
        }
        if (command == "redo" && args.isEmpty()) {
            if (!m_project.redo()) return error("Nothing can be redone");
            return commitEdit();
        }
        return error("Unknown command or invalid arguments");
    }

private:
    using TrackFlagSetter = bool (nylon::Project::*)(std::uint64_t, bool);

    QJsonObject status()
    {
        QJsonArray active;
        for (std::uint64_t track = 0; track < m_project.trackCount(); ++track) {
            const std::int64_t scene = m_audio.activeSessionScene(track);
            if (scene >= 0)
                active.append(QJsonObject{{"track", static_cast<double>(track)},
                    {"scene", static_cast<double>(scene)}});
        }
        return {{"ok", true}, {"audioOpen", m_audio.isOpen()},
            {"playing", m_audio.isPlaying()}, {"positionBeats", m_audio.positionBeats()},
            {"dropouts", static_cast<double>(m_audio.dropouts())}, {"tempo", m_project.tempo()},
            {"framesRendered", static_cast<double>(m_audio.framesRendered())},
            {"tracks", static_cast<double>(m_project.trackCount())},
            {"scenes", static_cast<double>(m_project.sceneCount())},
            {"activeSessions", active}};
    }

    QJsonObject notes(const QJsonArray& args)
    {
        std::uint64_t track = 0;
        std::uint64_t scene = 0;
        if (!indexValue(args[0], track) || !indexValue(args[1], scene))
            return error("Invalid track or scene index");
        QJsonArray notes;
        const auto count = m_project.clipNoteCount(track, scene);
        for (std::uint64_t index = 0; index < count; ++index) {
            nylon::MidiNote note{};
            if (!m_project.clipNote(track, scene, index, note))
                return error("Could not read the MIDI note");
            notes.append(QJsonObject{{"index", static_cast<double>(index)},
                {"pitch", note.pitch}, {"velocity", note.velocity},
                {"startBeats", note.startBeats}, {"lengthBeats", note.lengthBeats}});
        }
        return {{"ok", true}, {"notes", notes}};
    }

    QJsonObject audioResult(bool accepted, const QString& operation)
    {
        return accepted ? QJsonObject{{"ok", true}}
                        : error(QString("Could not %1").arg(operation));
    }

    QJsonObject commitEdit()
    {
        if (m_audio.isOpen() && !m_audio.sync(m_project))
            return error("The edit was applied but audio synchronization failed");
        if (!m_project.save(m_bundle.toStdString()))
            return error("The edit was applied but saving failed");
        return {{"ok", true}};
    }

    QJsonObject setTrackFlag(
        const QJsonArray& args, TrackFlagSetter setter, const QString& label)
    {
        std::uint64_t track = 0;
        bool enabled = false;
        if (!indexValue(args[0], track) || !flagValue(args[1], enabled)
            || !(m_project.*setter)(track, enabled))
            return error(QString("Invalid track %1").arg(label));
        return commitEdit();
    }

    QString m_bundle;
    nylon::Project m_project;
    nylon::AudioEngine m_audio;
};

bool listen(QLocalServer& server, const QString& endpoint, QString& message)
{
    if (server.listen(endpoint)) return true;
    QLocalSocket probe;
    probe.connectToServer(endpoint);
    if (probe.waitForConnected(100)) {
        message = "The endpoint is already in use";
        return false;
    }
    QLocalServer::removeServer(endpoint);
    if (server.listen(endpoint)) return true;
    message = server.errorString();
    return false;
}

void writeReply(QLocalSocket& socket, const QJsonObject& reply)
{
    socket.write(QJsonDocument(reply).toJson(QJsonDocument::Compact) + '\n');
    socket.flush();
}
}

int runControlServer(QCoreApplication& application, const ServerOptions& options)
{
    ControlHost host;
    QString message;
    if (!host.open(options, message)) {
        const QByteArray output = QJsonDocument(error(message)).toJson(QJsonDocument::Compact) + '\n';
        std::fwrite(output.constData(), 1, static_cast<std::size_t>(output.size()), stdout);
        return 1;
    }
    QLocalServer server;
    if (!listen(server, options.endpoint, message)) {
        const QByteArray output = QJsonDocument(error(message))
                                      .toJson(QJsonDocument::Compact)
            + '\n';
        std::fwrite(output.constData(), 1, static_cast<std::size_t>(output.size()), stdout);
        return 1;
    }
    const QByteArray ready = QJsonDocument(
        QJsonObject{{"ok", true}, {"endpoint", options.endpoint}, {"audioOpen", options.audioEnabled}})
                                 .toJson(QJsonDocument::Compact)
        + '\n';
    std::fwrite(ready.constData(), 1, static_cast<std::size_t>(ready.size()), stdout);
    std::fflush(stdout);

    QObject::connect(&server, &QLocalServer::newConnection, &application, [&] {
        while (QLocalSocket* socket = server.nextPendingConnection()) {
            QObject::connect(socket, &QLocalSocket::disconnected, socket, &QObject::deleteLater);
            QObject::connect(socket, &QLocalSocket::readyRead, socket, [&, socket] {
                QByteArray buffer = socket->property("requestBuffer").toByteArray();
                buffer += socket->readAll();
                if (buffer.size() > ProtocolLimit) {
                    writeReply(*socket, error("The command is longer than the protocol limit"));
                    socket->disconnectFromServer();
                    return;
                }
                const qsizetype newline = buffer.indexOf('\n');
                if (newline < 0) {
                    socket->setProperty("requestBuffer", buffer);
                    return;
                }
                QJsonParseError parseError;
                const QJsonDocument document = QJsonDocument::fromJson(buffer.left(newline), &parseError);
                bool quit = false;
                const QJsonObject reply = parseError.error == QJsonParseError::NoError
                        && document.isObject()
                    ? host.execute(document.object(), quit)
                    : error("The command is not valid JSON");
                writeReply(*socket, reply);
                if (quit)
                    QObject::connect(socket, &QLocalSocket::disconnected, &application,
                        &QCoreApplication::quit);
                socket->disconnectFromServer();
            });
        }
    });
    const int status = application.exec();
    server.close();
    QLocalServer::removeServer(options.endpoint);
    return status;
}
