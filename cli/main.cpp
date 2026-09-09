#include "nylon.hpp"

#include <QCommandLineParser>
#include <QCoreApplication>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLocalSocket>
#include <QStringList>
#include <QThread>
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

bool routingKind(const QString& text, nylon::RoutingKind& kind)
{
    if (text == "main") kind = nylon::RoutingKind::Main;
    else if (text == "send-pre") kind = nylon::RoutingKind::SendPreFader;
    else if (text == "send-post") kind = nylon::RoutingKind::SendPostFader;
    else if (text == "sidechain") kind = nylon::RoutingKind::Sidechain;
    else return false;
    return true;
}

QString routingKindName(nylon::RoutingKind kind)
{
    switch (kind) {
    case nylon::RoutingKind::Main: return "main";
    case nylon::RoutingKind::SendPreFader: return "send-pre";
    case nylon::RoutingKind::SendPostFader: return "send-post";
    case nylon::RoutingKind::Sidechain: return "sidechain";
    }
    return "main";
}

QString deviceKindName(nylon::DeviceKind kind)
{
    switch (kind) {
    case nylon::DeviceKind::Utility: return "utility";
    case nylon::DeviceKind::Equalizer: return "equalizer";
    case nylon::DeviceKind::Compressor: return "compressor";
    case nylon::DeviceKind::StereoDelay: return "delay";
    case nylon::DeviceKind::Limiter: return "limiter";
    }
    return "utility";
}

QString filterKindName(int kind)
{
    static const QStringList names{
        "low-pass", "high-pass", "band-pass", "notch", "all-pass", "peaking",
        "low-shelf", "high-shelf"};
    return kind >= 0 && kind < names.size() ? names[kind] : QString();
}

bool filterKind(const QString& text, float& kind)
{
    for (int index = 0; index < 8; ++index) {
        if (text == filterKindName(index)) {
            kind = static_cast<float>(index);
            return true;
        }
    }
    return false;
}

bool parameter(const QString& text, float& value)
{
    double parsed = 0.0;
    if (!number(text, parsed)) return false;
    value = static_cast<float>(parsed);
    return std::isfinite(value);
}

bool parseTrackDevice(
    const QStringList& positional, int kindIndex, nylon::TrackDevice& device)
{
    const QString kind = positional[kindIndex];
    device.enabled = true;
    if (kind == "utility" && positional.size() == kindIndex + 4) {
        device.kind = nylon::DeviceKind::Utility;
        return parameter(positional[kindIndex + 1], device.parameters[0])
            && parameter(positional[kindIndex + 2], device.parameters[1])
            && parameter(positional[kindIndex + 3], device.parameters[2]);
    }
    if (kind == "equalizer" && positional.size() == kindIndex + 5) {
        device.kind = nylon::DeviceKind::Equalizer;
        return filterKind(positional[kindIndex + 1], device.parameters[0])
            && parameter(positional[kindIndex + 2], device.parameters[1])
            && parameter(positional[kindIndex + 3], device.parameters[2])
            && parameter(positional[kindIndex + 4], device.parameters[3]);
    }
    if (kind == "compressor" && positional.size() == kindIndex + 8) {
        device.kind = nylon::DeviceKind::Compressor;
        bool sidechain = false;
        for (int index = 0; index < 6; ++index) {
            if (!parameter(positional[kindIndex + 1 + index], device.parameters[index]))
                return false;
        }
        if (!flag(positional[kindIndex + 7], sidechain)) return false;
        device.parameters[6] = sidechain ? 1.0F : 0.0F;
        return true;
    }
    if (kind == "delay" && positional.size() == kindIndex + 4) {
        device.kind = nylon::DeviceKind::StereoDelay;
        return parameter(positional[kindIndex + 1], device.parameters[0])
            && parameter(positional[kindIndex + 2], device.parameters[1])
            && parameter(positional[kindIndex + 3], device.parameters[2]);
    }
    if (kind == "limiter" && positional.size() == kindIndex + 4) {
        device.kind = nylon::DeviceKind::Limiter;
        return parameter(positional[kindIndex + 1], device.parameters[0])
            && parameter(positional[kindIndex + 2], device.parameters[1])
            && parameter(positional[kindIndex + 3], device.parameters[2]);
    }
    return false;
}

