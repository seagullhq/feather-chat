#include "MainWindow.hpp"
#include "../widgets/ServerRail.hpp"
#include "../widgets/Statusbar.hpp"
#include "AuthPage.hpp"
#include "MainPage.hpp"
#include <QHBoxLayout>
#include <QStackedWidget>
#include <QVBoxLayout>

MainWindow::MainWindow(QWidget *parent)
    : QMainWindow(parent)
{
    setWindowTitle("Feather Chat");
    resize(800, 600);

    auto *stackedWidget = new QStackedWidget(this);

    auto *authPage = new AuthPage(this);

    auto *mainWidget = new QWidget(this);
    auto *layout = new QVBoxLayout(mainWidget);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);

    auto *pageLayout = new QHBoxLayout;
    pageLayout->setContentsMargins(0, 0, 0, 0);
    pageLayout->setSpacing(0);

    auto *serverRail = new ServerRail(mainWidget);
    auto *mainPage = new MainPage(mainWidget);
    auto *statusbar = new Statusbar(mainWidget);

    pageLayout->addWidget(serverRail);
    pageLayout->addWidget(mainPage, 1);

    layout->addLayout(pageLayout, 1);
    layout->addWidget(statusbar);

    stackedWidget->addWidget(authPage); // Index 0
    stackedWidget->addWidget(mainWidget); // Index 1

    setCentralWidget(stackedWidget);

    connect(authPage, &AuthPage::loginSuccess, this,
        [stackedWidget]() { stackedWidget->setCurrentIndex(1); });
}
