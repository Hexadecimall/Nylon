#include "TransportBar.h"

#include "ProjectBridge.h"
#include "Theme.h"

#include <QDoubleSpinBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QPushButton>
#include <QSpacerItem>
#include <QToolButton>

namespace nylon {

TransportBar::TransportBar(ProjectBridge* bridge, QWidget* parent)
    : QWidget(parent)
    , m_bridge(bridge)
    , m_tempo(new QDoubleSpinBox(this))
    , m_addTrack(new QPushButton(tr("Add Track"), this))
    , m_undo(new QPushButton(tr("Undo"), this))
    , m_redo(new QPushButton(tr("Redo"), this))
    , m_session(new QToolButton(this))
    , m_arrangement(new QToolButton(this))
    , m_trackCount(new QLabel(this))
{
    setObjectName(QStringLiteral("transport"));

    m_tempo->setObjectName(QStringLiteral("tempo"));
    m_tempo->setDecimals(2);
    m_tempo->setRange(1.0, 9999.0);
    m_tempo->setSingleStep(1.0);
    m_tempo->setKeyboardTracking(false);
    m_tempo->setSuffix(tr(" BPM"));
    m_tempo->setToolTip(tr("Tempo"));
    m_tempo->setAlignment(Qt::AlignRight);

    m_session->setText(tr("Session"));
    m_session->setCheckable(true);
    m_session->setChecked(true);
    m_arrangement->setText(tr("Arrangement"));
    m_arrangement->setCheckable(true);

    m_trackCount->setObjectName(QStringLiteral("secondary"));

    auto* layout = new QHBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);
    layout->addWidget(m_tempo);
    layout->addSpacerItem(m_gapA = new QSpacerItem(0, 0));
    layout->addWidget(m_addTrack);
    layout->addWidget(m_undo);
    layout->addWidget(m_redo);
    layout->addSpacerItem(m_gapB = new QSpacerItem(0, 0));
    layout->addWidget(m_trackCount);
    layout->addStretch(1);
    layout->addWidget(m_session);
    layout->addWidget(m_arrangement);

    connect(m_tempo, &QDoubleSpinBox::editingFinished, this, &TransportBar::commitTempo);
    connect(m_addTrack, &QPushButton::clicked, this, [this] {
        if (!m_bridge->addTrack()) {
            emit message(tr("Could not add a track."));
        }
    });
    connect(m_undo, &QPushButton::clicked, this, [this] {
        if (!m_bridge->undo()) {
            emit message(tr("Nothing to undo."));
        }
    });
    connect(m_redo, &QPushButton::clicked, this, [this] {
        if (!m_bridge->redo()) {
            emit message(tr("Nothing to redo."));
        }
    });
    connect(m_session, &QToolButton::clicked, this, [this] {
        showSessionActive(true);
        emit sessionRequested();
    });
    connect(m_arrangement, &QToolButton::clicked, this, [this] {
        showSessionActive(false);
        emit arrangementRequested();
    });
    connect(m_bridge, &ProjectBridge::changed, this, &TransportBar::refresh);

    refresh();
}

void TransportBar::applyTheme(const Theme& theme)
{
    const int h = theme.metricInt(QStringLiteral("transport.height"), 28);
    setFixedHeight(h);
    const int pad = theme.metricInt(QStringLiteral("control.padding"), 4);
    layout()->setContentsMargins(pad, 0, pad, 0);
    const int gap = theme.metricInt(QStringLiteral("transport.spacing"), 8);
    m_gapA->changeSize(gap, 0);
    m_gapB->changeSize(gap, 0);
    layout()->invalidate();
    m_tempo->setFixedWidth(qMax(40, theme.metricInt(QStringLiteral("transport.tempo.width"), 96)));
}

void TransportBar::showSessionActive(bool session)
{
    m_session->setChecked(session);
    m_arrangement->setChecked(!session);
}

void TransportBar::refresh()
{
    const QSignalBlocker block(m_tempo);
    m_tempo->setValue(m_bridge->tempo());
    const quint64 n = m_bridge->trackCount();
    m_trackCount->setText(n == 1 ? tr("1 track") : tr("%1 tracks").arg(n));
}

void TransportBar::commitTempo()
{
    const double requested = m_tempo->value();
    if (qFuzzyCompare(requested, m_bridge->tempo())) {
        return;
    }
    if (!m_bridge->setTempo(requested)) {
        emit message(tr("Tempo %1 is outside the supported range.").arg(requested, 0, 'f', 2));
        refresh();
    }
}

} // namespace nylon
