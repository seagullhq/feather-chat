#pragma once

#include <QWidget>

class QComboBox;
class QLineEdit;
class QPushButton;

class AuthPage : public QWidget {
    Q_OBJECT

public:
    explicit AuthPage(QWidget *parent = nullptr);

signals:
    // Requests only; the page stays disabled until the server answers. The
    // host travels with the request so the caller can retarget (the port is
    // fixed at 7700).
    void loginRequested(
        const QString &serverHost, const QString &identifier, const QString &password);
    void registerRequested(const QString &serverHost, const QString &username, const QString &email,
        const QString &password);

public slots:
    void onAuthenticationSucceeded();
    void onAuthenticationFailed(const QString &message);
    void onRegistrationSucceeded();
    void onRegistrationFailed(const QString &message);

private slots:
    void handleLogin();
    void handleRegister();

private:
    void setBusy(bool busy);
    QString serverHost() const;
    bool hostAccepted(const QString &host);

    QComboBox *serverComboBox { nullptr };

    QLineEdit *loginEmailLineEdit { nullptr };
    QLineEdit *loginPasswordLineEdit { nullptr };

    QLineEdit *regEmailLineEdit { nullptr };
    QLineEdit *regUsernameLineEdit { nullptr };
    QLineEdit *regPasswordLineEdit { nullptr };
    QLineEdit *regConfirmPasswordLineEdit { nullptr };

    QPushButton *loginButton { nullptr };
    QPushButton *regButton { nullptr };
};
