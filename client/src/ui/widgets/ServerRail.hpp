#pragma once

#include "../../models/Models.hpp"
#include "../../core/Utils.hpp"
#include <QVBoxLayout>
#include <cstdint>
#include <qframe.h>

class ServerRail : public QFrame {
    Q_OBJECT

public:
    ServerRail(QWidget *parent);
    void addServer(Server s);
    void getServerAt(int idx);

signals:
    void addServerRequested();

private:
    int64_t total_server;
};
