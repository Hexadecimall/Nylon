#pragma once

#include <QObject>

namespace nylon {

// Owns one core project handle and forwards edits to it. Every mutating
// call emits changed() when the core accepted it so views can repaint from
// the new snapshot.
class ProjectBridge : public QObject {
    Q_OBJECT
public:
    explicit ProjectBridge(QObject* parent = nullptr);
    ~ProjectBridge() override;

    ProjectBridge(const ProjectBridge&) = delete;
    ProjectBridge& operator=(const ProjectBridge&) = delete;

    // False when the core failed to allocate a project.
    bool isValid() const { return m_handle != nullptr; }

    double tempo() const;
    quint64 trackCount() const;

public slots:
    bool setTempo(double bpm);
    bool addTrack();
    bool undo();
    bool redo();

signals:
    void changed();

private:
    void* m_handle = nullptr;
};

} // namespace nylon
