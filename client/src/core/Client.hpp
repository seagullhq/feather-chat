#pragma once

#include <QObject>

class SessionService;
class DataStore;

// Application-level facade. UI depends on this, not on transport or protocol.
class Client : public QObject {
    Q_OBJECT

public:
    explicit Client(QObject *parent = nullptr);
    ~Client() override;

    SessionService *session() const;
    DataStore *store() const;

private:
    SessionService *sessionService { nullptr };
    DataStore *dataStore { nullptr };
};