#pragma once

#include "../interfaces/StoreObserver.hpp"

#include <QMutex>
#include <QString>
#include <vector>

// In-memory application state. Thread-safe; UI consumes it via observers.
class DataStore {
public:
    DataStore() = default;

    void addObserver(StoreObserver *observer);
    void removeObserver(StoreObserver *observer);

    void setConnected(bool connected);
    bool isConnected() const;

    void setCurrentUser(const QString &username);
    QString currentUser() const;

private:
    void notifyConnection(bool connected);
    void notifyUser(const QString &username);

    mutable QMutex mutex_;
    bool connected_ { false };
    QString currentUser_;
    std::vector<StoreObserver *> observers_;
};