#include "nylon.hpp"

#include <QCommandLineParser>
#include <QCoreApplication>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLocalSocket>
#include <QStringList>
#include <cmath>
#include <cstdio>
#include <limits>

namespace {
int writeJson(const QJsonObject& object, int status = 0)
{
    const QByteArray output = QJsonDocument(object).toJson(QJsonDocument::Compact) + '\n';
    std::fwrite(output.constData(), 1, static_cast<std::size_t>(output.size()), stdout);
    std::fflush(stdout);
    return status;
}

int fail(const QString& message)
{
    return writeJson({{"ok", false}, {"error", message}}, 1);
}

bool number(const QString& text, double& value)
{
    bool parsed = false;
    value = text.toDouble(&parsed);
    return parsed && std::isfinite(value);
}

bool unsignedNumber(const QString& text, std::uint32_t& value)
{
    bool parsed = false;
    const qulonglong wide = text.toULongLong(&parsed);
    if (!parsed || wide > std::numeric_limits<std::uint32_t>::max()) return false;
    value = static_cast<std::uint32_t>(wide);
    return true;
}

bool indexNumber(const QString& text, std::uint64_t& value)
{
    bool parsed = false;
    value = text.toULongLong(&parsed);
    return parsed;
}

bool flag(const QString& text, bool& value)
{
    if (text == "on" || text == "true" || text == "1") value = true;
    else if (text == "off" || text == "false" || text == "0") value = false;
    else return false;
    return true;
}

bool trackKind(const QString& text, nylon::TrackKind& kind)
{
    if (text == "audio") kind = nylon::TrackKind::Audio;
    else if (text == "midi") kind = nylon::TrackKind::Midi;
    else if (text == "return") kind = nylon::TrackKind::Return;
    else if (text == "master") kind = nylon::TrackKind::Master;
    else if (text == "group") kind = nylon::TrackKind::Group;
    else if (text == "cue") kind = nylon::TrackKind::Cue;
    else return false;
    return true;
}

QString trackKindName(nylon::TrackKind kind)
{
    switch (kind) {
    case nylon::TrackKind::Audio: return "audio";
    case nylon::TrackKind::Midi: return "midi";
    case nylon::TrackKind::Return: return "return";
    case nylon::TrackKind::Master: return "master";
    case nylon::TrackKind::Group: return "group";
    case nylon::TrackKind::Cue: return "cue";
    }
    return "audio";
}

int remoteCommand(const QString& endpoint, const QStringList& positional)
{
    QLocalSocket socket;
    socket.connectToServer(endpoint);
    if (!socket.waitForConnected(5000)) return fail("Could not connect to the Nylon endpoint");
    QJsonArray args;
    for (int index = 1; index < positional.size(); ++index) args.append(positional[index]);
    const QByteArray request = QJsonDocument(
        QJsonObject{{"command", positional[0]}, {"args", args}})
                                   .toJson(QJsonDocument::Compact)
        + '\n';
    if (request.size() > 8192) return fail("The command is longer than the protocol limit");
    socket.write(request);
    if (!socket.waitForBytesWritten(5000)) return fail("Could not send the command");
    QByteArray response;
    while (!response.contains('\n')) {
        if (!socket.bytesAvailable() && !socket.waitForReadyRead(5000))
            return fail("The endpoint did not reply");
        response += socket.readAll();
        if (response.size() > 8192) return fail("The reply is longer than the protocol limit");
    }
    std::fwrite(response.constData(), 1, static_cast<std::size_t>(response.size()), stdout);
    std::fflush(stdout);
    return QJsonDocument::fromJson(response).object().value("ok").toBool() ? 0 : 1;
}

int listDevices()
{
    QJsonArray devices;
    for (const auto& device : nylon::AudioEngine::devices()) {
        QJsonArray rates;
        for (const auto rate : device.sampleRates) rates.append(static_cast<double>(rate));
        devices.append(QJsonObject{{"id", static_cast<double>(device.id)},
            {"name", QString::fromStdString(device.name)},
            {"channels", static_cast<double>(device.channels)}, {"default", device.isDefault},
            {"sampleRates", rates}});
    }
    return writeJson({{"ok", true}, {"devices", devices}});
}

int directCommand(const QString& bundle, const QStringList& positional)
{
    const QString command = positional[0];
    if (command == "devices" && positional.size() == 1) return listDevices();
    if (bundle.isEmpty()) return fail("A project bundle is required with --project");
    if (command == "recovery-status" && positional.size() == 1) {
        return writeJson({{"ok", true},
            {"available", nylon::Project::recoveryAvailable(bundle.toStdString())}});
    }
    if (command == "discard-recovery" && positional.size() == 1) {
        if (!nylon::Project::discardRecovery(bundle.toStdString()))
            return fail("Could not discard the recovery document");
        return writeJson({{"ok", true}});
    }

    nylon::Project project;
    if (!project) return fail("Could not create a project handle");
    if (command == "new") {
        if (positional.size() != 1) return fail("new takes no arguments");
        if (QFileInfo::exists(bundle)) return fail("The project bundle already exists");
        if (!project.save(bundle.toStdString())) return fail("Could not create the project bundle");
        return writeJson({{"ok", true}});
    }
    if (command == "recover" && positional.size() == 1) {
        if (!project.recover(bundle.toStdString())) return fail("No valid recovery document");
        if (!project.save(bundle.toStdString())) return fail("Could not save recovered state");
        return writeJson({{"ok", true}});
    }
    if (!project.open(bundle.toStdString())) return fail("Could not open the project bundle");

    if (command == "info" && positional.size() == 1) {
        return writeJson({{"ok", true}, {"tempo", project.tempo()},
            {"sampleRate", static_cast<double>(project.sampleRate())},
            {"timeSignature", QString("%1/%2").arg(project.timeSignatureNumerator()).arg(project.timeSignatureDenominator())},
            {"tracks", static_cast<double>(project.trackCount())},
            {"scenes", static_cast<double>(project.sceneCount())}, {"canUndo", project.canUndo()},
            {"canRedo", project.canRedo()}});
    }
    if (command == "tracks" && positional.size() == 1) {
        QJsonArray tracks;
        for (std::uint64_t index = 0; index < project.trackCount(); ++index) {
            tracks.append(QJsonObject{{"index", static_cast<double>(index)},
                {"name", QString::fromStdString(project.trackName(index))},
                {"kind", trackKindName(project.trackKind(index))},
                {"volumeDb", project.trackVolumeDb(index)}, {"pan", project.trackPan(index)},
                {"mute", project.trackMuted(index)}, {"solo", project.trackSolo(index)},
                {"arm", project.trackArmed(index)}, {"color", project.trackColorIndex(index)}});
        }
        return writeJson({{"ok", true}, {"tracks", tracks}});
    }
    if (command == "clips" && (positional.size() == 1 || positional.size() == 2)) {
        std::uint64_t selectedTrack = 0;
        if (positional.size() == 2 && !indexNumber(positional[1], selectedTrack))
            return fail("Invalid track index");
        if (positional.size() == 2 && selectedTrack >= project.trackCount())
            return fail("Invalid track index");
        const std::uint64_t first = positional.size() == 2 ? selectedTrack : 0;
        const std::uint64_t end = positional.size() == 2 ? selectedTrack + 1 : project.trackCount();
        QJsonArray clips;
        for (std::uint64_t track = first; track < end; ++track) {
            for (std::uint64_t scene = 0; scene < project.sceneCount(); ++scene) {
                if (!project.clipSlotOccupied(track, scene)) continue;
                const std::string media = project.clipMediaPath(track, scene);
                clips.append(QJsonObject{{"track", static_cast<double>(track)},
                    {"scene", static_cast<double>(scene)},
                    {"name", QString::fromStdString(project.clipName(track, scene))},
                    {"kind", media.empty() ? "midi" : "audio"},
                    {"mediaPath", QString::fromStdString(media)}});
            }
        }
        return writeJson({{"ok", true}, {"clips", clips}});
    }

    bool changed = false;
    if (command == "set-tempo" && positional.size() == 2) {
        double tempo = 0.0;
        changed = number(positional[1], tempo) && project.setTempo(tempo);
    } else if (command == "add-track" && positional.size() >= 1 && positional.size() <= 3) {
        nylon::TrackKind kind = nylon::TrackKind::Audio;
        changed = (positional.size() == 1 || trackKind(positional[1], kind)) && project.addTrack(kind);
        if (changed && positional.size() == 3)
            changed = project.setTrackName(project.trackCount() - 1, positional[2].toStdString());
    } else if (command == "undo" && positional.size() == 1) {
        changed = project.undo();
    } else if (command == "redo" && positional.size() == 1) {
        changed = project.redo();
    } else if (command == "import-wave" && (positional.size() == 4 || positional.size() == 5)) {
        std::uint64_t track = 0;
        std::uint64_t scene = 0;
        double sourceTempo = project.tempo();
        if (!indexNumber(positional[1], track) || !indexNumber(positional[2], scene)
            || (positional.size() == 5 && !number(positional[4], sourceTempo)))
            return fail("Invalid track, scene, or source tempo");
        changed = project.importWave(
            track, scene, positional[3].toStdString(), sourceTempo);
    } else if (command == "place-clip" && positional.size() == 5) {
        std::uint64_t track = 0;
        std::uint64_t scene = 0;
        double start = 0.0;
        double length = 0.0;
        if (!indexNumber(positional[1], track) || !indexNumber(positional[2], scene)
            || !number(positional[3], start) || !number(positional[4], length))
            return fail("Invalid clip placement");
        changed = project.addArrangementClipFromSlot(track, scene, {start, length});
    } else if (command == "set-audio-gain" && positional.size() == 4) {
        std::uint64_t track = 0;
        std::uint64_t scene = 0;
        double gain = 0.0;
        if (!indexNumber(positional[1], track) || !indexNumber(positional[2], scene)
            || !number(positional[3], gain))
            return fail("Invalid audio clip gain");
        changed = project.setClipAudioGainDb(track, scene, gain);
    } else if (command == "set-audio-reverse" && positional.size() == 4) {
        std::uint64_t track = 0;
        std::uint64_t scene = 0;
        bool enabled = false;
        if (!indexNumber(positional[1], track) || !indexNumber(positional[2], scene)
            || !flag(positional[3], enabled))
            return fail("Invalid audio clip reverse setting");
        changed = project.setClipAudioReversed(track, scene, enabled);
    } else if (command == "set-audio-warp" && positional.size() == 5) {
        std::uint64_t track = 0;
        std::uint64_t scene = 0;
        bool enabled = false;
        double sourceTempo = 0.0;
        if (!indexNumber(positional[1], track) || !indexNumber(positional[2], scene)
            || !flag(positional[3], enabled) || !number(positional[4], sourceTempo))
            return fail("Invalid audio clip warp setting");
        changed = project.setClipAudioWarp(track, scene, enabled, sourceTempo);
    } else if (command == "bounce" && (positional.size() == 4 || positional.size() == 5)) {
        double start = 0.0;
        double end = 0.0;
        std::uint32_t rate = project.sampleRate();
        if (!number(positional[2], start) || !number(positional[3], end)
            || (positional.size() == 5 && !unsignedNumber(positional[4], rate)))
            return fail("Invalid bounce range or sample rate");
        nylon::BounceReport report{};
        if (!project.bounceWave(positional[1].toStdString(), start, end, rate, report))
            return fail("The bounce failed");
        return writeJson({{"ok", true}, {"frames", static_cast<double>(report.frames)},
            {"peakLeft", report.peakLeft}, {"peakRight", report.peakRight}});
    } else {
        return fail("Unknown command or invalid arguments");
    }
    if (!changed) return fail("The project rejected the command");
    if (!project.save(bundle.toStdString())) return fail("Could not save the project bundle");
    return writeJson({{"ok", true}});
}
}

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);
    app.setApplicationName("nylon-control");
    QCommandLineParser parser;
    parser.setApplicationDescription("Control a live Nylon window or edit a project bundle.");
    parser.addHelpOption();
    const QCommandLineOption endpoint("endpoint", "Endpoint supplied to the GUI with --control.", "name");
    const QCommandLineOption project("project", "Project bundle for direct commands.", "directory");
    parser.addOptions({endpoint, project});
    parser.addPositionalArgument("command", "Command to execute.");
    parser.addPositionalArgument("args", "Command arguments.", "[args...]");
    parser.setOptionsAfterPositionalArgumentsMode(
        QCommandLineParser::ParseAsPositionalArguments);
    parser.process(app);
    const QStringList positional = parser.positionalArguments();
    if (positional.isEmpty()) parser.showHelp(2);
    if (!parser.value(endpoint).isEmpty()) return remoteCommand(parser.value(endpoint), positional);
    return directCommand(parser.value(project), positional);
}
