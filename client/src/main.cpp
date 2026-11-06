#include "audio/AudioManager.hpp"
#include "ui/windows/MainWindow.hpp"
#include <QApplication>
#include <QIcon>

int main(int argc, char *argv[])
{
    QApplication app(argc, argv);
    app.setWindowIcon(QIcon(":/feather.svg"));

    static const QAudioDevice inputDevice = QMediaDevices::defaultAudioInput();
    static const QAudioDevice outputDevice = QMediaDevices::defaultAudioOutput();

    // Assigns the global declared in AudioManager.hpp; VoiceTab reads it.
    audioManager = new AudioManager(&inputDevice, &outputDevice);
    MainWindow window;
    window.show();

    return app.exec();
}
