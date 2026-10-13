#include "MainWindow.hpp"
#include "../net/ControlClient.hpp"
#include "../widgets/ServerRail.hpp"
#include "../widgets/Statusbar.hpp"
#include "AuthPage.hpp"
#include "MainPage.hpp"
#include "ServerOptPage.hpp"
#include <QHBoxLayout>
#include <QStackedWidget>
#include <QVBoxLayout>

namespace {
constexpr quint16 kServerPort = 7700; // la porta e' sempre 7700
}

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

    connect(authPage, &AuthPage::loginRequested, this,
        [this, authPage](const QString &host, const QString &identifier, const QString &password) {
            if (!applyServer(host)) {
                authPage->onAuthenticationFailed("Host del server non valido.");
                return;
            }
            client->login(identifier, password);
        });
    connect(authPage, &AuthPage::registerRequested, this,
        [this, authPage](const QString &host, const QString &username, const QString &email,
            const QString &password) {
            if (!applyServer(host)) {
                authPage->onRegistrationFailed("Host del server non valido.");
                return;
            }
            client->registerUser(username, email, password);
        });

    connect(client, &ControlClient::authenticationSucceeded, authPage,
        &AuthPage::onAuthenticationSucceeded);
    connect(client, &ControlClient::authenticationSucceeded, this,
        [stackedWidget]() { stackedWidget->setCurrentIndex(1); });
    connect(
        client, &ControlClient::authenticationFailed, authPage, &AuthPage::onAuthenticationFailed);
    connect(client, &ControlClient::registrationSucceeded, authPage,
        &AuthPage::onRegistrationSucceeded);
    connect(client, &ControlClient::registrationFailed, authPage, &AuthPage::onRegistrationFailed);
    connect(serverRail, &ServerRail::addServerRequested, this,
        [stackedWidget, serverOptPage]() { stackedWidget->setCurrentWidget(serverOptPage); });
    connect(serverOptPage, &ServerOptPage::returnHomeRequested, this,
        [stackedWidget, mainWidget]() { stackedWidget->setCurrentWidget(mainWidget); });
}

bool MainWindow::applyServer(const QString &serverHost)
{
    const QString host = serverHost.trimmed();
    if (host.isEmpty()) {
        return false;
    }

    if (host != currentServerHost) {
        // A session is bound to the server it authenticated against, so a
        // different host drops the connection before the next login.
        client->disconnectFromServer();
        client->setServer(host, kServerPort);
        currentServerHost = host;
    }

    statusbar->setServerName(host);
    return true;
}
