#include "Theme.h"
#include "ThemeManager.h"

#include <QFontDatabase>
#include <QGuiApplication>
#include <QtTest>

using nylon::Theme;
using nylon::ThemeManager;

class TestTheme : public QObject {
    Q_OBJECT
private slots:
    void parsesColorsMetricsAndFonts();
    void parsesEightDigitColorsAsRgba();
    void reportsErrorsWithLineNumbersAndKeepsGoing();
    void rejectsMalformedColors();
    void rejectsNonFiniteAndOutOfRangeMetrics();
    void reportsDuplicates();
    void missingKeysListsRequiredTokens();
    void trackColorsCycle();
    void builtinThemesAreComplete();
    void builtinThemesShareTheSameKeySet();
    void styleSheetUsesTokens();
    void resolvedFontFollowsTokens();
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

void TestTheme::rejectsMalformedColors()
{
    const QStringList bad{
        QStringLiteral("#+12345"),
        QStringLiteral("#-12345"),
        QStringLiteral("# 12345"),
        QStringLiteral("#12345 "),
        QStringLiteral("#12345G"),
        QStringLiteral("#1234567"),
        QStringLiteral("#12345"),
        QStringLiteral("#1234567890"),
        QStringLiteral("112233"),
        QStringLiteral("#0x1122"),
        QStringLiteral("#11 22 33"),
        QStringLiteral("rgb(1,2,3)"),
        QStringLiteral("red"),
    };
    for (const QString& value : bad) {
        QStringList errors;
        const Theme t = Theme::parse(QStringLiteral("color.x = %1\n").arg(value), &errors);
        QVERIFY2(!t.hasColor(QStringLiteral("x")), qPrintable(value));
        QCOMPARE(errors.size(), 1);
        QVERIFY(errors.first().contains(QStringLiteral("invalid color")));
    }
    const Theme ok = Theme::parse(QStringLiteral("color.a = #aAbBcC\ncolor.b = #00000000\n"));
    QCOMPARE(ok.color(QStringLiteral("a")), QColor(0xAA, 0xBB, 0xCC));
    QCOMPARE(ok.color(QStringLiteral("b")).alpha(), 0);
}

void TestTheme::rejectsNonFiniteAndOutOfRangeMetrics()
{
    const QStringList bad{
        QStringLiteral("nan"),
        QStringLiteral("NaN"),
        QStringLiteral("inf"),
        QStringLiteral("-inf"),
        QStringLiteral("1e400"),
        QStringLiteral("-1"),
        QStringLiteral("-0.5"),
        QStringLiteral("100001"),
        QStringLiteral("1e12"),
        QStringLiteral("2147483648"),
        QStringLiteral("12px"),
        QStringLiteral("0x10"),
    };
    for (const QString& value : bad) {
        QStringList errors;
        const Theme t = Theme::parse(QStringLiteral("metric.x = %1\n").arg(value), &errors);
        QVERIFY2(!t.hasMetric(QStringLiteral("x")), qPrintable(value));
        QCOMPARE(errors.size(), 1);
        QCOMPARE(t.metricInt(QStringLiteral("x"), 3), 3);
    }
    const Theme ok = Theme::parse(QStringLiteral("metric.a = 0\nmetric.b = 100000\nmetric.c = 1.5\n"));
    QCOMPARE(ok.metricInt(QStringLiteral("a")), 0);
    QCOMPARE(ok.metricInt(QStringLiteral("b")), 100000);
    QCOMPARE(ok.metricInt(QStringLiteral("c")), 2);
    QCOMPARE(ok.metric(QStringLiteral("c")), 1.5);
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
    QVERIFY(css.contains(QStringLiteral("border-radius: %1px").arg(t.metricInt(QStringLiteral("radius.small")))));
    QVERIFY(css.contains(QStringLiteral("border-radius: %1px").arg(t.metricInt(QStringLiteral("radius")))));
    QVERIFY(!css.contains(QStringLiteral("border-radius: 0")));
    QVERIFY(!css.contains(QLatin1Char('%')));

    // Metrics that reach the style sheet are clamped to renderable sizes.
    const Theme extreme = Theme::parse(QStringLiteral(
        "metric.separator = 100000\nmetric.control.padding = 100000\nmetric.control.height = 100000\n"));
    const QString big = extreme.styleSheet();
    QVERIFY(!big.contains(QStringLiteral("100000")));
    QVERIFY(big.contains(QStringLiteral("border: 16px")));
    QVERIFY(big.contains(QStringLiteral("max-height: 256px")));
    const Theme tiny = Theme::parse(QStringLiteral("metric.control.height = 0\n"));
    QVERIFY(tiny.styleSheet().contains(QStringLiteral("min-height: 8px")));
}

void TestTheme::resolvedFontFollowsTokens()
{
    const Theme system = Theme::parse(QStringLiteral("font.family = system\nmetric.font.size = 13\n"));
    const QFont f = system.resolvedFont();
    QCOMPARE(f.pixelSize(), 13);
    QVERIFY(f.family() != QLatin1String("system"));
    // Whatever was chosen must be installed, so the matcher never falls
    // back through alias resolution.
    QVERIFY2(QFontDatabase::hasFamily(f.family()), qPrintable(f.family()));
    const QString sys = QFontDatabase::systemFont(QFontDatabase::GeneralFont).family();
    if (QFontDatabase::hasFamily(sys)) {
        QCOMPARE(f.family(), sys);
    }

    const QStringList installed = QFontDatabase::families();
    QVERIFY(!installed.isEmpty());
    const Theme named = Theme::parse(
        QStringLiteral("font.family = %1\nmetric.font.size = 9\n").arg(installed.last()));
    QCOMPARE(named.resolvedFont().family(), installed.last());
    QCOMPARE(named.resolvedFont().pixelSize(), 9);

    for (const QString& family : installed) {
        const Theme advertised = Theme::parse(
            QStringLiteral("font.family = %1\n").arg(family));
        QCOMPARE(advertised.resolvedFontFamily(), family);
    }

    const Theme missing = Theme::parse(QStringLiteral("font.family = No Such Family 0xDEAD\n"));
    QVERIFY(QFontDatabase::hasFamily(missing.resolvedFontFamily()));
    QVERIFY(missing.resolvedFontFamily() != QLatin1String("No Such Family 0xDEAD"));

    // No tokens at all still yields a usable font.
    const QFont fallback = Theme::parse(QString()).resolvedFont();
    QVERIFY(fallback.pixelSize() > 0);
}

void TestTheme::managerLoadsBuiltinAndRejectsUnknown()
{
    ThemeManager m;
    QSignalSpy changed(&m, &ThemeManager::themeChanged);
    QSignalSpy failed(&m, &ThemeManager::loadFailed);
    QVERIFY(m.load(QStringLiteral("Slate")));
    QCOMPARE(m.currentName(), QStringLiteral("slate"));
    QCOMPARE(QGuiApplication::font().pixelSize(), m.theme().metricInt(QStringLiteral("font.size")));
    QCOMPARE(QGuiApplication::font().family(), m.theme().resolvedFontFamily());
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
