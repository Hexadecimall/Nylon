#include "PreferencesDialog.h"

#include "BrowserPanel.h"
#include "Shortcuts.h"
#include "ThemeManager.h"

#include <QAction>
#include <QHeaderView>
#include <QKeySequenceEdit>
#include <QTableWidget>

#include <QComboBox>
#include <QDialogButtonBox>
#include <QFileDialog>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QSettings>
#include <QStackedWidget>
#include <QVBoxLayout>

#include <algorithm>

namespace nylon {

PreferencesDialog::PreferencesDialog(ThemeManager* themes, const QList<QAction*>& actions, QWidget* parent)
    : QDialog(parent)
    , m_themes(themes)
    , m_actions(actions)
    , m_sections(new QListWidget(this))
    , m_pages(new QStackedWidget(this))
    , m_theme(new QComboBox(this))
    , m_library(new QLineEdit(this))
    , m_audioStatus(new QLabel(this))
{
    setWindowTitle(tr("Preferences"));
    setModal(true);
    resize(560, 360);

    m_sections->setFrameShape(QFrame::NoFrame);
    m_sections->setFixedWidth(140);
    m_sections->addItems({tr("Look & Feel"), tr("Library"), tr("Shortcuts"), tr("Audio")});

    // Look & Feel
    auto* look = new QWidget(this);
    auto* lookForm = new QFormLayout(look);
    for (const QString& name : ThemeManager::builtinNames()) {
        QString label = name;
        label[0] = label[0].toUpper();
        m_theme->addItem(label, name);
    }
    m_theme->setCurrentIndex(m_theme->findData(m_themes->currentName()));
    lookForm->addRow(tr("Theme"), m_theme);
    auto* userDir = new QLabel(tr("Custom themes: %1").arg(ThemeManager::userThemeDirectory()), look);
    userDir->setObjectName(QStringLiteral("secondary"));
    userDir->setWordWrap(true);
    lookForm->addRow(userDir);

    // Library
    auto* library = new QWidget(this);
    auto* libraryForm = new QFormLayout(library);
    m_library->setText(BrowserPanel::libraryRoot());
    auto* browse = new QPushButton(tr("Choose..."), library);
    auto* row = new QHBoxLayout;
    row->addWidget(m_library, 1);
    row->addWidget(browse);
    libraryForm->addRow(tr("Library folder"), row);
    auto* libraryNote = new QLabel(tr("Category folders are created inside this folder."), library);
    libraryNote->setObjectName(QStringLiteral("secondary"));
    libraryForm->addRow(libraryNote);

    // Audio
    auto* audio = new QWidget(this);
    auto* audioForm = new QFormLayout(audio);
    m_audioStatus->setText(tr("No audio device backend is available in this build.\n"
                              "Device, sample rate, and buffer size settings appear here once one exists."));
    m_audioStatus->setObjectName(QStringLiteral("secondary"));
    m_audioStatus->setWordWrap(true);
    audioForm->addRow(m_audioStatus);

    // Shortcuts
    auto* shortcuts = new QWidget(this);
    auto* shortcutsLayout = new QVBoxLayout(shortcuts);
    m_shortcuts = new QTableWidget(0, 2, shortcuts);
    m_shortcuts->setObjectName(QStringLiteral("shortcutTable"));
    m_shortcuts->setHorizontalHeaderLabels({tr("Command"), tr("Shortcut")});
    m_shortcuts->horizontalHeader()->setStretchLastSection(true);
    m_shortcuts->verticalHeader()->hide();
    m_shortcuts->setSelectionBehavior(QAbstractItemView::SelectRows);
    m_shortcuts->setEditTriggers(QAbstractItemView::NoEditTriggers);
    m_shortcutNote = new QLabel(tr("Click a shortcut to change it. Keys already in use are refused."), shortcuts);
    m_shortcutNote->setObjectName(QStringLiteral("secondary"));
    m_shortcutNote->setWordWrap(true);
    auto* resetShortcuts = new QPushButton(tr("Restore Defaults"), shortcuts);
    resetShortcuts->setObjectName(QStringLiteral("resetShortcuts"));
    shortcutsLayout->addWidget(m_shortcuts, 1);
    shortcutsLayout->addWidget(m_shortcutNote);
    shortcutsLayout->addWidget(resetShortcuts, 0, Qt::AlignLeft);
    connect(resetShortcuts, &QPushButton::clicked, this, [this] {
        Shortcuts::resetAll(m_actions);
        buildShortcutRows();
    });
    buildShortcutRows();

    m_pages->addWidget(look);
    m_pages->addWidget(library);
    m_pages->addWidget(shortcuts);
    m_pages->addWidget(audio);

    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Close, this);

