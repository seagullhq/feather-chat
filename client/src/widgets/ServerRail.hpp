#pragma once

#include "../data/datastructures.hpp"
#include <QVBoxLayout>
#include <cstdint>
#include <qframe.h>

class ServerRail : public QFrame {

public:
    ServerRail(QWidget *parent);
    void addServer(Server s);
    void getServerAt(int idx);

private:
    int64_t total_server;
};
