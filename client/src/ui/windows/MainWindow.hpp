#pragma once

#include <QMainWindow>
#include <QString>

class ControlClient;
class MainView;
class QStackedWidget;

class MainWindow final : public QMainWindow {
    Q_OBJECT

public:
    explicit MainWindow(QWidget *parent = nullptr);

private:
    bool applyServer(const QString &serverHost);

    ControlClient *client { nullptr };
    MainView *mainPage { nullptr };
};
