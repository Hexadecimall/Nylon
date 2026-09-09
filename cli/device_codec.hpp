#pragma once

#include "nylon.hpp"

#include <QJsonObject>
#include <QStringList>

bool parseTrackDevice(
    const QStringList& positional, int kindIndex, nylon::TrackDevice& device);
QJsonObject deviceJson(const nylon::TrackDevice& device, std::uint64_t index);
QJsonObject pluginDeviceJson(const nylon::TrackPluginDevice& device, std::uint64_t index);
bool parsePluginFormat(const QString& text, nylon::PluginFormat& format);
