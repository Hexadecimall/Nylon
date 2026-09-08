#include "CommandPalette.h"

#include "PanelPaint.h"
#include "Theme.h"

#include <QAction>
#include <QKeyEvent>
#include <QLineEdit>
#include <QListWidget>
#include <QPainter>
#include <QVBoxLayout>

#include <algorithm>

namespace nylon {

namespace {

QString cleanText(const QAction* a)
{
    QString t = a->text();
    t.remove(QLatin1Char('&'));
    if (t.endsWith(QLatin1String("..."))) {
        t.chop(3);
    }
    return t;
}

} // namespace

CommandPalette::CommandPalette(const QList<QAction*>& actions, const Theme* theme, QWidget* parent)
    : QDialog(parent, Qt::Popup | Qt::FramelessWindowHint)
    , m_theme(theme)
    , m_search(new QLineEdit(this))
    , m_list(new QListWidget(this))
{
    setObjectName(QStringLiteral("commandPalette"));
    setAttribute(Qt::WA_TranslucentBackground);
    setModal(true);
    for (QAction* a : actions) {
        if (!a->objectName().isEmpty() && !a->isSeparator() && !a->text().isEmpty()) {
            m_actions.append(a);
        }
    }
    std::sort(m_actions.begin(), m_actions.end(), [](QAction* x, QAction* y) {
        return cleanText(x).localeAwareCompare(cleanText(y)) < 0;
    });

    m_search->setPlaceholderText(tr("Type a command"));
    m_search->setClearButtonEnabled(true);
    m_list->setFrameShape(QFrame::NoFrame);
    m_list->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_list->setFocusPolicy(Qt::NoFocus);

    auto* layout = new QVBoxLayout(this);
    const int pad = theme->metricInt(QStringLiteral("panel.padding"), 8);
    layout->setContentsMargins(pad, pad, pad, pad);
    layout->setSpacing(6);
    layout->addWidget(m_search);
    layout->addWidget(m_list, 1);
    resize(520, 360);

    connect(m_search, &QLineEdit::textChanged, this, [this] { rebuild(); });
    connect(m_list, &QListWidget::itemActivated, this, [this] { activateCurrent(); });
    connect(m_list, &QListWidget::itemClicked, this, [this] { activateCurrent(); });
    rebuild();
    m_search->setFocus();
}

bool CommandPalette::fuzzyMatch(const QString& pattern, const QString& text)
{
    qsizetype i = 0;
    for (const QChar c : text) {
        if (i < pattern.size() && c.toLower() == pattern.at(i).toLower()) {
            ++i;
        }
    }
    return i == pattern.size();
}

int CommandPalette::matchScore(const QString& pattern, const QString& text)
{
    if (pattern.isEmpty()) {
        return 0;
    }
    const QString lowerText = text.toLower();
    const QString lowerPattern = pattern.toLower();
    if (lowerText.startsWith(lowerPattern)) {
        return 0;
    }
    const qsizetype at = lowerText.indexOf(lowerPattern);
    if (at >= 0) {
        return 1 + static_cast<int>(at);
    }
    return 1000;
}

void CommandPalette::setFilter(const QString& text)
{
    m_search->setText(text);
}

void CommandPalette::rebuild()
{
    const QString pattern = m_search->text().trimmed();
    QList<QAction*> matches;
    for (QAction* a : m_actions) {
        if (pattern.isEmpty() || fuzzyMatch(pattern, cleanText(a))) {
            matches.append(a);
        }
    }
    std::stable_sort(matches.begin(), matches.end(), [&pattern](QAction* x, QAction* y) {
        return matchScore(pattern, cleanText(x)) < matchScore(pattern, cleanText(y));
    });
    m_list->clear();
    for (QAction* a : matches) {
        const QString shortcut = a->shortcut().toString(QKeySequence::NativeText);
        auto* item = new QListWidgetItem(shortcut.isEmpty() ? cleanText(a)
                                                            : QStringLiteral("%1\t%2").arg(cleanText(a), shortcut), m_list);
        item->setData(Qt::UserRole, QVariant::fromValue<void*>(a));
        if (!a->isEnabled()) {
            item->setForeground(m_theme->color(QStringLiteral("text.disabled")));
        }
        if (a->isCheckable()) {
            item->setText(item->text() + (a->isChecked() ? tr("  (on)") : tr("  (off)")));
        }
    }
    if (m_list->count() > 0) {
        m_list->setCurrentRow(0);
    }
}

QList<QAction*> CommandPalette::visibleActions() const
{
    QList<QAction*> out;
    for (int i = 0; i < m_list->count(); ++i) {
        out.append(static_cast<QAction*>(m_list->item(i)->data(Qt::UserRole).value<void*>()));
    }
    return out;
}

QAction* CommandPalette::currentAction() const
{
    QListWidgetItem* item = m_list->currentItem();
    return item ? static_cast<QAction*>(item->data(Qt::UserRole).value<void*>()) : nullptr;
}

void CommandPalette::activateCurrent()
{
    QAction* a = currentAction();
    if (!a || !a->isEnabled()) {
        return;
    }
    accept();
    emit triggered(a);
    a->trigger();
}

void CommandPalette::keyPressEvent(QKeyEvent* event)
{
    switch (event->key()) {
    case Qt::Key_Return:
    case Qt::Key_Enter:
        activateCurrent();
        return;
    case Qt::Key_Down:
        m_list->setCurrentRow(qMin(m_list->count() - 1, m_list->currentRow() + 1));
        return;
    case Qt::Key_Up:
        m_list->setCurrentRow(qMax(0, m_list->currentRow() - 1));
        return;
    case Qt::Key_Escape:
        reject();
        return;
    default:
        QDialog::keyPressEvent(event);
    }
}

void CommandPalette::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    paint::panel(p, *m_theme, rect(), m_theme->color(QStringLiteral("raised")));
}

} // namespace nylon
