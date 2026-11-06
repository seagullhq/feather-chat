#pragma once

#include <QByteArray>
#include <QJsonObject>
#include <QObject>
#include <QQueue>
#include <QString>
#include <QtGlobal>

class QTimer;

class TcpTransport;
class DataStore;

// Owns the Feather control protocol and session state on top of TcpTransport.
class SessionService : public QObject {
    Q_OBJECT

public:
    explicit SessionService(QObject *parent = nullptr);

    void setServer(const QString &host, quint16 port);

    void login(const QString &identifier, const QString &password);
    void registerUser(const QString &username, const QString &email, const QString &password);
    void join(const QString &room);
    void disconnectFromServer();

    void setDataStore(DataStore *store);

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
    void onTransportConnected();
    void onTransportDisconnected();
    void onTransportFrame(const QByteArray &payload);
    void onTransportError(const QString &message);
    void onTimeout();

private:
    enum class Pending { None, Hello, Login, Register, Join };

    void enqueue(const QJsonObject &message);
    void finishPending();
    void failPending(const QString &message);
    void resetSession();

    void handleMessage(const QJsonObject &message);
    void handleAck(const QJsonObject &ack);
    void handleError(const QJsonObject &error);
    void sendHello();

    TcpTransport *transport { nullptr };
    QTimer *timeout { nullptr };
    DataStore *dataStore { nullptr };

    Pending pending { Pending::None };
    QString host;
    quint16 port { 0 };
    quint32 ssrc { 0 };
    QString username;
    QString pendingPassword;
};