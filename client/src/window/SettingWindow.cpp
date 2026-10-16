#include "SettingWindow.hpp"
#include "../widgets/settingsWidgets/SettingsTabs.hpp"

SettingWindow::SettingWindow()
    : QMainWindow(nullptr, Qt::Window)
{
    setWindowTitle("Settings");
    resize(480, 360);

    // QMainWindow installs its own internal layout to manage the central
    // widget, menu bar and status bar, so setLayout() on the window itself is
    // refused with a runtime warning and the layout is thrown away. The tabs
    // go on a central widget instead, as MainWindow does. The tab widget spans
    // the window; there is no sidebar rail any more.
    setCentralWidget(new SettingsTabs(this));
}
