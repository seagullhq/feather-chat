#include "AuthPage.hpp"
#include <QFormLayout>
#include <QLineEdit>
#include <QMessageBox>
#include <QPushButton>
#include <QTabWidget>
#include <QVBoxLayout>

AuthPage::AuthPage(QWidget *parent)
    : QWidget(parent)
{
    auto *mainLayout = new QVBoxLayout(this);
    auto *tabWidget = new QTabWidget(this);

    // --- Tab 1: Login ---
    auto *loginWidget = new QWidget(this);
    auto *loginLayout = new QFormLayout(loginWidget);

    loginEmailLineEdit = new QLineEdit(loginWidget);
    loginPasswordLineEdit = new QLineEdit(loginWidget);
    loginPasswordLineEdit->setEchoMode(QLineEdit::Password);

    auto *loginButton = new QPushButton("Accedi", loginWidget);
    loginButton->setDefault(true); // Premere Invio attiva automaticamente il pulsante

    loginLayout->addRow("Email / Username:", loginEmailLineEdit);
    loginLayout->addRow("Password:", loginPasswordLineEdit);
    loginLayout->addRow(loginButton);

    // --- Tab 2: Registrazione ---
    auto *regWidget = new QWidget(this);
    auto *regLayout = new QFormLayout(regWidget);

    regEmailLineEdit = new QLineEdit(regWidget);
    regPasswordLineEdit = new QLineEdit(regWidget);
    regPasswordLineEdit->setEchoMode(QLineEdit::Password);

    regConfirmPasswordLineEdit = new QLineEdit(regWidget);
    regConfirmPasswordLineEdit->setEchoMode(QLineEdit::Password);

    auto *regButton = new QPushButton("Registrati", regWidget);

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
    const QString email = loginEmailLineEdit->text().trimmed();
    const QString password = loginPasswordLineEdit->text();

    if (email.isEmpty() || password.isEmpty()) {
        QMessageBox::warning(this, "Attenzione", "Compila tutti i campi!");
        return;
    }

    emit loginSuccess();
}

void AuthPage::handleRegister()
{
    const QString email = regEmailLineEdit->text().trimmed();
    const QString password = regPasswordLineEdit->text();
    const QString confirmPassword = regConfirmPasswordLineEdit->text();

    if (email.isEmpty() || password.isEmpty() || confirmPassword.isEmpty()) {
        QMessageBox::warning(this, "Attenzione", "Compila tutti i campi!");
        return;
    }

    if (password != confirmPassword) {
        QMessageBox::warning(this, "Errore", "Le password non coincidono!");
        return;
    }

    emit loginSuccess();
}
