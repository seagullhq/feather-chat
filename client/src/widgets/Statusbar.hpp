#pragma once

#include <QHBoxLayout>
#include <QString>

class QLabel;
class QPushButton;

class Statusbar : public QHBoxLayout {
public:
    explicit Statusbar();

    void setMicrophoneName(const QString &name);
    void setServerName(const QString &name);

private:
    QLabel *serverLabel;
    QLabel *microphoneLabel;

    QPushButton *muteButton;
    QPushButton *deafenButton;
};
