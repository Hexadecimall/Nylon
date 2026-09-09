#include "nylon.hpp"

#include <cmath>
#include <cstdint>
#include <vector>

namespace {
bool close(float left, float right) { return std::fabs(left - right) < 0.000001F; }
}

int main(int argc, char** argv)
{
    if (argc != 3) return 1;
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
    return 0;
}
