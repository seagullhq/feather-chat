#include "AuthPage.hpp"
#include <QComboBox>
#include <QFormLayout>
#include <QLineEdit>
#include <QMessageBox>
#include <QPushButton>
#include <QTabWidget>
#include <QVBoxLayout>

namespace {
constexpr auto kDefaultServerHost = "127.0.0.1"; // la porta e' sempre 7700
}

AuthPage::AuthPage(QWidget *parent)
    : QWidget(parent)
{
    auto *mainLayout = new QVBoxLayout(this);
    auto *tabWidget = new QTabWidget(this);

    // --- Server ---
    auto *serverLayout = new QFormLayout;
    serverComboBox = new QComboBox(this);
    serverComboBox->setEditable(true);
    serverComboBox->setInsertPolicy(QComboBox::NoInsert);
    serverComboBox->addItem(QString::fromLatin1(kDefaultServerHost));
    serverLayout->addRow("Server:", serverComboBox);

    mainLayout->addLayout(serverLayout);

    // --- Tab 1: Login ---
    auto *loginWidget = new QWidget(this);
    auto *loginLayout = new QFormLayout(loginWidget);

    loginEmailLineEdit = new QLineEdit(loginWidget);
    loginPasswordLineEdit = new QLineEdit(loginWidget);
    loginPasswordLineEdit->setEchoMode(QLineEdit::Password);

    loginButton = new QPushButton("Accedi", loginWidget);
    loginButton->setDefault(true); // Premere Invio attiva automaticamente il pulsante

    loginLayout->addRow("Email / Username:", loginEmailLineEdit);
    loginLayout->addRow("Password:", loginPasswordLineEdit);
    loginLayout->addRow(loginButton);

    // --- Tab 2: Registrazione ---
    auto *regWidget = new QWidget(this);
    auto *regLayout = new QFormLayout(regWidget);

    regUsernameLineEdit = new QLineEdit(regWidget);
    regEmailLineEdit = new QLineEdit(regWidget);
    regPasswordLineEdit = new QLineEdit(regWidget);
    regPasswordLineEdit->setEchoMode(QLineEdit::Password);

    regConfirmPasswordLineEdit = new QLineEdit(regWidget);
    regConfirmPasswordLineEdit->setEchoMode(QLineEdit::Password);

    regButton = new QPushButton("Registrati", regWidget);

    regLayout->addRow("Username:", regUsernameLineEdit);
    regLayout->addRow("Email:", regEmailLineEdit);
    regLayout->addRow("Password:", regPasswordLineEdit);
    regLayout->addRow("Conferma Password:", regConfirmPasswordLineEdit);
    regLayout->addRow(regButton);

    // Aggiunta schede al TabWidget
    tabWidget->addTab(loginWidget, "Accedi");
    tabWidget->addTab(regWidget, "Registrati");

    mainLayout->addWidget(tabWidget);

    // --- Connessioni ---
    connect(loginButton, &QPushButton::clicked, this, &AuthPage::handleLogin);
    connect(regButton, &QPushButton::clicked, this, &AuthPage::handleRegister);

    // Invio con tasto Enter dai campi di testo
    connect(loginPasswordLineEdit, &QLineEdit::returnPressed, loginButton, &QPushButton::click);
    connect(regConfirmPasswordLineEdit, &QLineEdit::returnPressed, regButton, &QPushButton::click);
}

void AuthPage::handleLogin()
{
    const QString host = serverHost();
    const QString identifier = loginEmailLineEdit->text().trimmed();
    const QString password = loginPasswordLineEdit->text();

    if (host.isEmpty() || identifier.isEmpty() || password.isEmpty()) {
        QMessageBox::warning(this, "Attenzione", "Compila tutti i campi!");
        return;
    }

    if (!hostAccepted(host)) {
        return;
    }

    setBusy(true);
    emit loginRequested(host, identifier, password);
}

void AuthPage::handleRegister()
{
    const QString host = serverHost();
    const QString username = regUsernameLineEdit->text().trimmed();
    const QString email = regEmailLineEdit->text().trimmed();
    const QString password = regPasswordLineEdit->text();
    const QString confirmPassword = regConfirmPasswordLineEdit->text();

    if (host.isEmpty() || username.isEmpty() || email.isEmpty() || password.isEmpty()
        || confirmPassword.isEmpty()) {
        QMessageBox::warning(this, "Attenzione", "Compila tutti i campi!");
        return;
    }

    if (!hostAccepted(host)) {
        return;
    }

    // Stesse regole di validateRegistration (server/control.go): lo username e'
    // contato in code point, la password in byte per il limite di bcrypt.
    if (username.toUcs4().size() < 3 || username.toUcs4().size() > 32) {
        QMessageBox::warning(this, "Errore", "Lo username deve avere 3-32 caratteri.");
        return;
    }

    if (password != confirmPassword) {
        QMessageBox::warning(this, "Errore", "Le password non coincidono!");
        return;
    }

    if (password.toUtf8().size() < 8 || password.toUtf8().size() > 72) {
        QMessageBox::warning(this, "Errore", "La password deve avere 8-72 byte.");
        return;
    }

    setBusy(true);
    emit registerRequested(host, username, email, password);
}

void AuthPage::setBusy(bool busy)
{
    loginButton->setEnabled(!busy);
    regButton->setEnabled(!busy);
}

QString AuthPage::serverHost() const
{
    return serverComboBox->currentText().trimmed();
}

bool AuthPage::hostAccepted(const QString &host)
{
    // Only the host goes on the wire; the port is fixed at 7700.
    if (host.contains(':')) {
        QMessageBox::warning(
            this, "Indirizzo non valido", "Inserisci solo l'host: la porta e' sempre 7700.");
        return false;
    }
    return true;
}

void AuthPage::onAuthenticationSucceeded()
{
    setBusy(false);
}

void AuthPage::onAuthenticationFailed(const QString &message)
{
    setBusy(false);
    QMessageBox::warning(this, "Accesso non riuscito", message);
}

void AuthPage::onRegistrationSucceeded()
{
    // L'auto-login e' in corso: lascia i pulsanti disabilitati fino all'ACK.
}

void AuthPage::onRegistrationFailed(const QString &message)
{
    setBusy(false);
    QMessageBox::warning(this, "Registrazione non riuscita", message);
}
