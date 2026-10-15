#include "ServerCall.hpp"

#include "../tools/Utils.hpp"
#include "../window/SettingWindow.hpp"
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

    userSettingsButton = new QPushButton;
    ICONS::setButtonIcon(userSettingsButton, ":/icons/gear-six.svg");
    userSettingsButton->setObjectName("userSettings");
    userSettingsButton->setToolTip("User settings");

    ICONS::setButtonIcon(connectButton, ":/icons/phone.svg");
    connectButton->setObjectName("openButton");
    connectButton->setCheckable(true);

    connectButton->setToolTip("Mute");
    QT_MANAGER::connectWithContext(this,
        QT_MANAGER::Conn { connectButton, &QPushButton::toggled,
            [this](bool leave) {
                ICONS::setButtonIcon(
                    connectButton, leave ? ":/icons/phone-slash.svg" : ":/icons/phone.svg");
            } },
        QT_MANAGER::Conn { userSettingsButton, &QPushButton::clicked, [this]() {
                              auto *settings = new SettingWindow;
                              settings->setAttribute(
                                  Qt::WA_DeleteOnClose); // closes and frees on window close
                              settings->show();
                          } });

    connectButton->setStyleSheet("QPushButton:checked { background-color: red; }");

    layout->addWidget(statusLabel);
    layout->addWidget(channelLabel);
    layout->addWidget(participantsLabel);
    layout->addStretch();
    layout->addWidget(userSettingsButton);
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
