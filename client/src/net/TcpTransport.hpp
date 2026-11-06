#pragma once

#include <QByteArray>
#include <QJsonObject>
#include <QObject>
#include <QQueue>
#include <QString>
#include <QtGlobal>

class QTcpSocket;

// Transport-only TCP client: socket lifecycle, framing and outbox. It knows
// nothing about Feather's control protocol or session state.
class TcpTransport : public QObject {
    Q_OBJECT

public:
    explicit TcpTransport(QObject *parent = nullptr);

    void setEndpoint(const QString &host, quint16 port);
    void connectIfNeeded();
    void disconnectFromHost();
    void abort();
    void reset();
    void send(const QByteArray &payload);

    bool isConnected() const;

signals:
    void connected();
    void disconnected();
    void frameReceived(const QByteArray &payload);
    void errorOccurred(const QString &message);

private slots:
    void onConnected();
    void onReadyRead();
    void onDisconnected();
    void onSocketError();

private:
    void writeFrame(const QByteArray &payload);

    QTcpSocket *socket { nullptr };
    QByteArray buffer;
    QQueue<QByteArray> outbox;
    QString host;
    quint16 port { 0 };
};