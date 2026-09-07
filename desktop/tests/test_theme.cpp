#include "Theme.h"
#include "ThemeManager.h"

#include <QtTest>

using nylon::Theme;
using nylon::ThemeManager;

class TestTheme : public QObject {
    Q_OBJECT
private slots:
    void parsesColorsMetricsAndFonts();
    void parsesEightDigitColorsAsRgba();
    void reportsErrorsWithLineNumbersAndKeepsGoing();
    void reportsDuplicates();
    void missingKeysListsRequiredTokens();
    void trackColorsCycle();
    void builtinThemesAreComplete();
    void builtinThemesShareTheSameKeySet();
    void styleSheetUsesTokens();
    void managerLoadsBuiltinAndRejectsUnknown();
    void managerKeepsCurrentThemeOnFailure();
};

void TestTheme::parsesColorsMetricsAndFonts()
{
    QStringList errors;
    const Theme t = Theme::parse(QStringLiteral(
        "# comment\n"
        "name = Example\n"
        "\n"
        "color.background = #0F1113\n"
        "metric.separator = 1\n"
        "metric.scale = 1.5\n"
        "font.family = Inter\n"), &errors);
    QVERIFY2(errors.isEmpty(), qPrintable(errors.join(QStringLiteral("; "))));
    QCOMPARE(t.name(), QStringLiteral("Example"));
    QCOMPARE(t.color(QStringLiteral("background")), QColor(0x0F, 0x11, 0x13));
    QCOMPARE(t.metricInt(QStringLiteral("separator")), 1);
    QCOMPARE(t.metric(QStringLiteral("scale")), 1.5);
    QCOMPARE(t.font(QStringLiteral("family")), QStringLiteral("Inter"));
    QVERIFY(!t.hasColor(QStringLiteral("nope")));
    QVERIFY(!t.color(QStringLiteral("nope")).isValid());
    QCOMPARE(t.metric(QStringLiteral("nope"), 7.0), 7.0);
}

void TestTheme::parsesEightDigitColorsAsRgba()
{
    const Theme t = Theme::parse(QStringLiteral("color.selection = #5B9CF640\n"));
    const QColor c = t.color(QStringLiteral("selection"));
    QCOMPARE(c.red(), 0x5B);
    QCOMPARE(c.green(), 0x9C);
    QCOMPARE(c.blue(), 0xF6);
    QCOMPARE(c.alpha(), 0x40);
}

void TestTheme::reportsErrorsWithLineNumbersAndKeepsGoing()
{
    QStringList errors;
    const Theme t = Theme::parse(QStringLiteral(
        "color.a = #112233\n"
        "this line has no equals\n"
        "color.b = notacolor\n"
        "metric.c = abc\n"
        "unknown.d = 1\n"
        "color. = #000000\n"
        "= value\n"
        "color.e =\n"
        "color.f = #445566\n"), &errors);
    QCOMPARE(errors.size(), 7);
    QVERIFY(errors.at(0).startsWith(QStringLiteral("line 2:")));
    QVERIFY(errors.at(1).startsWith(QStringLiteral("line 3:")));
    QVERIFY(errors.at(2).startsWith(QStringLiteral("line 4:")));
    QVERIFY(errors.at(3).startsWith(QStringLiteral("line 5:")));
    QVERIFY(errors.at(4).startsWith(QStringLiteral("line 6:")));
    QVERIFY(errors.at(5).startsWith(QStringLiteral("line 7:")));
    QVERIFY(errors.at(6).startsWith(QStringLiteral("line 8:")));
    QVERIFY(t.hasColor(QStringLiteral("a")));
    QVERIFY(t.hasColor(QStringLiteral("f")));
    QVERIFY(!t.hasColor(QStringLiteral("b")));
    QVERIFY(!t.hasMetric(QStringLiteral("c")));
}

void TestTheme::reportsDuplicates()
{
    QStringList errors;
    const Theme t = Theme::parse(QStringLiteral(
        "name = A\nname = B\ncolor.x = #000000\ncolor.x = #FFFFFF\n"), &errors);
    QCOMPARE(errors.size(), 2);
    // The later value wins so an edited entry at the bottom takes effect.
    QCOMPARE(t.name(), QStringLiteral("B"));
    QCOMPARE(t.color(QStringLiteral("x")), QColor(Qt::white));
}

