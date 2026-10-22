#include "SettingWindow.hpp"
#include "../widgets/settings/SettingsTabs.hpp"

SettingWindow::SettingWindow()
    : QMainWindow(nullptr, Qt::Window)
{
    setWindowTitle("Settings");
    resize(480, 360);

    setCentralWidget(new SettingsTabs(this));
}
