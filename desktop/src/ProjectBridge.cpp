#include "ProjectBridge.h"

namespace nylon {

namespace {

// Wraps a setter so the change signal fires only when the core accepted it.
template <typename F>
bool commit(ProjectBridge* bridge, F&& call)
{
    const bool ok = call();
    if (ok) {
        emit bridge->changed();
    }
    return ok;
}

} // namespace

ProjectBridge::ProjectBridge(QObject* parent)
    : QObject(parent)
{
}

ProjectBridge::~ProjectBridge() = default;

bool ProjectBridge::save(const QString& bundleDirectory)
{
    if (!m_project.save(bundleDirectory.toStdString())) {
        return false;
    }
    m_bundlePath = bundleDirectory;
    emit changed();
    return true;
}

bool ProjectBridge::open(const QString& bundleDirectory)
{
    if (!m_project.open(bundleDirectory.toStdString())) {
        return false;
    }
    m_bundlePath = bundleDirectory;
    emit changed();
    return true;
}

bool ProjectBridge::reset()
{
    m_bundlePath.clear();
    if (!m_project.valid()) {
        m_project = Project();
        if (!m_project.valid()) {
            return false;
        }
        emit changed();
        return true;
    }
    return commit(this, [this] { return m_project.reset(); });
}

double ProjectBridge::tempo() const { return m_project.tempo(); }
quint64 ProjectBridge::trackCount() const { return m_project.trackCount(); }
bool ProjectBridge::canUndo() const { return m_project.canUndo(); }
bool ProjectBridge::canRedo() const { return m_project.canRedo(); }
int ProjectBridge::timeSignatureNumerator() const { return m_project.timeSignatureNumerator(); }
int ProjectBridge::timeSignatureDenominator() const { return m_project.timeSignatureDenominator(); }
unsigned int ProjectBridge::sampleRate() const { return m_project.sampleRate(); }

QString ProjectBridge::trackName(quint64 index) const
{
    return QString::fromStdString(m_project.trackName(index));
}

ProjectBridge::TrackKind ProjectBridge::trackKind(quint64 index) const { return m_project.trackKind(index); }

QString ProjectBridge::kindName(TrackKind kind)
{
    switch (kind) {
    case TrackKind::Audio:
        return tr("Audio");
    case TrackKind::Midi:
        return tr("MIDI");
    case TrackKind::Return:
        return tr("Return");
    case TrackKind::Master:
        return tr("Master");
    case TrackKind::Group:
        return tr("Group");
    case TrackKind::Cue:
        return tr("Cue");
    }
    return QString();
}

double ProjectBridge::trackVolumeDb(quint64 index) const { return m_project.trackVolumeDb(index); }
double ProjectBridge::trackPan(quint64 index) const { return m_project.trackPan(index); }
bool ProjectBridge::trackMuted(quint64 index) const { return m_project.trackMuted(index); }
bool ProjectBridge::trackSolo(quint64 index) const { return m_project.trackSolo(index); }
bool ProjectBridge::trackArmed(quint64 index) const { return m_project.trackArmed(index); }
int ProjectBridge::trackColorIndex(quint64 index) const { return m_project.trackColorIndex(index); }
quint64 ProjectBridge::sceneCount() const { return m_project.sceneCount(); }
QString ProjectBridge::sceneName(quint64 scene) const { return QString::fromStdString(m_project.sceneName(scene)); }
bool ProjectBridge::clipSlotOccupied(quint64 track, quint64 scene) const
{
    return m_project.clipSlotOccupied(track, scene);
}
QString ProjectBridge::clipName(quint64 track, quint64 scene) const
{
    return QString::fromStdString(m_project.clipName(track, scene));
}
int ProjectBridge::clipColorIndex(quint64 track, quint64 scene) const
{
    return m_project.clipColorIndex(track, scene);
}
BeatRange ProjectBridge::clipLoop(quint64 track, quint64 scene) const
{
    return m_project.clipLoop(track, scene);
}
quint64 ProjectBridge::clipNoteCount(quint64 track, quint64 scene) const
{
    return m_project.clipNoteCount(track, scene);
}
bool ProjectBridge::clipNote(quint64 track, quint64 scene, quint64 index, MidiNote& note) const
{
    return m_project.clipNote(track, scene, index, note);
}
quint64 ProjectBridge::arrangementClipCount(quint64 track) const
{
    return m_project.arrangementClipCount(track);
}
bool ProjectBridge::arrangementClipRange(quint64 track, quint64 index, BeatRange& range) const
{
    return m_project.arrangementClipRange(track, index, range);
}

bool ProjectBridge::setTempo(double bpm) { return commit(this, [&] { return m_project.setTempo(bpm); }); }
bool ProjectBridge::setTimeSignature(int numerator, int denominator)
{
    return commit(this, [&] { return m_project.setTimeSignature(numerator, denominator); });
}
bool ProjectBridge::setSampleRate(unsigned int rate) { return commit(this, [&] { return m_project.setSampleRate(rate); }); }
bool ProjectBridge::addTrack() { return commit(this, [this] { return m_project.addTrack(); }); }
bool ProjectBridge::addTrack(TrackKind kind) { return commit(this, [&] { return m_project.addTrack(kind); }); }
bool ProjectBridge::deleteTrack(quint64 index) { return commit(this, [&] { return m_project.deleteTrack(index); }); }
bool ProjectBridge::setTrackName(quint64 index, const QString& name)
{
    return commit(this, [&] { return m_project.setTrackName(index, name.toStdString()); });
}
bool ProjectBridge::setTrackVolumeDb(quint64 index, double db)
{
    return commit(this, [&] { return m_project.setTrackVolumeDb(index, db); });
}
bool ProjectBridge::setTrackPan(quint64 index, double pan) { return commit(this, [&] { return m_project.setTrackPan(index, pan); }); }
bool ProjectBridge::setTrackMuted(quint64 index, bool muted)
{
    return commit(this, [&] { return m_project.setTrackMuted(index, muted); });
}
bool ProjectBridge::setTrackSolo(quint64 index, bool solo) { return commit(this, [&] { return m_project.setTrackSolo(index, solo); }); }
bool ProjectBridge::setTrackArmed(quint64 index, bool armed)
{
    return commit(this, [&] { return m_project.setTrackArmed(index, armed); });
}
bool ProjectBridge::setTrackColorIndex(quint64 index, int color)
{
    return commit(this, [&] { return m_project.setTrackColorIndex(index, color); });
}
bool ProjectBridge::createScene(const QString& name)
{
    return commit(this, [&] { return m_project.createScene(name.toStdString()); });
}
bool ProjectBridge::deleteScene(quint64 scene)
{
    return commit(this, [&] { return m_project.deleteScene(scene); });
}
bool ProjectBridge::setSceneName(quint64 scene, const QString& name)
{
    return commit(this, [&] { return m_project.setSceneName(scene, name.toStdString()); });
}
bool ProjectBridge::createMidiClip(quint64 track, quint64 scene, double lengthBeats)
{
    return commit(this, [&] { return m_project.createMidiClip(track, scene, lengthBeats); });
}
bool ProjectBridge::deleteClip(quint64 track, quint64 scene)
{
    return commit(this, [&] { return m_project.deleteClip(track, scene); });
}
bool ProjectBridge::setClipName(quint64 track, quint64 scene, const QString& name)
{
    return commit(this, [&] { return m_project.setClipName(track, scene, name.toStdString()); });
}
bool ProjectBridge::setClipColorIndex(quint64 track, quint64 scene, int color)
{
    return commit(this, [&] { return m_project.setClipColorIndex(track, scene, color); });
}
bool ProjectBridge::setClipLoop(quint64 track, quint64 scene, BeatRange range)
{
    return commit(this, [&] { return m_project.setClipLoop(track, scene, range); });
}
bool ProjectBridge::addClipNote(quint64 track, quint64 scene, MidiNote note)
{
    return commit(this, [&] { return m_project.addClipNote(track, scene, note); });
}
bool ProjectBridge::removeClipNote(quint64 track, quint64 scene, quint64 index)
{
    return commit(this, [&] { return m_project.removeClipNote(track, scene, index); });
}
bool ProjectBridge::moveClipNote(quint64 track, quint64 scene, quint64 index, MidiNote note)
{
    return commit(this, [&] { return m_project.moveClipNote(track, scene, index, note); });
}
bool ProjectBridge::addArrangementClipFromSlot(quint64 track, quint64 scene, BeatRange range)
{
    return commit(this, [&] { return m_project.addArrangementClipFromSlot(track, scene, range); });
}
bool ProjectBridge::removeArrangementClip(quint64 track, quint64 index)
{
    return commit(this, [&] { return m_project.removeArrangementClip(track, index); });
}
bool ProjectBridge::setArrangementClipRange(quint64 track, quint64 index, BeatRange range)
{
    return commit(this, [&] { return m_project.setArrangementClipRange(track, index, range); });
}
bool ProjectBridge::undo() { return commit(this, [this] { return m_project.undo(); }); }
bool ProjectBridge::redo() { return commit(this, [this] { return m_project.redo(); }); }

} // namespace nylon
