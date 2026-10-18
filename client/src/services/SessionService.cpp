#include "SessionService.hpp"

#include "../net/TcpTransport.hpp"
#include "../store/DataStore.hpp"

#include <QAbstractSocket>
#include <QJsonDocument>
#include <QRandomGenerator>
#include <QTimer>
#include <QtEndian>

namespace {
constexpr int kTimeoutMs = 10000;
}

SessionService::SessionService(QObject *parent)
    : QObject(parent)
    , transport(new TcpTransport(this))
    , timeout(new QTimer(this))
    , ssrc(QRandomGenerator::global()->generate() | 1u)
{
    timeout->setSingleShot(true);
    timeout->setInterval(kTimeoutMs);

    connect(transport, &TcpTransport::connected, this, &SessionService::onTransportConnected);
    connect(transport, &TcpTransport::disconnected, this, &SessionService::onTransportDisconnected);
    connect(transport, &TcpTransport::frameReceived, this, &SessionService::onTransportFrame);
    connect(transport, &TcpTransport::errorOccurred, this, &SessionService::onTransportError);
    connect(timeout, &QTimer::timeout, this, &SessionService::onTimeout);
}

void SessionService::setServer(const QString &newHost, quint16 newPort)
{
    if (host == newHost && port == newPort) {
        return;
    }
    transport->setEndpoint(newHost, newPort);
    resetSession();
    host = newHost;
    port = newPort;
}

void SessionService::login(const QString &identifier, const QString &password)
{
    pending = Pending::Login;
    enqueue({
        { QStringLiteral("type"), QStringLiteral("LOGIN") },
        { QStringLiteral("identifier"), identifier },
        { QStringLiteral("password"), password },
    });
}

void SessionService::registerUser(
    const QString &usernameValue, const QString &email, const QString &password)
{
    pending = Pending::Register;
    username = usernameValue;
    pendingPassword = password;
    enqueue({
        { QStringLiteral("type"), QStringLiteral("REGISTER") },
        { QStringLiteral("username"), usernameValue },
        { QStringLiteral("email"), email },
        { QStringLiteral("password"), password },
    });
}

void SessionService::join(const QString &room)
{
    pending = Pending::Join;
    enqueue({
        { QStringLiteral("type"), QStringLiteral("JOIN") },
        { QStringLiteral("room"), room },
    });
}

void SessionService::disconnectFromServer()
{
    transport->disconnectFromHost();
}

void SessionService::setDataStore(DataStore *store)
{
    dataStore = store;
}

bool SessionService::isConnected() const
{
    return transport->isConnected();
}

void SessionService::enqueue(const QJsonObject &message)
{
    const QByteArray payload = QJsonDocument(message).toJson(QJsonDocument::Compact);
    transport->send(payload);
    timeout->start();
}

void SessionService::resetSession()
{
    transport->reset();
    pendingPassword.clear();
    pending = Pending::None;
    timeout->stop();
}

void SessionService::finishPending()
{
    pending = Pending::None;
    timeout->stop();
}

void SessionService::failPending(const QString &message)
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
        break;
    }
}

void SessionService::onTransportConnected()
{
    if (dataStore) {
        dataStore->setConnected(true);
    }
    emit connected();
}

void SessionService::onTransportDisconnected()
{
    if (dataStore) {
        dataStore->setConnected(false);
    }
    if (pending != Pending::None) {
        failPending(QStringLiteral("connection lost"));
    }
    emit disconnected();
}

void SessionService::onTransportFrame(const QByteArray &payload)
{
    const QJsonDocument document = QJsonDocument::fromJson(payload);
    if (document.isObject()) {
        handleMessage(document.object());
    }
}

void SessionService::onTransportError(const QString &message)
{
    if (pending != Pending::None) {
        failPending(message);
    }
}

void SessionService::onTimeout()
{
    if (pending != Pending::None) {
        failPending(QStringLiteral("il server non risponde"));
    }
}

void SessionService::handleMessage(const QJsonObject &message)
{
    const QString type = message.value(QStringLiteral("type")).toString();
    if (type == QStringLiteral("ACK")) {
        handleAck(message);
    }
    else if (type == QStringLiteral("ERROR")) {
        handleError(message);
    }
}

void SessionService::handleAck(const QJsonObject &ack)
{
    const Pending wasPending = pending;
    finishPending();

    switch (wasPending) {
    case Pending::Login:
        username = ack.value(QStringLiteral("name")).toString();
        if (dataStore) {
            dataStore->setCurrentUser(username);
        }
        emit authenticationSucceeded(username);
        sendHello();
        break;
    case Pending::Register:
        emit registrationSucceeded();
        login(username, pendingPassword);
        pendingPassword.clear();
        break;
    case Pending::Hello:
        if (!ack.contains(QStringLiteral("ssrc"))) {
            ssrc = QRandomGenerator::global()->generate() | 1u;
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

void SessionService::handleError(const QJsonObject &error)
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

void SessionService::sendHello()
{
    pending = Pending::Hello;
    enqueue({
        { QStringLiteral("type"), QStringLiteral("HELLO") },
        { QStringLiteral("ssrc"), static_cast<double>(ssrc) },
        { QStringLiteral("name"), username },
    });
}