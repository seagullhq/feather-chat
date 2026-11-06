#include "DataStore.hpp"

#include <QMutexLocker>

void DataStore::addObserver(StoreObserver *observer)
{
    QMutexLocker lock(&mutex_);
    observers_.push_back(observer);
}

void DataStore::removeObserver(StoreObserver *observer)
{
    QMutexLocker lock(&mutex_);
    for (auto it = observers_.begin(); it != observers_.end(); ++it) {
        if (*it == observer) {
            observers_.erase(it);
            break;
        }
    }
}

void DataStore::setConnected(bool connected)
{
    {
        QMutexLocker lock(&mutex_);
        connected_ = connected;
    }
    notifyConnection(connected);
}

bool DataStore::isConnected() const
{
    QMutexLocker lock(&mutex_);
    return connected_;
}

void DataStore::setCurrentUser(const QString &username)
{
    {
        QMutexLocker lock(&mutex_);
        currentUser_ = username;
    }
    notifyUser(username);
}

QString DataStore::currentUser() const
{
    QMutexLocker lock(&mutex_);
    return currentUser_;
}

void DataStore::notifyConnection(bool connected)
{
    std::vector<StoreObserver *> snapshot;
    {
        QMutexLocker lock(&mutex_);
        snapshot = observers_;
    }
    for (auto *observer : snapshot) {
        observer->onConnectionChanged(connected);
    }
}

void DataStore::notifyUser(const QString &username)
{
    std::vector<StoreObserver *> snapshot;
    {
        QMutexLocker lock(&mutex_);
        snapshot = observers_;
    }
    for (auto *observer : snapshot) {
        observer->onCurrentUserChanged(username);
    }
}