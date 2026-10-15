#pragma once

#include <QByteArray>
#include <QJsonObject>
#include <QObject>
#include <QQueue>
#include <QString>
#include <QtGlobal>

class QTcpSocket;
class QTimer;

// Owns the TCP control connection to one server (docs/wire-format.md §7) and
// turns the server's length-prefixed JSON frames into signals. Authentication
// is session-bound: the server gates JOIN on a completed LOGIN (server/control.go).
class ControlClient : public QObject {
    Q_OBJECT

public:
    explicit ControlClient(QObject *parent = nullptr);

    // Target server. Requests connect lazily on first use.
    void setServer(const QString &host, quint16 port);

    void login(const QString &identifier, const QString &password);
    void registerUser(const QString &username, const QString &email, const QString &password);
    void join(const QString &room);
    void disconnectFromServer();

    bool isConnected() const;

signals:
    void connected();
    void disconnected();
    void authenticationSucceeded(const QString &username);
    void authenticationFailed(const QString &message);
    void registrationSucceeded();
    void registrationFailed(const QString &message);
    void roomJoined(const QString &room);

private slots:
    void onConnected();
    void onReadyRead();
    void onDisconnected();
    void onSocketError();
    void onTimeout();

private:
    // The server answers REGISTER, LOGIN, HELLO and JOIN with the same ACK
    // shape, so the outstanding request is how an ACK is interpreted.
    enum class Pending { None, Hello, Login, Register, Join };

    void enqueue(const QJsonObject &message);
    void writeFrame(const QJsonObject &message);
    void flushOutbox();
    void sendHello();
    void finishPending();
    void failPending(const QString &message);
    void handleMessage(const QJsonObject &message);
    void handleAck(const QJsonObject &ack);
    void handleError(const QJsonObject &error);

    QTcpSocket *socket { nullptr };
    QTimer *timeout { nullptr };
    QByteArray buffer;
    QQueue<QJsonObject> outbox;
    Pending pending { Pending::None };
    QString host;
    quint16 port { 0 };
    quint32 ssrc { 0 }; // §3: random per session, never 0
    QString username;
    QString pendingPassword; // held only until the post-register LOGIN
};
