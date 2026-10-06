#pragma once

#include <QWidget>

class QLabel;
class QPushButton;

class ServerCall : public QWidget {
    Q_OBJECT

public:
    explicit ServerCall(QWidget *parent = nullptr);

    void setChannelName(const QString &name);
    void setParticipantCount(int count);

private:
    QLabel *statusLabel;
    QLabel *channelLabel;
    QLabel *participantsLabel;
    QPushButton *openButton;

    void setupUi();
};
