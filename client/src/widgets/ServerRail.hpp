#pragma once

#include "../data/datastructures.hpp"
#include <QVBoxLayout>
#include <cstdint>

class ServerRail : public QVBoxLayout {

public:
    ServerRail() { }
    void addServer(Server s);
    void getServerAt(int idx);

private:
    int64_t total_server;
};
