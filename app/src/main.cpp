// Program entry point: starts Qt, picks the look of the controls and loads
// the user interface from the QML module "ArtApp" (see app/CMakeLists.txt).

#include <artengine/version.h>

#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQuickStyle>
#include <QString>

#include <cstdlib>

int main(int argc, char *argv[])
{
    QGuiApplication app(argc, argv);

    const std::string_view version = art::engineVersion();
    QGuiApplication::setApplicationName(QStringLiteral("Art App"));
    QGuiApplication::setApplicationVersion(
        QString::fromUtf8(version.data(), qsizetype(version.size())));

    // "Fusion" looks the same on every system and is easy to give a dark
    // theme, which suits a painting app. The colours are set in Main.qml.
    QQuickStyle::setStyle(QStringLiteral("Fusion"));

    QQmlApplicationEngine engine;

    // If the QML fails to load (for example a typo in a .qml file), quit
    // with an error instead of running with no window.
    QObject::connect(
        &engine, &QQmlApplicationEngine::objectCreationFailed, &app,
        [] { QCoreApplication::exit(EXIT_FAILURE); }, Qt::QueuedConnection);

    engine.loadFromModule("ArtApp", "Main");

    return app.exec();
}
