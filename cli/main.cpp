#include <QCoreApplication>
#include <QCommandLineParser>
#include <QLocalSocket>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonArray>
#include <cstdio>

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);
    app.setApplicationName("nylon");
    QCommandLineParser parser;
    parser.setApplicationDescription("Control a running Nylon window through its local Qt endpoint.");
    parser.addHelpOption();
    const QCommandLineOption endpoint("endpoint", "Endpoint supplied to the GUI with --control.", "name");
    parser.addOption(endpoint);
    parser.addPositionalArgument("command", "info, set-tempo, add-track, undo, redo, view, window");
    parser.addPositionalArgument("args", "Command arguments.", "[args...]");
    parser.process(app);
    const auto positional = parser.positionalArguments();
    if (parser.value(endpoint).isEmpty() || positional.isEmpty()) parser.showHelp(2);
    QLocalSocket socket;
    socket.connectToServer(parser.value(endpoint));
    if (!socket.waitForConnected(2000)) {
        std::fprintf(stderr, "Could not connect to the Nylon endpoint\n");
        return 1;
    }
    QJsonArray args;
    for (int i = 1; i < positional.size(); ++i) args.append(positional[i]);
    const auto request = QJsonDocument(QJsonObject{{"command", positional[0]}, {"args", args}})
        .toJson(QJsonDocument::Compact) + '\n';
    if (request.size() > 8192) return 2;
    socket.write(request);
    if (!socket.waitForBytesWritten(2000)) return 1;
    QByteArray response;
    while (!response.contains('\n')) {
        if (!socket.bytesAvailable() && !socket.waitForReadyRead(2000)) return 1;
        response += socket.readAll();
        if (response.size() > 8192) return 1;
    }
    std::fwrite(response.constData(), 1, static_cast<size_t>(response.size()), stdout);
    return QJsonDocument::fromJson(response).object().value("ok").toBool() ? 0 : 1;
}
