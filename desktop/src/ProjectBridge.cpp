#include "ProjectBridge.h"

#include "nylon.h"

namespace nylon {

ProjectBridge::ProjectBridge(QObject* parent)
    : QObject(parent)
    , m_handle(nylon_project_new())
{
}

ProjectBridge::~ProjectBridge()
{
    nylon_project_free(m_handle);
}

double ProjectBridge::tempo() const
{
    return nylon_project_tempo(m_handle);
}

quint64 ProjectBridge::trackCount() const
{
    return nylon_project_track_count(m_handle);
}

bool ProjectBridge::setTempo(double bpm)
{
    const bool ok = nylon_project_set_tempo(m_handle, bpm) != 0;
    if (ok) {
        emit changed();
    }
    return ok;
}

bool ProjectBridge::addTrack()
{
    const bool ok = nylon_project_add_track(m_handle) != 0;
    if (ok) {
        emit changed();
    }
    return ok;
}

bool ProjectBridge::undo()
{
    const bool ok = nylon_project_undo(m_handle) != 0;
    if (ok) {
        emit changed();
    }
    return ok;
}

bool ProjectBridge::redo()
{
    const bool ok = nylon_project_redo(m_handle) != 0;
    if (ok) {
        emit changed();
    }
    return ok;
}

} // namespace nylon
