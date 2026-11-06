#include "Client.hpp"

#include "../services/SessionService.hpp"
#include "../store/DataStore.hpp"

Client::Client(QObject *parent)
    : QObject(parent)
    , sessionService(new SessionService(this))
    , dataStore(new DataStore())
{
    sessionService->setDataStore(dataStore);
}

Client::~Client()
{
    delete dataStore;
}

SessionService *Client::session() const
{
    return sessionService;
}

DataStore *Client::store() const
{
    return dataStore;
}