QJsonObject deviceJson(const nylon::TrackDevice& device, std::uint64_t index)
{
    QJsonObject parameters;
    switch (device.kind) {
    case nylon::DeviceKind::Utility:
        parameters = {{"gainDb", device.parameters[0]}, {"width", device.parameters[1]},
            {"balance", device.parameters[2]}};
        break;
    case nylon::DeviceKind::Equalizer:
        parameters = {{"filter", filterKindName(static_cast<int>(device.parameters[0]))},
            {"frequency", device.parameters[1]}, {"q", device.parameters[2]},
            {"gainDb", device.parameters[3]}};
        break;
    case nylon::DeviceKind::Compressor:
        parameters = {{"thresholdDb", device.parameters[0]}, {"ratio", device.parameters[1]},
            {"kneeDb", device.parameters[2]}, {"attackSeconds", device.parameters[3]},
            {"releaseSeconds", device.parameters[4]}, {"makeupDb", device.parameters[5]},
            {"externalSidechain", device.parameters[6] == 1.0F}};
        break;
    case nylon::DeviceKind::StereoDelay:
        parameters = {{"delaySeconds", device.parameters[0]},
            {"feedback", device.parameters[1]}, {"mix", device.parameters[2]}};
        break;
    case nylon::DeviceKind::Limiter:
        parameters = {{"ceilingDb", device.parameters[0]},
            {"releaseSeconds", device.parameters[1]},
            {"lookaheadSeconds", device.parameters[2]}};
        break;
    }
    return {{"index", static_cast<double>(index)}, {"kind", deviceKindName(device.kind)},
        {"enabled", device.enabled}, {"parameters", parameters}};
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

int listDevices(bool input)
{
    QJsonArray devices;
    const auto found = input ? nylon::AudioEngine::inputDevices() : nylon::AudioEngine::devices();
    for (const auto& device : found) {
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
    if (command == "devices" && positional.size() == 1) return listDevices(false);
    if (command == "input-devices" && positional.size() == 1) return listDevices(true);
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
                {"arm", project.trackArmed(index)}, {"color", project.trackColorIndex(index)},
                {"latencyFrames", static_cast<double>(project.trackLatencyFrames(index))}});
        }
        return writeJson({{"ok", true}, {"tracks", tracks}});
    }
    if (command == "routes" && positional.size() == 1) {
        QJsonArray routes;
        const auto projectRoutes = project.routes();
        for (std::size_t index = 0; index < projectRoutes.size(); ++index) {
            const auto& route = projectRoutes[index];
            routes.append(QJsonObject{{"index", static_cast<double>(index)},
                {"source", static_cast<double>(route.source)},
                {"destination", static_cast<double>(route.destination)},
                {"kind", routingKindName(route.kind)}, {"gain", route.gain}});
        }
        return writeJson({{"ok", true}, {"routes", routes}});
    }
    if (command == "track-devices" && positional.size() == 2) {
        std::uint64_t track = 0;
        if (!indexNumber(positional[1], track) || track >= project.trackCount())
            return fail("Invalid track index");
        QJsonArray devices;
        const auto chain = project.trackDevices(track);
        for (std::size_t index = 0; index < chain.size(); ++index)
            devices.append(deviceJson(chain[index], index));
        return writeJson({{"ok", true}, {"track", static_cast<double>(track)},
            {"devices", devices}});
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
    if (command == "record" && positional.size() >= 4 && positional.size() <= 7) {
        std::uint64_t track = 0;
        std::uint64_t scene = 0;
        std::uint64_t device = 0;
        std::uint32_t rate = project.sampleRate();
        std::uint32_t block = 256;
        double seconds = 0.0;
        if (!indexNumber(positional[1], track) || !indexNumber(positional[2], scene)
            || !number(positional[3], seconds) || seconds <= 0.0 || seconds > 86400.0)
            return fail("Invalid recording track, scene, or duration");
        if (positional.size() >= 5) {
            if (!indexNumber(positional[4], device)) return fail("Invalid input device");
        } else if (!nylon::AudioEngine::defaultInput(device)) {
            return fail("No default input device is available");
        }
        if (positional.size() >= 6 && !unsignedNumber(positional[5], rate))
            return fail("Invalid recording sample rate");
        if (positional.size() == 7 && !unsignedNumber(positional[6], block))
            return fail("Invalid recording block size");
        nylon::Recording recording;
        if (!recording.open(project, track, scene, device, rate, block))
            return fail("Could not open the recording input or project slot");
        if (!recording.start()) return fail("Could not start recording");
        QThread::msleep(static_cast<unsigned long>(std::ceil(seconds * 1000.0)));
        if (!recording.stop()) return fail("Could not stop recording");
        nylon::RecordingReport report{};
        if (!recording.finish(project, report)) return fail("Could not finish recording");
        if (!project.save(bundle.toStdString())) return fail("Could not save the project bundle");
        return writeJson({{"ok", true}, {"frames", static_cast<double>(report.frames)},
            {"sampleRate", static_cast<double>(report.sampleRate)},
            {"lengthBeats", report.lengthBeats},
            {"lostBlocks", static_cast<double>(report.lostBlocks)},
            {"lostFrames", static_cast<double>(report.lostFrames)}});
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
    } else if (command == "set-track-latency" && positional.size() == 3) {
        std::uint64_t track = 0;
        std::uint32_t frames = 0;
        if (!indexNumber(positional[1], track) || !unsignedNumber(positional[2], frames))
            return fail("Invalid track latency");
        changed = project.setTrackLatencyFrames(track, frames);
    } else if (command == "add-route" && (positional.size() == 4 || positional.size() == 5)) {
        std::uint64_t source = 0;
        std::uint64_t destination = 0;
        nylon::RoutingKind kind = nylon::RoutingKind::Main;
        double gain = 1.0;
        if (!indexNumber(positional[1], source) || !indexNumber(positional[2], destination)
            || !routingKind(positional[3], kind)
            || (positional.size() == 5 && !number(positional[4], gain)))
            return fail("Invalid route");
        changed = project.addRoute(source, destination, kind, static_cast<float>(gain));
    } else if (command == "delete-route" && positional.size() == 2) {
        std::uint64_t route = 0;
        if (!indexNumber(positional[1], route)) return fail("Invalid route index");
        changed = project.deleteRoute(route);
    } else if (command == "add-device" && positional.size() >= 3) {
        std::uint64_t track = 0;
        nylon::TrackDevice device;
        if (!indexNumber(positional[1], track) || !parseTrackDevice(positional, 2, device))
            return fail("Invalid track device");
        changed = project.addTrackDevice(track, device);
    } else if (command == "set-device" && positional.size() >= 4) {
        std::uint64_t track = 0;
        std::uint64_t index = 0;
        nylon::TrackDevice device;
        if (!indexNumber(positional[1], track) || !indexNumber(positional[2], index)
            || !parseTrackDevice(positional, 3, device))
            return fail("Invalid track device");
        const auto chain = project.trackDevices(track);
        if (index >= chain.size()) return fail("Invalid device index");
        device.enabled = chain[static_cast<std::size_t>(index)].enabled;
        changed = project.setTrackDevice(track, index, device);
    } else if (command == "set-device-enabled" && positional.size() == 4) {
        std::uint64_t track = 0;
        std::uint64_t index = 0;
        bool enabled = false;
        if (!indexNumber(positional[1], track) || !indexNumber(positional[2], index)
            || !flag(positional[3], enabled))
            return fail("Invalid device state");
        auto chain = project.trackDevices(track);
        if (index >= chain.size()) return fail("Invalid device index");
        chain[static_cast<std::size_t>(index)].enabled = enabled;
        changed = project.setTrackDevice(track, index, chain[static_cast<std::size_t>(index)]);
    } else if (command == "move-device" && positional.size() == 4) {
        std::uint64_t track = 0;
        std::uint64_t from = 0;
        std::uint64_t to = 0;
        if (!indexNumber(positional[1], track) || !indexNumber(positional[2], from)
            || !indexNumber(positional[3], to))
            return fail("Invalid device position");
        changed = project.moveTrackDevice(track, from, to);
    } else if (command == "delete-device" && positional.size() == 3) {
        std::uint64_t track = 0;
        std::uint64_t index = 0;
        if (!indexNumber(positional[1], track) || !indexNumber(positional[2], index))
            return fail("Invalid device index");
        changed = project.deleteTrackDevice(track, index);
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
