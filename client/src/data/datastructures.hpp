#pragma once

#include <QString>
#include <QVector>
enum class Visibility { Public, Private };

struct Room {
    QString name;
    int online;
    Visibility visible;
};

struct Server {
    QString id, name, address;
    bool isConnected = false;
    QVector<Room> serverRooms;
};

typedef struct Server Server;
typedef struct Room Room;