void TestTheme::missingKeysListsRequiredTokens()
{
    const Theme empty = Theme::parse(QString());
    const QStringList missing = empty.missingKeys();
    QCOMPARE(missing.size(), Theme::requiredColors().size() + Theme::requiredMetrics().size());
    QVERIFY(missing.contains(QStringLiteral("color.background")));
    QVERIFY(missing.contains(QStringLiteral("metric.separator")));
    QVERIFY(missing.contains(QStringLiteral("color.track.16")));
}

void TestTheme::trackColorsCycle()
{
    const Theme t = Theme::parse(QStringLiteral(
        "color.track.1 = #FF0000\ncolor.track.2 = #00FF00\ncolor.track.3 = #0000FF\n"));
    QCOMPARE(t.trackColorCount(), 3);
    QCOMPARE(t.trackColor(0), QColor(Qt::red));
    QCOMPARE(t.trackColor(3), QColor(Qt::red));
    QCOMPARE(t.trackColor(4), QColor(Qt::green));
    QVERIFY(!t.trackColor(-1).isValid());
    QVERIFY(!Theme::parse(QString()).trackColor(0).isValid());
}

void TestTheme::builtinThemesAreComplete()
{
    const QStringList names = ThemeManager::builtinNames();
    QCOMPARE(names, (QStringList{QStringLiteral("graphite"), QStringLiteral("nylon"),
        QStringLiteral("paper"), QStringLiteral("slate")}));
    for (const QString& name : names) {
        QStringList errors;
        const Theme t = Theme::fromFile(QStringLiteral(":/themes/%1.theme").arg(name), &errors);
        QVERIFY2(errors.isEmpty(), qPrintable(name + QStringLiteral(": ") + errors.join(QStringLiteral("; "))));
        QVERIFY2(t.missingKeys().isEmpty(), qPrintable(name + QStringLiteral(": ") + t.missingKeys().join(QStringLiteral(", "))));
        QCOMPARE(t.name().toLower(), name);
        QCOMPARE(t.trackColorCount(), 16);
        QVERIFY(t.metricInt(QStringLiteral("separator")) >= 1);
    }
}

void TestTheme::builtinThemesShareTheSameKeySet()
{
    // Every theme must define the same tokens so switching never leaves a
    // widget without a value.
    const Theme reference = Theme::fromFile(QStringLiteral(":/themes/nylon.theme"));
    QStringList refColors = reference.colorKeys();
    QStringList refMetrics = reference.metricKeys();
    refColors.sort();
    refMetrics.sort();
    for (const QString& name : ThemeManager::builtinNames()) {
        const Theme t = Theme::fromFile(QStringLiteral(":/themes/%1.theme").arg(name));
        QStringList colors = t.colorKeys();
        QStringList metrics = t.metricKeys();
        colors.sort();
        metrics.sort();
        QCOMPARE(colors, refColors);
        QCOMPARE(metrics, refMetrics);
    }
}

void TestTheme::styleSheetUsesTokens()
{
    const Theme t = Theme::fromFile(QStringLiteral(":/themes/nylon.theme"));
    const QString css = t.styleSheet();
    QVERIFY(css.contains(t.color(QStringLiteral("background")).name(QColor::HexArgb)));
    QVERIFY(css.contains(t.color(QStringLiteral("accent")).name(QColor::HexArgb)));
    QVERIFY(css.contains(QStringLiteral("border-radius: 0")));
    QVERIFY(!css.contains(QLatin1Char('%')));
}

void TestTheme::managerLoadsBuiltinAndRejectsUnknown()
{
    ThemeManager m;
    QSignalSpy changed(&m, &ThemeManager::themeChanged);
    QSignalSpy failed(&m, &ThemeManager::loadFailed);
    QVERIFY(m.load(QStringLiteral("Slate")));
    QCOMPARE(m.currentName(), QStringLiteral("slate"));
    QCOMPARE(m.theme().name(), QStringLiteral("Slate"));
    QCOMPARE(changed.count(), 1);
    QVERIFY(!m.load(QStringLiteral("does-not-exist")));
    QCOMPARE(failed.count(), 1);
    QCOMPARE(m.currentName(), QStringLiteral("slate"));
    QVERIFY(m.reload());
    QCOMPARE(changed.count(), 2);
}

void TestTheme::managerKeepsCurrentThemeOnFailure()
{
    ThemeManager m;
    QVERIFY(!m.reload());
    QVERIFY(m.load(QStringLiteral("paper")));
    const QColor before = m.theme().color(QStringLiteral("background"));
    QVERIFY(!m.load(QString()));
    QCOMPARE(m.theme().color(QStringLiteral("background")), before);
    QVERIFY(!m.lastErrors().isEmpty());
}

QTEST_MAIN(TestTheme)
#include "test_theme.moc"
