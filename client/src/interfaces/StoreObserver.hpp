#pragma once

#include <QString>
#include <QtGlobal>

// Observer interface for UI/state consumers. Implement in views or adapters.
class StoreObserver {
public:
    virtual ~StoreObserver() = default;

    virtual void onConnectionChanged(bool connected) { Q_UNUSED(connected); }
    virtual void onCurrentUserChanged(const QString &username) { Q_UNUSED(username); }
};