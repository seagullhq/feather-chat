#include "SettingWindow.hpp"
#include "../widgets/settingsWidgets/SettingsRail.hpp"
#include <QLabel>
#include <QVBoxLayout>
#include <QWidget>
#include <qboxlayout.h>
#include <qlayout.h>

SettingWindow::SettingWindow()
    : QMainWindow(nullptr, Qt::Window)
{
    setWindowTitle("Settings");
    resize(480, 360);
    auto *settingRail = new SettingsRail(this);
    auto *layout = new QHBoxLayout;

    layout->addWidget(settingRail);

    this->setLayout(layout);
}
