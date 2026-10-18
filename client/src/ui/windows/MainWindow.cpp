#include "MainWindow.hpp"
#include "../../net/ControlClient.hpp"
#include "../views/AuthView.hpp"
#include "../views/MainView.hpp"
#include "../views/ServerOptView.hpp"
#include "../../core/Constants.hpp"
#include "../../core/Utils.hpp"
#include <QStackedWidget>

MainWindow::MainWindow(QWidget *parent)
    : QMainWindow(parent)
{
    setWindowTitle("Feather Chat");
    resize(800, 600);

    auto *stackedWidget = new QStackedWidget(this);

    auto *authPage = new AuthView(this);

    mainPage = new MainView(this);

    stackedWidget->addWidget(authPage); // Index 0
    stackedWidget->addWidget(mainPage); // Index 1

    auto *serverOptPage = new ServerOptView(this);

    stackedWidget->addWidget(serverOptPage);

    setCentralWidget(stackedWidget);

    // La connessione TCP si apre alla prima richiesta (login/registrazione).
    client = new ControlClient(this);

    // Events Handler
    QT_MANAGER::connectAll(client, authPage,
        std::pair { &ControlClient::authenticationSucceeded, &AuthView::onAuthenticationSucceeded },
        std::pair { &ControlClient::authenticationFailed, &AuthView::onAuthenticationFailed },
        std::pair { &ControlClient::registrationSucceeded, &AuthView::onRegistrationSucceeded },
        std::pair { &ControlClient::registrationFailed, &AuthView::onRegistrationFailed });

    // Login/Register Handler
    QT_MANAGER::connectAll(authPage, this,
        std::pair { &AuthView::loginRequested,
            [this, authPage](
                const QString &host, const QString &identifier, const QString &password) {
                if (!applyServer(host)) {
                    authPage->onAuthenticationFailed("Invalid server host.");
                    return;
                }
                client->login(identifier, password);
            } },
        std::pair { &AuthView::registerRequested,
            [this, authPage](const QString &host, const QString &username, const QString &email,
                const QString &password) {
                if (!applyServer(host)) {
                    authPage->onRegistrationFailed("Invalid server host.");
                    return;
                }
                client->registerUser(username, email, password);
            } });

    // Pages Handler
    QT_MANAGER::connectWithContext(this,
        QT_MANAGER::Conn { client, &ControlClient::authenticationSucceeded,
            [stackedWidget]() { stackedWidget->setCurrentIndex(1); } });
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

    mainPage->setServerName(host);
    return true;
}
