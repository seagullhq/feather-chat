#include "TcpTransport.hpp"
#include "../core/Constants.hpp"
#include <QAbstractSocket>
#include <QTcpSocket>
#include <QtEndian>

TcpTransport::TcpTransport(QObject *parent)
    : QObject(parent)
    , socket(new QTcpSocket(this))
{
    connect(socket, &QTcpSocket::connected, this, &TcpTransport::onConnected);
    connect(socket, &QTcpSocket::readyRead, this, &TcpTransport::onReadyRead);
    connect(socket, &QTcpSocket::disconnected, this, &TcpTransport::onDisconnected);
    connect(socket, &QTcpSocket::errorOccurred, this, &TcpTransport::onSocketError);
}

void TcpTransport::setEndpoint(const QString &newHost, quint16 newPort)
{
    if (host == newHost && port == newPort) {
        return;
    }
    reset();
    host = newHost;
    port = newPort;
}

void TcpTransport::connectIfNeeded()
{
    if (socket->state() == QAbstractSocket::UnconnectedState && !host.isEmpty()) {
        socket->connectToHost(host, port);
    }
}

void TcpTransport::disconnectFromHost()
{
    socket->disconnectFromHost();
}

void TcpTransport::abort()
{
    socket->abort();
}

void TcpTransport::reset()
{
    socket->abort();
    outbox.clear();
    buffer.clear();
}

void TcpTransport::send(const QByteArray &payload)
{
    if (socket->state() != QAbstractSocket::ConnectedState) {
        outbox.enqueue(payload);
        connectIfNeeded();
        return;
    }
    writeFrame(payload);
}

bool TcpTransport::isConnected() const
{
    return socket->state() == QAbstractSocket::ConnectedState;
}

void TcpTransport::writeFrame(const QByteArray &payload)
{
    QByteArray frame(4, '\0');
    qToBigEndian<quint32>(static_cast<quint32>(payload.size()), frame.data());
    frame += payload;
    socket->write(frame);
}

void TcpTransport::onConnected()
{
    while (!outbox.isEmpty()) {
        writeFrame(outbox.dequeue());
    }
    emit connected();
}

void TcpTransport::onReadyRead()
{
    buffer += socket->readAll();

    while (buffer.size() >= 4) {
        const quint32 length = qFromBigEndian<quint32>(buffer.constData());
        if (length == 0 || length >= MaxFrame) {
            socket->abort();
            return;
        }
        if (buffer.size() < 4 + static_cast<int>(length)) {
            return;
        }

        const QByteArray payload = buffer.mid(4, static_cast<int>(length));
        buffer.remove(0, 4 + static_cast<int>(length));
        emit frameReceived(payload);
    }
}

void TcpTransport::onDisconnected()
{
    outbox.clear();
    buffer.clear();
    emit disconnected();
}

void TcpTransport::onSocketError()
{
    emit errorOccurred(socket->errorString());
}
