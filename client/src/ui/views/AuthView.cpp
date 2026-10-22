#include "AuthView.hpp"
#include "../../core/Constants.hpp"
#include "../../core/Utils.hpp"
#include <QComboBox>
#include <QFormLayout>
#include <QLineEdit>
#include <QMessageBox>
#include <QPushButton>
#include <QTabWidget>
#include <QVBoxLayout>

AuthView::AuthView(QWidget *parent)
    : QWidget(parent)
{
    auto *mainLayout = new QVBoxLayout(this);
    auto *tabWidget = new QTabWidget(this);

    // Server IP input
    auto *serverLayout = new QFormLayout;
    serverComboBox = new QComboBox(this);
    serverComboBox->setEditable(true);
    serverComboBox->setInsertPolicy(QComboBox::NoInsert);
    serverComboBox->addItem(QString::fromLatin1(DefaultHost));
    serverLayout->addRow("Server:", serverComboBox);

    mainLayout->addLayout(serverLayout);

    // Tab: Login
    auto *loginWidget = new QWidget(this);
    auto *loginLayout = new QFormLayout(loginWidget);

    loginEmailLineEdit = new QLineEdit(loginWidget);
    loginPasswordLineEdit = new QLineEdit(loginWidget);
    loginPasswordLineEdit->setEchoMode(QLineEdit::Password);

    loginButton = new QPushButton("Login", loginWidget);
    loginButton->setDefault(true); // Pressing Enter enable automatically the button

    loginLayout->addRow("Email / Username:", loginEmailLineEdit);
    loginLayout->addRow("Password:", loginPasswordLineEdit);
    loginLayout->addRow(loginButton);

    // Tab: Register
    auto *regWidget = new QWidget(this);
    auto *regLayout = new QFormLayout(regWidget);

    regUsernameLineEdit = new QLineEdit(regWidget);
    regEmailLineEdit = new QLineEdit(regWidget);
    regPasswordLineEdit = new QLineEdit(regWidget);
    regPasswordLineEdit->setEchoMode(QLineEdit::Password);

    regConfirmPasswordLineEdit = new QLineEdit(regWidget);
    regConfirmPasswordLineEdit->setEchoMode(QLineEdit::Password);

    regButton = new QPushButton("Regiser", regWidget);

    regLayout->addRow("Username:", regUsernameLineEdit);
    regLayout->addRow("Email:", regEmailLineEdit);
    regLayout->addRow("Password:", regPasswordLineEdit);
    regLayout->addRow("Confirm Password:", regConfirmPasswordLineEdit);
    regLayout->addRow(regButton);

    // Add the tabs to the TabWidget
    tabWidget->addTab(loginWidget, "Login");
    tabWidget->addTab(regWidget, "Register");

    mainLayout->addWidget(tabWidget);

    QT_MANAGER::connectWithContext(this,
        QT_MANAGER::Conn { loginButton, &QPushButton::clicked, &AuthView::handleLogin },
        QT_MANAGER::Conn { regButton, &QPushButton::clicked, &AuthView::handleRegister });

    connect(loginPasswordLineEdit, &QLineEdit::returnPressed, loginButton, &QPushButton::click);
    connect(regConfirmPasswordLineEdit, &QLineEdit::returnPressed, regButton, &QPushButton::click);
}

void AuthView::handleLogin()
{
    const QString host = serverHost();
    const QString identifier = loginEmailLineEdit->text().trimmed();
    const QString password = loginPasswordLineEdit->text();

    if (host.isEmpty() || identifier.isEmpty() || password.isEmpty()) {
        QMessageBox::warning(this, "Warning", "Compile all the fields!");
        return;
    }

    if (!hostAccepted(host)) {
        return;
    }

    setBusy(true);
    emit loginRequested(host, identifier, password);
}

void AuthView::handleRegister()
{
    const QString host = serverHost();
    const QString username = regUsernameLineEdit->text().trimmed();
    const QString email = regEmailLineEdit->text().trimmed();
    const QString password = regPasswordLineEdit->text();
    const QString confirmPassword = regConfirmPasswordLineEdit->text();

    if (host.isEmpty() || username.isEmpty() || email.isEmpty() || password.isEmpty()
        || confirmPassword.isEmpty()) {
        QMessageBox::warning(this, "Warning", "Compile all the fields!");
        return;
    }

    if (!hostAccepted(host)) {
        return;
    }

    // Same rules of validateRegistration (server/control.go): Username is
    // counted in code point, The password in byte for the limit of bcrypt.
    if (username.toUcs4().size() < 3 || username.toUcs4().size() > 32) {
        QMessageBox::warning(this, "Error", "The username must have a lenght of 3-32 characters.");
        return;
    }

    if (password != confirmPassword) {
        QMessageBox::warning(this, "Error", "The password does not match!");
        return;
    }

    if (password.toUtf8().size() < 8 || password.toUtf8().size() > 72) {
        QMessageBox::warning(this, "Errore", "La password deve avere 8-72 byte.");
        return;
    }

    setBusy(true);
    emit registerRequested(host, username, email, password);
}

void AuthView::setBusy(bool busy)
{
    loginButton->setEnabled(!busy);
    regButton->setEnabled(!busy);
}

QString AuthView::serverHost() const
{
    return serverComboBox->currentText().trimmed();
}

bool AuthView::hostAccepted(const QString &host)
{
    // Only the host goes on the wire; the port is fixed at 7700.
    if (host.contains(':')) {
        QMessageBox::warning(
            this, "Indirizzo non valido", "Inserisci solo l'host: la porta e' sempre 7700.");
        return false;
    }
    return true;
}

void AuthView::onAuthenticationSucceeded()
{
    setBusy(false);
}

void AuthView::onAuthenticationFailed(const QString &message)
{
    setBusy(false);
    QMessageBox::warning(this, "Accesso non riuscito", message);
}

void AuthView::onRegistrationSucceeded()
{
    // TODO
}

void AuthView::onRegistrationFailed(const QString &message)
{
    setBusy(false);
    QMessageBox::warning(this, "Registrazione non riuscita", message);
}
