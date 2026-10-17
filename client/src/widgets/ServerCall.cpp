#include "ServerCall.hpp"

#include "../tools/Utils.hpp"
#include <QHBoxLayout>
#include <QLabel>
#include <QPainter>
#include <QPixmap>
#include <QPushButton>
#include <QString>
#include <qpushbutton.h>

ServerCall::ServerCall(QWidget *parent)
    : QWidget(parent)
    , statusLabel(new QLabel(this))
    , channelLabel(new QLabel(this))
    , participantsLabel(new QLabel(this))
    , connectButton(new QPushButton(this))
    , muteButton(new QPushButton)
    , deafenButton(new QPushButton)
{
    setupUi();
}

void ServerCall::setupUi()
{
    setFixedHeight(56);
    auto palette = this->palette();
    palette.setColor(QPalette::Window, palette.color(QPalette::Mid));
    setPalette(palette);

    auto *layout = new QHBoxLayout(this);
    layout->setContentsMargins(16, 8, 12, 8);
    layout->setSpacing(8);

    statusLabel->setText("●");
    statusLabel->setStyleSheet("color: #43b581;");

    channelLabel->setText("General");
    channelLabel->setObjectName("channelLabel");

    participantsLabel->setText("0 users");
    participantsLabel->setObjectName("participantsLabel");

    muteButton->setCheckable(true);
    muteButton->setToolTip("Mute");
    ICONS::setButtonIcon(muteButton, ":/icons/microphone.svg");
    muteButton->setStyleSheet("QPushButton:checked { background-color: red; }");

    deafenButton->setCheckable(true);
    deafenButton->setToolTip("Deafen");
    ICONS::setButtonIcon(deafenButton, ":/icons/ear.svg");
    deafenButton->setStyleSheet("QPushButton:checked { background-color: gray; }");

    QT_MANAGER::connectWithContext(this,
        QT_MANAGER::Conn { muteButton, &QPushButton::toggled,
            [this](bool muted) {
                ICONS::setButtonIcon(
                    muteButton, muted ? ":/icons/microphone-slash.svg" : ":/icons/microphone.svg");
            } },
        QT_MANAGER::Conn { deafenButton, &QPushButton::toggled, [this](bool deafened) {
                              ICONS::setButtonIcon(deafenButton,
                                  deafened ? ":/icons/ear-slash.svg" : ":/icons/ear.svg");
                          } });

    ICONS::setButtonIcon(connectButton, ":/icons/phone.svg");
    connectButton->setObjectName("openButton");
    connectButton->setCheckable(true);

    connectButton->setToolTip("Mute");
    QT_MANAGER::connectWithContext(
        this, QT_MANAGER::Conn { connectButton, &QPushButton::toggled, [this](bool leave) {
                                    ICONS::setButtonIcon(connectButton,
                                        leave ? ":/icons/phone-slash.svg" : ":/icons/phone.svg");
                                } });

    connectButton->setStyleSheet("QPushButton:checked { background-color: red; }");

    layout->addWidget(statusLabel);
    layout->addWidget(channelLabel);
    layout->addWidget(participantsLabel);
    layout->addStretch();
    layout->addWidget(muteButton);
    layout->addWidget(deafenButton);
    layout->addWidget(connectButton);
}

void ServerCall::setChannelName(const QString &name)
{
    channelLabel->setText(name);
}

void ServerCall::setParticipantCount(int count)
{
    participantsLabel->setText(count == 1 ? "1 user" : QString("%1 users").arg(count));
}
