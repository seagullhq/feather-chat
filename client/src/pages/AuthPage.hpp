#pragma once

#include <QWidget>

class QLineEdit;
class QPushButton;

class AuthPage : public QWidget {
    Q_OBJECT

public:
    explicit AuthPage(QWidget *parent = nullptr);

signals:
    // Segnale emesso quando il login o la registrazione hanno successo
    void loginSuccess();

private slots:
    void handleLogin();
    void handleRegister();

private:
    QLineEdit *loginEmailLineEdit { nullptr };
    QLineEdit *loginPasswordLineEdit { nullptr };

    QLineEdit *regEmailLineEdit { nullptr };
    QLineEdit *regPasswordLineEdit { nullptr };
    QLineEdit *regConfirmPasswordLineEdit { nullptr };
};
