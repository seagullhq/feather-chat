#include "MainWindow.hpp"
#include "../net/ControlClient.hpp"
#include "../pages/AuthPage.hpp"
#include "../pages/MainPage.hpp"
#include "../pages/ServerOptPage.hpp"
#include "../tools/Base.hpp"
#include "../widgets/ServerRail.hpp"
#include "../widgets/Statusbar.hpp"
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
    statusbar = new Statusbar(mainWidget);

    pageLayout->addWidget(serverRail);
    pageLayout->addWidget(mainPage, 1);

    layout->addLayout(pageLayout, 1);
    layout->addWidget(statusbar);

    stackedWidget->addWidget(authPage); // Index 0
    stackedWidget->addWidget(mainWidget); // Index 1

    auto *serverOptPage = new ServerOptPage(this);

    stackedWidget->addWidget(serverOptPage);

    setCentralWidget(stackedWidget);

    // La connessione TCP si apre alla prima richiesta (login/registrazione).
    client = new ControlClient(this);

    // Events Handler
    QT_MANAGER::connectAll(client, authPage,
        std::pair { &ControlClient::authenticationSucceeded, &AuthPage::onAuthenticationSucceeded },
        std::pair { &ControlClient::authenticationFailed, &AuthPage::onAuthenticationFailed },
        std::pair { &ControlClient::registrationSucceeded, &AuthPage::onRegistrationSucceeded },
        std::pair { &ControlClient::registrationFailed, &AuthPage::onRegistrationFailed });

    // Login/Register Handler
    QT_MANAGER::connectAll(authPage, this,
        std::pair { &AuthPage::loginRequested,
            [this, authPage](
                const QString &host, const QString &identifier, const QString &password) {
                if (!applyServer(host)) {
                    authPage->onAuthenticationFailed("Host del server non valido.");
                    return;
                }
                client->login(identifier, password);
            } },
        std::pair { &AuthPage::registerRequested,
            [this, authPage](const QString &host, const QString &username, const QString &email,
                const QString &password) {
                if (!applyServer(host)) {
                    authPage->onRegistrationFailed("Host del server non valido.");
                    return;
                }
                client->registerUser(username, email, password);
            } });

    // Pages Handler
    QT_MANAGER::connectWithContext(this,
        QT_MANAGER::Conn { client, &ControlClient::authenticationSucceeded,
            [stackedWidget]() { stackedWidget->setCurrentIndex(1); } },
        QT_MANAGER::Conn { serverRail, &ServerRail::addServerRequested,
            [stackedWidget, serverOptPage]() { stackedWidget->setCurrentWidget(serverOptPage); } },
        QT_MANAGER::Conn { serverOptPage, &ServerOptPage::returnHomeRequested,
            [stackedWidget, mainWidget]() { stackedWidget->setCurrentWidget(mainWidget); } });
}
bool MainWindow::applyServer(const QString &serverHost)
{
    const QString host = serverHost.trimmed();
    if (host.isEmpty()) {
        return false;
    }

    // setServer() tears the old socket down synchronously when the host
    // changes, so the login that follows connects fresh instead of racing a
    // Closing one.
    client->setServer(host, kServerPort);

    statusbar->setServerName(host);
    return true;
}
