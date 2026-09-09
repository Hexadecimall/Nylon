#include "TransportBar.h"

#include "ProjectBridge.h"
#include "Theme.h"
#include "widgets/FlatButton.h"
#include "widgets/LcdDisplay.h"
#include "widgets/ValueBox.h"

#include "PanelPaint.h"

#include <QHBoxLayout>
#include <QLabel>
#include <QPaintEvent>
#include <QPainter>

#include <cmath>

namespace nylon {

TransportBar::TransportBar(ProjectBridge* bridge, const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_bridge(bridge)
    , m_theme(theme)
    , m_tempo(new ValueBox(theme, this))
    , m_tap(new FlatButton(theme, this))
    , m_numerator(new ValueBox(theme, this))
    , m_denominator(new ValueBox(theme, this))
    , m_metronome(new FlatButton(theme, this))
    , m_lcd(new LcdDisplay(theme, this))
    , m_play(new FlatButton(theme, this))
    , m_stop(new FlatButton(theme, this))
    , m_record(new FlatButton(theme, this))
    , m_loop(new FlatButton(theme, this))
    , m_trackCount(new QLabel(this))
    , m_session(new FlatButton(theme, this))
    , m_arrangement(new FlatButton(theme, this))
{
    setObjectName(QStringLiteral("transport"));
    setAutoFillBackground(false);

    m_tempo->setObjectName(QStringLiteral("tempo"));
    m_tempo->setRange(20.0, 999.0);
    m_tempo->setDecimals(2);
    m_tempo->setDefaultValue(120.0);
    m_tempo->setStatusTip(tr("Tempo in beats per minute. Drag, or double-click to type."));

    m_tap->setText(tr("Tap"));
    m_tap->setStatusTip(tr("Tap tempo. Needs a running transport."));
    m_tap->setToolTip(tr("Tap tempo is not available until playback exists."));

    m_numerator->setObjectName(QStringLiteral("signatureNumerator"));
    m_numerator->setRange(1.0, 99.0);
    m_numerator->setDecimals(0);
    m_numerator->setDragPixels(600);
    m_numerator->setStatusTip(tr("Time signature beats per bar."));
    m_denominator->setObjectName(QStringLiteral("signatureDenominator"));
    m_denominator->setRange(1.0, 64.0);
    m_denominator->setDecimals(0);
    m_denominator->setDragPixels(600);
    m_denominator->setStatusTip(tr("Time signature beat unit."));

    m_metronome->setGlyph(FlatButton::Glyph::Metronome);
    m_metronome->setCheckable(true);
    m_metronome->setActiveColorKey(QStringLiteral("state.on"));
    m_metronome->setStatusTip(tr("Metronome."));

    m_lcd->setStatusTip(tr("Position, tempo, time signature, and key."));

    m_play->setGlyph(FlatButton::Glyph::Play);
    m_play->setCheckable(true);
    m_play->setActiveColorKey(QStringLiteral("state.play"));
    m_play->setStatusTip(tr("Play."));
    m_stop->setGlyph(FlatButton::Glyph::Stop);
    m_stop->setStatusTip(tr("Stop."));
    m_record->setGlyph(FlatButton::Glyph::Record);
    m_record->setCheckable(true);
    m_record->setActiveColorKey(QStringLiteral("state.record"));
    m_record->setStatusTip(tr("Arrangement record."));
    m_loop->setGlyph(FlatButton::Glyph::Loop);
    m_loop->setCheckable(true);
    m_loop->setActiveColorKey(QStringLiteral("state.loop"));
    m_loop->setStatusTip(tr("Loop the arrangement loop brace."));

    m_trackCount->setObjectName(QStringLiteral("secondary"));

    m_session->setText(tr("Session"));
    m_session->setCheckable(true);
    m_session->setChecked(true);
    m_session->setStatusTip(tr("Show the Session View (Tab)."));
    m_arrangement->setText(tr("Arrangement"));
    m_arrangement->setCheckable(true);
    m_arrangement->setStatusTip(tr("Show the Arrangement View (Tab)."));

    auto* layout = new QHBoxLayout(this);
    layout->setSpacing(2);
    layout->addWidget(m_tempo);
    layout->addWidget(m_tap);
    layout->addSpacing(6);
    layout->addWidget(m_numerator);
    auto* slash = new QLabel(QStringLiteral("/"), this);
    slash->setObjectName(QStringLiteral("secondary"));
    layout->addWidget(slash);
    layout->addWidget(m_denominator);
    layout->addWidget(m_metronome);
    layout->addStretch(1);
    layout->addWidget(m_lcd);
    layout->addSpacing(6);
    layout->addWidget(m_play);
    layout->addWidget(m_stop);
    layout->addWidget(m_record);
    layout->addSpacing(6);
    layout->addWidget(m_loop);
    layout->addStretch(1);
    layout->addWidget(m_trackCount);
    layout->addSpacing(6);
    layout->addWidget(m_session);
    layout->addWidget(m_arrangement);

    connect(m_tempo, &ValueBox::committed, this, &TransportBar::commitTempo);
    connect(m_numerator, &ValueBox::committed, this, [this](double v) {
        if (!m_bridge->setTimeSignature(qRound(v), m_bridge->timeSignatureDenominator())) {
            emit message(tr("%1 beats per bar is not supported.").arg(qRound(v)));
            refresh();
        }
    });
    connect(m_denominator, &ValueBox::committed, this, [this](double v) {
        if (!m_bridge->setTimeSignature(m_bridge->timeSignatureNumerator(), qRound(v))) {
            emit message(tr("A beat unit of %1 is not supported.").arg(qRound(v)));
            refresh();
        }
    });
    connect(m_session, &FlatButton::clicked, this, [this] {
        showSessionActive(true);
        emit sessionRequested();
    });
    connect(m_arrangement, &FlatButton::clicked, this, [this] {
        showSessionActive(false);
        emit arrangementRequested();
    });
    connect(m_play, &FlatButton::clicked, this, &TransportBar::playRequested);
    connect(m_stop, &FlatButton::clicked, this, &TransportBar::stopRequested);
    connect(m_bridge, &ProjectBridge::changed, this, &TransportBar::refresh);

    setTransportAvailable(false);
    setTheme(theme);
    refresh();
}

void TransportBar::setTheme(const Theme* theme)
{
    m_theme = theme;
    for (FlatButton* b : {m_tap, m_metronome, m_play, m_stop, m_record, m_loop, m_session, m_arrangement}) {
        b->setTheme(theme);
    }
    m_tempo->setTheme(theme);
    m_lcd->setTheme(theme);
    m_numerator->setTheme(theme);
    m_denominator->setTheme(theme);
    const int h = theme->metricInt(QStringLiteral("transport.height"), 28);
    setFixedHeight(h + 8);
    const int pad = theme->metricInt(QStringLiteral("panel.padding"), 8);
    layout()->setContentsMargins(pad, 4, pad, 4);
    layout()->setSpacing(qMax(1, theme->metricInt(QStringLiteral("transport.spacing"), 8) / 4));
    const int side = theme->metricInt(QStringLiteral("transport.button.size"), 22);
    for (FlatButton* b : {m_metronome, m_play, m_stop, m_record, m_loop}) {
        b->setSquare(side + 6);
    }
    m_tempo->setFixedWidth(theme->metricInt(QStringLiteral("transport.tempo.width"), 96));
    m_numerator->setFixedWidth(theme->metricInt(QStringLiteral("control.height"), 20) + 12);
    m_denominator->setFixedWidth(theme->metricInt(QStringLiteral("control.height"), 20) + 12);
    m_lcd->setFixedWidth(theme->metricInt(QStringLiteral("lcd.width"), 300));
    update();
}

void TransportBar::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    paint::panel(p, *m_theme, rect(), m_theme->color(QStringLiteral("panel")));
}

void TransportBar::setTransportAvailable(bool available)
{
    m_transportAvailable = available;
    const QString why = tr("Playback is not available until an audio backend drives the transport.");
    for (FlatButton* b : {m_tap, m_metronome, m_play, m_stop, m_record, m_loop}) {
        b->setEnabled(available);
        b->setToolTip(available ? QString() : why);
    }
    m_lcd->setEnabled(available);
}

void TransportBar::showPosition(double beats)
{
    const int perBar = qBound(1, m_bridge->timeSignatureNumerator(), 64);
    const double safe = std::isfinite(beats) && beats > 0.0 ? beats : 0.0;
    const int whole = static_cast<int>(safe);
    const int bar = whole / perBar + 1;
    const int beat = whole % perBar + 1;
    // Sixteenths inside the beat, counted from one the way a position
    // readout is written.
    const int sixteenth = static_cast<int>((safe - whole) * 4.0) + 1;
    m_lcd->setPosition(QStringLiteral("%1 . %2 . %3").arg(bar).arg(beat).arg(sixteenth));
}

void TransportBar::showSessionActive(bool session)
{
    m_session->setChecked(session);
    m_arrangement->setChecked(!session);
}

void TransportBar::refresh()
{
    const QSignalBlocker block(m_tempo);
    const QSignalBlocker blockNum(m_numerator);
    const QSignalBlocker blockDen(m_denominator);
    m_tempo->setValue(m_bridge->tempo());
    m_numerator->setValue(m_bridge->timeSignatureNumerator());
    m_denominator->setValue(m_bridge->timeSignatureDenominator());
    m_lcd->setTempo(m_bridge->tempo());
    m_lcd->setSignature(m_bridge->timeSignatureNumerator(), m_bridge->timeSignatureDenominator());
    const quint64 n = m_bridge->trackCount();
    m_trackCount->setText(n == 1 ? tr("1 track") : tr("%1 tracks").arg(n));
}

void TransportBar::commitTempo(double requested)
{
    if (qFuzzyCompare(requested, m_bridge->tempo())) {
        return;
    }
    if (!m_bridge->setTempo(requested)) {
        emit message(tr("Tempo %1 is outside the supported range.").arg(requested, 0, 'f', 2));
        refresh();
    }
}

} // namespace nylon
