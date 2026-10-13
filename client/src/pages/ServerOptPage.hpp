#pragma once

#include <QFrame>
#include <qtmetamacros.h>
class ServerOptPage : public QFrame {
    Q_OBJECT
public:
    ServerOptPage(QWidget *parent);
    void createServer();
    void modifyServer();
signals:
    void returnHomeRequested();
};
