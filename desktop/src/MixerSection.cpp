#include "MixerSection.h"

#include "MixerStrip.h"
#include "ProjectBridge.h"
#include "Theme.h"

#include "PanelPaint.h"

#include <QPainter>
#include <QScrollBar>

namespace nylon {

MixerSection::MixerSection(ProjectBridge* bridge, const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_bridge(bridge)
    , m_theme(theme)
    , m_stripHost(new QWidget(this))
    , m_master(new MixerStrip(theme, MixerStrip::Kind::Master, this))
{
    setObjectName(QStringLiteral("mixer"));
    m_master->setName(tr("Master"));
    m_master->setTrackIndex(-1);
    connect(m_bridge, &ProjectBridge::changed, this, &MixerSection::rebuild);
    rebuild();
}

void MixerSection::setTheme(const Theme* theme)
{
    m_theme = theme;
    m_master->setTheme(theme);
    for (MixerStrip* s : m_strips) {
        s->setTheme(theme);
    }
    relayout();
    update();
}

void MixerSection::followScrollBar(QScrollBar* bar)
{
    if (m_scroll) {
        disconnect(m_scroll, nullptr, this, nullptr);
    }
    m_scroll = bar;
    if (m_scroll) {
        connect(m_scroll, &QScrollBar::valueChanged, this, [this] { relayout(); });
    }
    relayout();
}

QSize MixerSection::sizeHint() const
{
    return QSize(400, m_theme->metricInt(QStringLiteral("mixer.height"), 170));
}

void MixerSection::selectTrack(int index)
{
    if (index >= m_strips.size()) {
        index = -1;
    }
    if (m_selected == index) {
        return;
    }
    m_selected = index;
    for (int i = 0; i < m_strips.size(); ++i) {
        m_strips[i]->setSelected(i == index);
    }
    emit trackSelected(index);
}

void MixerSection::rebuild()
{
    const int wanted = static_cast<int>(qMin<quint64>(m_bridge->trackCount(), 4096));
    while (m_strips.size() > wanted) {
        delete m_strips.takeLast();
    }
    while (m_strips.size() < wanted) {
        auto* s = new MixerStrip(m_theme, MixerStrip::Kind::Track, m_stripHost);
        const int index = static_cast<int>(m_strips.size());
        s->setTrackIndex(index);
        connect(s, &MixerStrip::selected, this, &MixerSection::selectTrack);
        connect(s, &MixerStrip::volumeChanged, this, [this](int track, double db) {
            if (!m_bridge->setTrackVolumeDb(static_cast<quint64>(track), db)) {
                syncStrip(track);
            }
        });
        connect(s, &MixerStrip::panChanged, this, [this](int track, double pan) {
            if (!m_bridge->setTrackPan(static_cast<quint64>(track), pan)) {
                syncStrip(track);
            }
        });
        connect(s, &MixerStrip::activeToggled, this, [this](int track, bool on) {
            if (!m_bridge->setTrackMuted(static_cast<quint64>(track), !on)) {
                syncStrip(track);
            }
        });
        connect(s, &MixerStrip::soloToggled, this, [this](int track, bool on) {
            if (!m_bridge->setTrackSolo(static_cast<quint64>(track), on)) {
                syncStrip(track);
            }
        });
        connect(s, &MixerStrip::armToggled, this, [this](int track, bool on) {
            if (!m_bridge->setTrackArmed(static_cast<quint64>(track), on)) {
                syncStrip(track);
            }
        });
        s->setInteractive(ProjectBridge::isMixerAvailable());
        s->show();
        m_strips.append(s);
    }
    for (int i = 0; i < m_strips.size(); ++i) {
        syncStrip(i);
        m_strips[i]->setSelected(i == m_selected);
    }
    if (m_selected >= m_strips.size()) {
        selectTrack(m_strips.isEmpty() ? -1 : static_cast<int>(m_strips.size()) - 1);
    }
    relayout();
}

void MixerSection::syncStrip(int index)
{
    MixerStrip* s = m_strips.value(index);
    if (!s) {
        return;
    }
    const auto i = static_cast<quint64>(index);
    s->setName(m_bridge->trackName(i));
    const int color = m_bridge->trackColorIndex(i);
    s->setColor(m_theme->trackColor(color >= 0 ? color : index));
    s->setState(m_bridge->trackVolumeDb(i), m_bridge->trackPan(i), !m_bridge->trackMuted(i),
        m_bridge->trackSolo(i), m_bridge->trackArmed(i));
}

void MixerSection::resizeEvent(QResizeEvent* event)
{
    QWidget::resizeEvent(event);
    relayout();
}

void MixerSection::relayout()
{
    const int sep = qMax(0, m_theme->metricInt(QStringLiteral("separator"), 1));
    const int stripW = m_theme->metricInt(QStringLiteral("session.slot.width"), 96);
    const int masterW = m_theme->metricInt(QStringLiteral("session.master.width"), 72);
    const int h = height();
    const int scroll = m_scroll ? m_scroll->value() : 0;
    // The master strip follows the grid's master column, but never leaves
    // the visible area.
    const qint64 masterX = qint64(m_strips.size()) * (stripW + sep) - scroll;
    const qint64 maxX = qMax(0, width() - masterW);
    const int mx = static_cast<int>(qBound<qint64>(0, masterX, maxX));
    m_master->setGeometry(mx, 0, masterW, h);
    m_stripHost->setGeometry(0, 0, qMax(0, mx - sep), h);
    for (int i = 0; i < m_strips.size(); ++i) {
        m_strips[i]->setGeometry(i * (stripW + sep) - scroll, 0, stripW, h);
    }
}

void MixerSection::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.fillRect(rect(), m_theme->color(QStringLiteral("background")));
}

} // namespace nylon
