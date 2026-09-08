#include "MixerStrip.h"

#include "Theme.h"
#include "widgets/Fader.h"
#include "widgets/FlatButton.h"
#include "widgets/Knob.h"
#include "widgets/LevelMeter.h"

#include <QHBoxLayout>
#include <QLabel>
#include "PanelPaint.h"

#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QVBoxLayout>

#include <limits>

namespace nylon {

MixerStrip::MixerStrip(const Theme* theme, Kind kind, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
    , m_kind(kind)
    , m_name(new QLabel(this))
    , m_activator(new FlatButton(theme, this))
    , m_solo(new FlatButton(theme, this))
    , m_arm(new FlatButton(theme, this))
    , m_pan(new Knob(theme, this))
    , m_fader(new Fader(theme, this))
    , m_meter(new LevelMeter(theme, this))
    , m_volume(new QLabel(this))
{
    setObjectName(QStringLiteral("mixerStrip"));
    m_name->setAlignment(Qt::AlignCenter);
    m_name->setObjectName(QStringLiteral("stripName"));

    m_activator->setText(kind == Kind::Master ? tr("M") : QString());
    m_activator->setCheckable(true);
    m_activator->setChecked(true);
    m_activator->setActiveColorKey(QStringLiteral("state.on"));
    m_activator->setStatusTip(tr("Track activator. Off mutes the track."));
    m_solo->setText(tr("S"));
    m_solo->setCheckable(true);
    m_solo->setActiveColorKey(QStringLiteral("state.solo"));
    m_solo->setStatusTip(tr("Solo. Only soloed tracks are heard."));
    m_arm->setGlyph(FlatButton::Glyph::Circle);
    m_arm->setCheckable(true);
    m_arm->setActiveColorKey(QStringLiteral("state.arm"));
    m_arm->setStatusTip(tr("Arm the track for recording."));

    m_pan->setRange(-1.0, 1.0);
    m_pan->setDefaultValue(0.0);
    m_pan->setValue(0.0);
    m_pan->setBipolar(true);
    m_pan->setDisplayText(tr("C"));
    m_pan->setStatusTip(tr("Pan. Drag up or down; double-click to center."));

    m_fader->setStatusTip(tr("Track volume in dB. Double-click resets to 0 dB."));
    m_fader->setShowScale(false);
    m_meter->setStatusTip(tr("Output level."));
    m_volume->setAlignment(Qt::AlignCenter);
    m_volume->setObjectName(QStringLiteral("secondary"));

    auto* buttons = new QHBoxLayout;
    buttons->setContentsMargins(0, 0, 0, 0);
    buttons->setSpacing(2);
    buttons->addWidget(m_activator);
    if (kind == Kind::Track) {
        buttons->addWidget(m_solo);
        buttons->addWidget(m_arm);
    } else {
        m_solo->hide();
        m_arm->hide();
    }

    auto* faderRow = new QHBoxLayout;
    faderRow->setContentsMargins(0, 0, 0, 0);
    faderRow->setSpacing(2);
    faderRow->addStretch(1);
    faderRow->addWidget(m_fader);
    faderRow->addWidget(m_meter);
    faderRow->addStretch(1);

    auto* layout = new QVBoxLayout(this);
    layout->setSpacing(3);
    layout->addWidget(m_name);
    layout->addLayout(buttons);
    layout->addWidget(m_pan, 0, Qt::AlignHCenter);
    layout->addLayout(faderRow, 1);
    layout->addWidget(m_volume);

    connect(m_fader, &Fader::valueChanged, this, [this] { m_volume->setText(m_fader->displayText()); });
    connect(m_fader, &Fader::dragFinished, this, [this] {
        if (!m_syncing) {
            emit volumeChanged(m_index, m_fader->value() <= m_fader->minimum()
                    ? -std::numeric_limits<double>::infinity()
                    : m_fader->value());
        }
    });
    connect(m_pan, &Knob::valueChanged, this, [this](double v) {
        const int pct = qRound(qAbs(v) * 50.0);
        m_pan->setDisplayText(pct == 0 ? tr("C") : (v < 0 ? tr("%1L").arg(pct) : tr("%1R").arg(pct)));
    });
    connect(m_pan, &Knob::dragFinished, this, [this] {
        if (!m_syncing) {
            emit panChanged(m_index, m_pan->value());
        }
    });
    connect(m_activator, &FlatButton::toggled, this, [this](bool on) {
        if (!m_syncing) {
            emit activeToggled(m_index, on);
        }
    });
    connect(m_solo, &FlatButton::toggled, this, [this](bool on) {
        if (!m_syncing) {
            emit soloToggled(m_index, on);
        }
    });
    connect(m_arm, &FlatButton::toggled, this, [this](bool on) {
        if (!m_syncing) {
            emit armToggled(m_index, on);
        }
    });

    m_volume->setText(m_fader->displayText());
    setInteractive(false);
    setTheme(theme);
}

void MixerStrip::setTheme(const Theme* theme)
{
    m_theme = theme;
    m_activator->setTheme(theme);
    m_solo->setTheme(theme);
    m_arm->setTheme(theme);
    m_pan->setTheme(theme);
    m_fader->setTheme(theme);
    m_meter->setTheme(theme);
    const int h = m_theme->metricInt(QStringLiteral("strip.button.height"), 16);
    m_activator->setFixedHeight(h);
    m_solo->setFixedHeight(h);
    m_arm->setFixedHeight(h);
    m_arm->setFixedWidth(h + 4);
    const int pad = m_theme->metricInt(QStringLiteral("control.padding"), 4);
    layout()->setContentsMargins(pad, pad + 3, pad, pad);
    updateGeometry();
    update();
}

void MixerStrip::setTrackIndex(int index)
{
    m_index = index;
}

void MixerStrip::setName(const QString& name)
{
    m_name->setText(name);
}

QString MixerStrip::name() const
{
    return m_name->text();
}

void MixerStrip::setColor(const QColor& color)
{
    m_color = color;
    update();
}

void MixerStrip::setSelected(bool selected)
{
    if (m_selected == selected) {
        return;
    }
    m_selected = selected;
    update();
}

void MixerStrip::setState(double volumeDb, double pan, bool active, bool solo, bool armed)
{
    m_syncing = true;
    m_fader->setValue(std::isfinite(volumeDb) ? volumeDb : m_fader->minimum());
    m_volume->setText(m_fader->displayText());
    m_pan->setValue(pan);
    m_activator->setChecked(active);
    m_solo->setChecked(solo);
    m_arm->setChecked(armed);
    m_syncing = false;
}

void MixerStrip::setInteractive(bool interactive)
{
    m_interactive = interactive;
    const QString why = tr("Not available until the core exposes mixer state for this track.");
    for (QWidget* w : {static_cast<QWidget*>(m_activator), static_cast<QWidget*>(m_solo),
             static_cast<QWidget*>(m_arm), static_cast<QWidget*>(m_pan), static_cast<QWidget*>(m_fader)}) {
        w->setEnabled(interactive);
        w->setToolTip(interactive ? QString() : why);
    }
}

QSize MixerStrip::sizeHint() const
{
    const int w = m_kind == Kind::Master ? m_theme->metricInt(QStringLiteral("session.master.width"), 72)
                                         : m_theme->metricInt(QStringLiteral("session.slot.width"), 96);
    return QSize(w, m_theme->metricInt(QStringLiteral("mixer.height"), 170));
}

void MixerStrip::mousePressEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton) {
        emit selected(m_index);
    }
    QWidget::mousePressEvent(event);
}

void MixerStrip::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing, true);
    const QRect card = rect().adjusted(1, 1, -1, -1);
    const QPainterPath shape = paint::rounded(*m_theme, QRectF(card).adjusted(0.5, 0.5, -0.5, -0.5), false);
    p.fillPath(shape, m_selected ? m_theme->color(QStringLiteral("raised"))
                                 : m_theme->color(QStringLiteral("mixer.background")));
    const int band = m_theme->metricInt(QStringLiteral("session.header.band"), 2) + 1;
    if (m_color.isValid()) {
        p.save();
        p.setClipPath(shape);
        p.fillRect(QRect(card.x(), card.y(), card.width(), band), m_color);
        p.restore();
    }
    p.setPen(QPen(m_selected ? m_theme->color(QStringLiteral("accent")) : m_theme->color(QStringLiteral("panel.border")), 1));
    p.setBrush(Qt::NoBrush);
    p.drawPath(shape);
}

} // namespace nylon
