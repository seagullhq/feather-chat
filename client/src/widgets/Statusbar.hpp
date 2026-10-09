#pragma once

#include <QFrame>
#include <QString>

class QLabel;
class QPushButton;

class Statusbar : public QFrame {
public:
    explicit Statusbar(QWidget *parent);

    void setMicrophoneName(const QString &name);
    void setServerName(const QString &name);

private:
    QLabel *serverLabel;
    QLabel *microphoneLabel;

    QPushButton *muteButton;
    QPushButton *deafenButton;
};
