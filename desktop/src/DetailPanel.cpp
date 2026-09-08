#include "DetailPanel.h"

#include "PanelPaint.h"
#include "ProjectBridge.h"
#include "Theme.h"

#include <QPaintEvent>
#include <QPainter>
#include "PianoRoll.h"
#include "widgets/FlatButton.h"

#include <QHBoxLayout>
#include <QFormLayout>
#include <QLabel>
#include <QStackedWidget>
#include <QVBoxLayout>

namespace nylon {

DetailPanel::DetailPanel(ProjectBridge* bridge, const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
    , m_bridge(bridge)
    , m_title(new QLabel(this))
    , m_summary(new QWidget(this))
    , m_kind(new QLabel(m_summary))
    , m_volume(new QLabel(m_summary))
    , m_pan(new QLabel(m_summary))
    , m_state(new QLabel(m_summary))
    , m_color(new QLabel(m_summary))
    , m_clipTab(new FlatButton(theme, this))
    , m_deviceTab(new FlatButton(theme, this))
    , m_stack(new QStackedWidget(this))
    , m_clipEmpty(new QLabel(this))
    , m_clipPage(new QWidget(this))
    , m_clipTitle(new QLabel(this))
    , m_pianoRoll(new PianoRoll(bridge, theme, this))
    , m_deviceEmpty(new QLabel(this))
{
    setObjectName(QStringLiteral("detail"));
    setAutoFillBackground(false);

    m_title->setObjectName(QStringLiteral("detailTitle"));
    for (QLabel* value : {m_kind, m_volume, m_pan, m_state, m_color}) {
        value->setObjectName(QStringLiteral("secondary"));
        value->setTextInteractionFlags(Qt::TextSelectableByMouse);
    }
    m_clipTab->setText(tr("Clip"));
    m_clipTab->setCheckable(true);
    m_clipTab->setStatusTip(tr("Show the selected clip's properties."));
    m_deviceTab->setText(tr("Device"));
    m_deviceTab->setCheckable(true);
    m_deviceTab->setStatusTip(tr("Show the selected track's device chain."));

    m_clipEmpty->setObjectName(QStringLiteral("secondary"));
    m_clipEmpty->setAlignment(Qt::AlignCenter);
    m_clipEmpty->setWordWrap(true);
    m_deviceEmpty->setObjectName(QStringLiteral("secondary"));
    m_deviceEmpty->setAlignment(Qt::AlignCenter);
    m_deviceEmpty->setWordWrap(true);
    // Clip page: title row over the piano roll, or the empty label.
    m_clipTitle->setObjectName(QStringLiteral("secondary"));
    auto* clipLayout = new QVBoxLayout(m_clipPage);
    clipLayout->setContentsMargins(0, 0, 0, 0);
    clipLayout->setSpacing(4);
    clipLayout->addWidget(m_clipTitle);
    clipLayout->addWidget(m_pianoRoll, 1);
    clipLayout->addWidget(m_clipEmpty, 1);
    connect(m_pianoRoll, &PianoRoll::message, this, [this](const QString& text) { m_clipTitle->setText(text); });
    m_stack->addWidget(m_clipPage);
    m_stack->addWidget(m_deviceEmpty);

    auto* header = new QHBoxLayout;
    header->setContentsMargins(0, 0, 0, 0);
    header->setSpacing(0);
    header->addWidget(m_clipTab);
    header->addWidget(m_deviceTab);
    header->addSpacing(8);
    header->addWidget(m_title, 1);

    auto* summaryLayout = new QFormLayout(m_summary);
    summaryLayout->setContentsMargins(0, 10, 0, 10);
    summaryLayout->setHorizontalSpacing(12);
    summaryLayout->setVerticalSpacing(6);
    summaryLayout->setFieldGrowthPolicy(QFormLayout::AllNonFixedFieldsGrow);
    summaryLayout->addRow(tr("Type"), m_kind);
    summaryLayout->addRow(tr("Volume"), m_volume);
    summaryLayout->addRow(tr("Pan"), m_pan);
    summaryLayout->addRow(tr("State"), m_state);
    summaryLayout->addRow(tr("Color"), m_color);

    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);
    layout->addLayout(header);
    layout->addWidget(m_summary);
    layout->addWidget(m_stack, 1);

    connect(m_clipTab, &FlatButton::clicked, this, &DetailPanel::showClipPage);
    connect(m_deviceTab, &FlatButton::clicked, this, &DetailPanel::showDevicePage);
    connect(m_bridge, &ProjectBridge::changed, this, &DetailPanel::refresh);

    setTheme(theme);
    showPage(Page::Device);
}

void DetailPanel::setTheme(const Theme* theme)
{
    m_theme = theme;
    m_clipTab->setTheme(theme);
    m_deviceTab->setTheme(theme);
    m_pianoRoll->setTheme(theme);
    const int pad = m_theme->metricInt(QStringLiteral("panel.padding"), 8);
    layout()->setContentsMargins(pad, pad, pad, pad);
    refresh();
}

void DetailPanel::setSelectedTrack(int index, const QString& name)
{
    m_track = index;
    m_trackName = name;
    refresh();
}

QString DetailPanel::headerText() const
{
    return m_title->text();
}

void DetailPanel::setSelectedClip(int track, int scene)
{
    m_clipTrack = track;
    m_clipScene = scene;
    if (track >= 0 && track != m_track) {
        m_track = track;
        m_trackName = m_bridge->trackName(static_cast<quint64>(track));
    }
    m_pianoRoll->setClip(track, scene);
    if (scene >= 0) {
        // Picking a slot brings up its editor, as clicking a clip does.
        showPage(Page::Clip);
    } else {
        refresh();
    }
}

void DetailPanel::showPage(Page page)
{
    m_page = page;
    m_clipTab->setChecked(page == Page::Clip);
    m_deviceTab->setChecked(page == Page::Device);
    m_stack->setCurrentIndex(page == Page::Clip ? 0 : 1);
    refresh();
}

void DetailPanel::refresh()
{
    if (m_track < 0) {
        m_title->setText(tr("No track selected"));
        m_summary->hide();
        m_clipEmpty->setText(tr("Select a clip slot to edit its clip."));
        m_deviceEmpty->setText(tr("Select a track to see its devices."));
        return;
    }
    const quint64 track = static_cast<quint64>(m_track);
    m_summary->show();
    m_title->setText(m_trackName);
    m_kind->setText(ProjectBridge::kindName(m_bridge->trackKind(track)));
    m_volume->setText(tr("%1 dB").arg(m_bridge->trackVolumeDb(track), 0, 'f', 1));
    const double pan = m_bridge->trackPan(track);
    m_pan->setText(qAbs(pan) < 0.005 ? tr("Center")
                                    : pan < 0.0 ? tr("%1 L").arg(qRound(-pan * 100.0))
                                                : tr("%1 R").arg(qRound(pan * 100.0)));
    QStringList states;
    if (m_bridge->trackMuted(track)) states.append(tr("Muted"));
    if (m_bridge->trackSolo(track)) states.append(tr("Solo"));
    if (m_bridge->trackArmed(track)) states.append(tr("Armed"));
    m_state->setText(states.isEmpty() ? tr("Active") : states.join(QStringLiteral(" / ")));
    m_color->setText(tr("Palette %1").arg(m_bridge->trackColorIndex(track) + 1));
    m_clipEmpty->setText(tr("%1 has no clip in the selected slot.").arg(m_trackName));
    m_deviceEmpty->setText(tr("%1 has no devices.\nDrop an instrument or effect here from the browser.").arg(m_trackName));
    const bool hasClip = m_clipTrack >= 0 && m_clipScene >= 0
        && m_bridge->clipSlotOccupied(static_cast<quint64>(m_clipTrack), static_cast<quint64>(m_clipScene));
    m_pianoRoll->setVisible(hasClip);
    m_clipEmpty->setVisible(!hasClip);
    if (hasClip) {
        const QString name = m_bridge->clipName(static_cast<quint64>(m_clipTrack), static_cast<quint64>(m_clipScene));
        const quint64 notes = m_bridge->clipNoteCount(static_cast<quint64>(m_clipTrack), static_cast<quint64>(m_clipScene));
        m_clipTitle->setText(tr("%1 on %2, scene %3, %n note(s)", nullptr, static_cast<int>(notes))
                                 .arg(name, m_trackName).arg(m_clipScene + 1));
    } else {
        m_clipTitle->clear();
    }
}

void DetailPanel::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    paint::panel(p, *m_theme, rect(), m_theme->color(QStringLiteral("detail.background")));
}

} // namespace nylon
