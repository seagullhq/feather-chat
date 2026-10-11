#include "ControlClient.hpp"

#include <QAbstractSocket>
#include <QJsonDocument>
#include <QRandomGenerator>
#include <QTcpSocket>
#include <QTimer>
#include <QtEndian>

namespace {
constexpr int kTimeoutMs = 10000; // matches the server's Mongo context
constexpr quint32 kMaxFrame = 1u << 20; // §7: len MUST be < 1 MiB
}

ControlClient::ControlClient(QObject *parent)
    : QObject(parent)
    , socket(new QTcpSocket(this))
    , timeout(new QTimer(this))
    , ssrc(QRandomGenerator::global()->generate() | 1u) // keep it non-zero
{
    timeout->setSingleShot(true);
    timeout->setInterval(kTimeoutMs);

    connect(socket, &QTcpSocket::connected, this, &ControlClient::onConnected);
    connect(socket, &QTcpSocket::readyRead, this, &ControlClient::onReadyRead);
    connect(socket, &QTcpSocket::disconnected, this, &ControlClient::onDisconnected);
    connect(socket, &QTcpSocket::errorOccurred, this, &ControlClient::onSocketError);
    connect(timeout, &QTimer::timeout, this, &ControlClient::onTimeout);
}

void ControlClient::setServer(const QString &host, quint16 port)
{
    this->host = host;
    this->port = port;
}

void ControlClient::login(const QString &identifier, const QString &password)
{
    pending = Pending::Login;
    enqueue({
        { QStringLiteral("type"), QStringLiteral("LOGIN") },
        { QStringLiteral("identifier"), identifier },
        { QStringLiteral("password"), password },
    });
}

void ControlClient::registerUser(
    const QString &username, const QString &email, const QString &password)
{
    pending = Pending::Register;
    this->username = username;
    pendingPassword = password; // only until the ACK, then auto-login
    enqueue({
        { QStringLiteral("type"), QStringLiteral("REGISTER") },
        { QStringLiteral("username"), username },
        { QStringLiteral("email"), email },
        { QStringLiteral("password"), password },
    });
}

void ControlClient::join(const QString &room)
{
    pending = Pending::Join;
    enqueue({
        { QStringLiteral("type"), QStringLiteral("JOIN") },
        { QStringLiteral("room"), room },
    });
}

void ControlClient::disconnectFromServer()
{
    socket->disconnectFromHost();
}

bool ControlClient::isConnected() const
{
    return socket->state() == QAbstractSocket::ConnectedState;
}

void ControlClient::enqueue(const QJsonObject &message)
{
    if (socket->state() != QAbstractSocket::ConnectedState) {
        outbox.enqueue(message);
        if (socket->state() == QAbstractSocket::UnconnectedState && !host.isEmpty()) {
            socket->connectToHost(host, port);
        }
        timeout->start();
        return;
    }

    writeFrame(message);
    timeout->start();
}

void ControlClient::writeFrame(const QJsonObject &message)
{
    const QByteArray payload = QJsonDocument(message).toJson(QJsonDocument::Compact);
    QByteArray frame(4, '\0');
    qToBigEndian<quint32>(static_cast<quint32>(payload.size()), frame.data());
    frame += payload;
    socket->write(frame);
}

void ControlClient::flushOutbox()
{
    while (!outbox.isEmpty()) {
        writeFrame(outbox.dequeue());
    }
}

void ControlClient::sendHello()
{
    pending = Pending::Hello;
    enqueue({
        { QStringLiteral("type"), QStringLiteral("HELLO") },
        { QStringLiteral("ssrc"), static_cast<double>(ssrc) },
        { QStringLiteral("name"), username },
    });
}

void ControlClient::finishPending()
{
    pending = Pending::None;
    timeout->stop();
}

void ControlClient::failPending(const QString &message)
{
    const Pending wasPending = pending;
    finishPending();

    switch (wasPending) {
    case Pending::Register:
        emit registrationFailed(message);
        break;
    case Pending::Login:
        emit authenticationFailed(message);
        break;
    default:
        break; // HELLO/JOIN failures have no auth UI
    }
}

void ControlClient::onConnected()
{
    emit connected();
    flushOutbox();
}

// Frames can arrive split or coalesced, so accumulate and cut whole frames.
void ControlClient::onReadyRead()
{
    buffer += socket->readAll();

    while (buffer.size() >= 4) {
        const quint32 length = qFromBigEndian<quint32>(buffer.constData());
        if (length == 0 || length >= kMaxFrame) {
            socket->abort(); // protocol violation; the server never sends this
            return;
        }
        if (buffer.size() < 4 + static_cast<int>(length)) {
            return; // wait for the rest of the frame
        }

        const QByteArray payload = buffer.mid(4, static_cast<int>(length));
        buffer.remove(0, 4 + static_cast<int>(length));

        const QJsonDocument document = QJsonDocument::fromJson(payload);
        if (document.isObject()) {
            handleMessage(document.object());
        }
        // Malformed JSON is tolerated (§7 interop rule), not fatal.
    }
}

void ControlClient::onDisconnected()
{
    if (pending != Pending::None) {
        failPending(QStringLiteral("connection lost"));
    }
    outbox.clear();
    buffer.clear();
    emit disconnected();
}

void ControlClient::onSocketError()
{
    if (pending != Pending::None) {
        failPending(socket->errorString());
    }
    outbox.clear();
}

void ControlClient::onTimeout()
{
    if (pending != Pending::None) {
        failPending(QStringLiteral("il server non risponde"));
    }
}

void ControlClient::handleMessage(const QJsonObject &message)
{
    const QString type = message.value(QStringLiteral("type")).toString();
    if (type == QStringLiteral("ACK")) {
        handleAck(message);
    }
    else if (type == QStringLiteral("ERROR")) {
        handleError(message);
    }
    // Unknown types are tolerated (§7 interop rule).
}

void ControlClient::handleAck(const QJsonObject &ack)
{
    const Pending wasPending = pending;
    finishPending();

    switch (wasPending) {
    case Pending::Login:
        username = ack.value(QStringLiteral("name")).toString();
        emit authenticationSucceeded(username);
        sendHello(); // JOIN is gated on this session being authed
        break;
    case Pending::Register:
        emit registrationSucceeded();
        login(username, pendingPassword); // REGISTER does not log in (control.go)
        pendingPassword.clear();
        break;
    case Pending::Hello:
        if (!ack.contains(QStringLiteral("ssrc"))) {
            ssrc = QRandomGenerator::global()->generate() | 1u; // collision, re-roll
            sendHello();
        }
        break;
    case Pending::Join:
        emit roomJoined(ack.value(QStringLiteral("room")).toString());
        break;
    case Pending::None:
        break;
    }
}

void ControlClient::handleError(const QJsonObject &error)
{
    const QString message = error.value(QStringLiteral("error")).toString();
    const Pending wasPending = pending;
    finishPending();

    if (wasPending == Pending::Register) {
        emit registrationFailed(message);
    }
    else if (wasPending == Pending::Login) {
        emit authenticationFailed(message);
    }
}
