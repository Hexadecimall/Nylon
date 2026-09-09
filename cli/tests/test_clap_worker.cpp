#include "nylon.hpp"

#include <cmath>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <thread>
#include <vector>

namespace {
bool close(float left, float right) { return std::fabs(left - right) < 0.000001F; }
}

int main(int argc, char** argv)
{
    if (argc != 4) return 1;
    nylon::AudioEngine audio;
    if (!audio.configurePluginHost(
            argv[1], {std::filesystem::path(argv[2]).parent_path().string()}))
        return 2;
    auto worker = nylon::ClapWorker::open(argv[1], argv[2],
        "app.nylon.fixture", 48'000.0, 64);
    if (!worker) return 2;
    if (worker.inputAudioPorts() != 1 || worker.inputNotePorts() != 1) return 3;
    std::uint32_t latency = 0;
    if (!worker.latency(latency) || latency != 32) return 4;
    const auto parameters = worker.parameters();
    if (parameters.size() != 1 || parameters[0].identifier != 7
        || parameters[0].name != "Gain" || parameters[0].module != "Output")
        return 5;

    const float inputLeft[] = {1.0F, 1.0F, 1.0F};
    const float inputRight[] = {-1.0F, -1.0F, -1.0F};
    float outputLeft[3]{};
    float outputRight[3]{};
    const nylon::ClapWorker::ParameterEvent parameter{1, 7, 0.25};
    const nylon::ClapWorker::NoteEvent note{2, 0, 41, 0, 2, 60, 0.6};
    if (!worker.processStereo(inputLeft, inputRight, outputLeft, outputRight, 3,
            &parameter, 1, &note, 1))
        return 6;
    if (!close(outputLeft[0], 0.5F) || !close(outputLeft[1], 0.25F)
        || !close(outputLeft[2], 0.6F) || !close(outputRight[2], -0.6F))
        return 7;

    std::vector<std::uint8_t> state;
    if (!worker.saveState(state) || state.empty()) return 8;
    const nylon::ClapWorker::ParameterEvent change{0, 7, 0.75};
    if (!worker.processStereo(inputLeft, inputRight, outputLeft, outputRight, 1,
            &change, 1, nullptr, 0))
        return 9;
    if (!worker.loadState(state)) return 10;
    if (!worker.processStereo(inputLeft, inputRight, outputLeft, outputRight, 1,
            nullptr, 0, nullptr, 0)
        || !close(outputLeft[0], 0.6F))
        return 11;

    const nylon::ClapWorker::ParameterEvent unordered[] = {
        {2, 7, 0.5}, {1, 7, 0.5}};
    if (worker.processStereo(inputLeft, inputRight, outputLeft, outputRight, 3,
            unordered, 2, nullptr, 0))
        return 12;
    if (!worker.processStereo(inputLeft, inputRight, outputLeft, outputRight, 1,
            nullptr, 0, nullptr, 0))
        return 13;

    auto bridge = nylon::ClapBridge::open(argv[1], argv[2],
        "app.nylon.fixture", 48'000.0, 64, 3);
    if (!bridge || !bridge.isRunning() || bridge.latency() != 96) return 14;
    std::vector<float> bridgeInputLeft(64, 0.5F);
    std::vector<float> bridgeInputRight(64, -0.25F);
    std::vector<float> bridgeOutputLeft(64, 1.0F);
    std::vector<float> bridgeOutputRight(64, 1.0F);
    if (!bridge.processStereo(bridgeInputLeft.data(), bridgeInputRight.data(),
            bridgeOutputLeft.data(), bridgeOutputRight.data(), 64,
            nullptr, 0, nullptr, 0))
        return 15;
    if (!close(bridgeOutputLeft[0], 0.0F)) return 16;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(1);
    while (bridge.completedBlocks() < 1 && std::chrono::steady_clock::now() < deadline)
        std::this_thread::yield();
    if (bridge.completedBlocks() != 1) return 17;
    if (!bridge.processStereo(bridgeInputLeft.data(), bridgeInputRight.data(),
            bridgeOutputLeft.data(), bridgeOutputRight.data(), 64,
            nullptr, 0, nullptr, 0))
        return 18;
    if (!close(bridgeOutputLeft[0], 0.25F)
        || !close(bridgeOutputRight[0], -0.125F))
        return 19;
    if (bridge.submittedBlocks() != 2 || bridge.underruns() != 0
        || bridge.queueDrops() != 0 || bridge.workerFailures() != 0)
        return 20;
    auto hanging = nylon::ClapWorker::open(argv[1], argv[3],
        "app.nylon.fixture", 48'000.0, 64);
    if (!hanging) return 21;
    const auto hangStart = std::chrono::steady_clock::now();
    if (hanging.processStereo(inputLeft, inputRight, outputLeft, outputRight, 1,
            nullptr, 0, nullptr, 0))
        return 22;
    if (std::chrono::steady_clock::now() - hangStart > std::chrono::seconds(3))
        return 23;
    const auto retryStart = std::chrono::steady_clock::now();
    if (hanging.processStereo(inputLeft, inputRight, outputLeft, outputRight, 1,
            nullptr, 0, nullptr, 0))
        return 24;
    if (std::chrono::steady_clock::now() - retryStart > std::chrono::milliseconds(50))
        return 25;
    return 0;
}
