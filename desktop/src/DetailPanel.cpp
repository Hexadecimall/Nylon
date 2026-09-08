#include "DetailPanel.h"

#include "Theme.h"
#include "widgets/FlatButton.h"

#include <QHBoxLayout>
#include <QLabel>
#include <QStackedWidget>
#include <QVBoxLayout>

namespace nylon {

DetailPanel::DetailPanel(const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
    , m_title(new QLabel(this))
    , m_clipTab(new FlatButton(theme, this))
    , m_deviceTab(new FlatButton(theme, this))
    , m_stack(new QStackedWidget(this))
    , m_clipEmpty(new QLabel(this))
    , m_deviceEmpty(new QLabel(this))
{
    setObjectName(QStringLiteral("detail"));
    setAutoFillBackground(true);

    m_title->setObjectName(QStringLiteral("detailTitle"));
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
    m_stack->addWidget(m_clipEmpty);
    m_stack->addWidget(m_deviceEmpty);

    auto* header = new QHBoxLayout;
    header->setContentsMargins(0, 0, 0, 0);
    header->setSpacing(0);
    header->addWidget(m_clipTab);
    header->addWidget(m_deviceTab);
    header->addSpacing(8);
    header->addWidget(m_title, 1);

    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);
    layout->addLayout(header);
    layout->addWidget(m_stack, 1);

    connect(m_clipTab, &FlatButton::clicked, this, &DetailPanel::showClipPage);
    connect(m_deviceTab, &FlatButton::clicked, this, &DetailPanel::showDevicePage);

    setTheme(theme);
    showPage(Page::Device);
}

void DetailPanel::setTheme(const Theme* theme)
{
    m_theme = theme;
    m_clipTab->setTheme(theme);
    m_deviceTab->setTheme(theme);
    QPalette pal = palette();
    pal.setColor(QPalette::Window, m_theme->color(QStringLiteral("detail.background")));
    setPalette(pal);
    const int pad = m_theme->metricInt(QStringLiteral("control.padding"), 4);
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
        m_clipEmpty->setText(tr("Select a clip slot to edit its clip."));
        m_deviceEmpty->setText(tr("Select a track to see its devices."));
        return;
    }
    m_title->setText(m_trackName);
    m_clipEmpty->setText(tr("%1 has no clip in the selected slot.").arg(m_trackName));
    m_deviceEmpty->setText(tr("%1 has no devices.\nDrop an instrument or effect here from the browser.").arg(m_trackName));
}

} // namespace nylon
