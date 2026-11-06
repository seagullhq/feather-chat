#pragma once

#include "../widgets/Statusbar.hpp"
#include <QWidget>

class CreateServerModal;
class JoinServerModal;
class ServerModal;

class MainView : public QWidget {
    Q_OBJECT

public:
    explicit MainView(QWidget *parent = nullptr);

    // The status bar sits inside the page since it took over the window's
    // chrome, so MainWindow reports the active server through this.
    void setServerName(const QString &name);

private:
    Statusbar *statusbar = nullptr;

    // All three are children of MainView, so they stack over the page and
    // are resized with it. ServerModal routes to the other two by signal.
    ServerModal *serverModal = nullptr;
    CreateServerModal *createServerModal = nullptr;
    JoinServerModal *joinServerModal = nullptr;
};
