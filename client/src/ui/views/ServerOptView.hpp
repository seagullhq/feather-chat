#pragma once

#include <QFrame>
#include <qtmetamacros.h>
class ServerOptView : public QFrame {
    Q_OBJECT
public:
    ServerOptView(QWidget *parent);
    void createServer();
    void modifyServer();
signals:
    void returnHomeRequested();
};
