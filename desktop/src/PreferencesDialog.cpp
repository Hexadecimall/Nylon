#include "PreferencesDialog.h"

#include "BrowserPanel.h"
#include "ThemeManager.h"

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

namespace nylon {

PreferencesDialog::PreferencesDialog(ThemeManager* themes, QWidget* parent)
    : QDialog(parent)
    , m_themes(themes)
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
    m_sections->addItems({tr("Look & Feel"), tr("Library"), tr("Audio")});

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

    m_pages->addWidget(look);
    m_pages->addWidget(library);
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
