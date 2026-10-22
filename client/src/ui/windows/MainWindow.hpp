#pragma once

#include <QMainWindow>
#include <QString>

class Client;
class MainView;
class QStackedWidget;
class SessionService;

class MainWindow final : public QMainWindow {
    Q_OBJECT

public:
    explicit MainWindow(QWidget *parent = nullptr);

private:
    bool applyServer(const QString &serverHost);

    Client *client { nullptr };
    MainView *mainPage { nullptr };
};
