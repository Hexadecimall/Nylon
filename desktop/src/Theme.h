#pragma once

#include <QColor>
#include <QHash>
#include <QString>
#include <QStringList>

namespace nylon {

// A parsed theme file. Every color and metric the widgets draw with comes
// from here; widget code holds no literal colors.
//
// File format, one entry per line:
//   name = <text>
//   font.<key> = <text>
//   color.<key> = #RRGGBB | #RRGGBBAA
//   metric.<key> = <number>
// Blank lines and lines starting with '#' are ignored.
class Theme {
public:
    // Parses `text`. Problems are appended to `errors` as "line N: message".
    // Entries before and after a bad line are still loaded.
    static Theme parse(const QString& text, QStringList* errors = nullptr);

    // Reads and parses a file; a missing or unreadable file yields a theme
    // with no entries and one error.
    static Theme fromFile(const QString& path, QStringList* errors = nullptr);

    // Keys every widget expects. A theme missing any of these is rejected
    // by ThemeManager.
    static const QStringList& requiredColors();
    static const QStringList& requiredMetrics();

    bool isEmpty() const { return m_colors.isEmpty() && m_metrics.isEmpty(); }
    QString name() const { return m_name; }

    bool hasColor(const QString& key) const { return m_colors.contains(key); }
    bool hasMetric(const QString& key) const { return m_metrics.contains(key); }

    // Returns an invalid QColor for an unknown key.
    QColor color(const QString& key) const { return m_colors.value(key); }
    // Returns `fallback` for an unknown key.
    double metric(const QString& key, double fallback = 0.0) const;
    int metricInt(const QString& key, int fallback = 0) const;
    QString font(const QString& key) const { return m_fonts.value(key); }

    // Track color for a zero-based index, cycling through the palette.
    QColor trackColor(int index) const;
    int trackColorCount() const;

    // Keys from requiredColors/requiredMetrics that this theme lacks.
    QStringList missingKeys() const;

    // Qt stylesheet for standard widgets, built from the tokens.
    QString styleSheet() const;

    QStringList colorKeys() const { return m_colors.keys(); }
    QStringList metricKeys() const { return m_metrics.keys(); }

private:
    static bool parseColor(const QString& text, QColor* out);

    QString m_name;
    QHash<QString, QColor> m_colors;
    QHash<QString, double> m_metrics;
    QHash<QString, QString> m_fonts;
};

} // namespace nylon