    auto* body = new QHBoxLayout;
    body->setSpacing(8);
    body->addWidget(m_sections);
    body->addWidget(m_pages, 1);
    auto* layout = new QVBoxLayout(this);
    layout->addLayout(body, 1);
    layout->addWidget(buttons);

    connect(m_sections, &QListWidget::currentRowChanged, m_pages, &QStackedWidget::setCurrentIndex);
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    connect(m_theme, &QComboBox::currentIndexChanged, this, [this](int index) {
        const QString name = m_theme->itemData(index).toString();
        if (!name.isEmpty() && name != m_themes->currentName()) {
            m_themes->load(name);
            QSettings().setValue(QStringLiteral("look/theme"), name);
        }
    });
    connect(browse, &QPushButton::clicked, this, &PreferencesDialog::chooseLibraryFolder);
    connect(m_library, &QLineEdit::editingFinished, this, [this] {
        const QString path = m_library->text().trimmed();
        if (!path.isEmpty() && path != BrowserPanel::libraryRoot()) {
            BrowserPanel::setLibraryRoot(path);
            emit libraryRootChanged(path);
        }
    });
    m_sections->setCurrentRow(0);
}

void PreferencesDialog::buildShortcutRows()
{
    m_shortcuts->setRowCount(0);
    std::sort(m_actions.begin(), m_actions.end(), [](QAction* a, QAction* b) {
        return a->text().localeAwareCompare(b->text()) < 0;
    });
    for (QAction* a : m_actions) {
        const int row = m_shortcuts->rowCount();
        m_shortcuts->insertRow(row);
        QString text = a->text();
        text.remove(QLatin1Char('&'));
        auto* name = new QTableWidgetItem(text);
        name->setData(Qt::UserRole, a->objectName());
        m_shortcuts->setItem(row, 0, name);
        auto* edit = new QKeySequenceEdit(a->shortcut(), m_shortcuts);
        edit->setClearButtonEnabled(true);
#if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
        edit->setMaximumSequenceLength(1);
#endif
        m_shortcuts->setCellWidget(row, 1, edit);
        connect(edit, &QKeySequenceEdit::editingFinished, this, [this, row, edit] {
            if (!assignShortcut(row, edit->keySequence())) {
                const QString name = m_shortcuts->item(row, 0)->data(Qt::UserRole).toString();
                for (QAction* a : m_actions) {
                    if (a->objectName() == name) {
                        edit->setKeySequence(a->shortcut());
                    }
                }
            }
        });
    }
    m_shortcuts->resizeColumnToContents(0);
}

bool PreferencesDialog::assignShortcut(int row, const QKeySequence& sequence)
{
    if (row < 0 || row >= m_shortcuts->rowCount()) {
        return false;
    }
    const QString name = m_shortcuts->item(row, 0)->data(Qt::UserRole).toString();
    QAction* target = nullptr;
    for (QAction* a : m_actions) {
        if (a->objectName() == name) {
            target = a;
        }
    }
    if (!target) {
        return false;
    }
    if (QAction* other = Shortcuts::conflict(m_actions, sequence, target)) {
        QString otherText = other->text();
        otherText.remove(QLatin1Char('&'));
        m_shortcutNote->setText(tr("%1 is already used by %2.").arg(sequence.toString(QKeySequence::NativeText), otherText));
        return false;
    }
    Shortcuts::setOverride(target, sequence);
    m_shortcutNote->setText(tr("Click a shortcut to change it. Keys already in use are refused."));
    return true;
}

void PreferencesDialog::chooseLibraryFolder()
{
    const QString dir = QFileDialog::getExistingDirectory(this, tr("Library folder"), m_library->text());
    if (dir.isEmpty()) {
        return;
    }
    m_library->setText(dir);
    BrowserPanel::setLibraryRoot(dir);
    emit libraryRootChanged(dir);
}

} // namespace nylon
