#pragma once
#include <QJsonObject>
#include <QObject>
#include <QTcpSocket>

class TcpClient : public QObject {
    Q_OBJECT
public:
    explicit TcpClient(QObject *parent = nullptr);

    void connectToServer(const QString &host, quint16 port);
    void close();
    void send(const QJsonObject &message);
    bool isConnected() const;

signals:
    void connected();
    void disconnected();
    void messageReceived(const QJsonObject &message);
    void failed(const QString &reason);

private:
    void onReadyRead();

    QTcpSocket m_socket;
    QByteArray m_buffer;
};
