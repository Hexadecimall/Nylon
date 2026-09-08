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
bool ProjectBridge::undo() { return commit(this, [this] { return m_project.undo(); }); }
bool ProjectBridge::redo() { return commit(this, [this] { return m_project.redo(); }); }

} // namespace nylon
