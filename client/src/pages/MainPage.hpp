#pragma once

#include "../widgets/Statusbar.hpp"
#include <QWidget>

class MainPage : public QWidget {
    Q_OBJECT

public:
    explicit MainPage(QWidget *parent = nullptr);

    // The status bar sits inside the page since it took over the window's
    // chrome, so MainWindow reports the active server through this.
    void setServerName(const QString &name);

private:
    Statusbar *statusbar = nullptr;
};
