#include "ServerCall.hpp"

#include "../tools/Utils.hpp"
#include <QHBoxLayout>
#include <QLabel>
#include <QPainter>
#include <QPixmap>
#include <QPushButton>
#include <QString>

ServerCall::ServerCall(QWidget *parent)
    : QWidget(parent)
    , statusLabel(new QLabel(this))
    , channelLabel(new QLabel(this))
    , participantsLabel(new QLabel(this))
    , openButton(new QPushButton(this))
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

    ICONS::setButtonIcon(openButton, ":/icons/phone.svg");
    openButton->setObjectName("openButton");
    openButton->setCheckable(true);

    openButton->setToolTip("Mute");
    connect(openButton, &QPushButton::toggled, this, [this](bool leave) {
        ICONS::setButtonIcon(openButton, leave ? ":/icons/phone-slash.svg" : ":/icons/phone.svg");
    });

    openButton->setStyleSheet("QPushButton:checked { background-color: red; }");

    layout->addWidget(statusLabel);
    layout->addWidget(channelLabel);
    layout->addWidget(participantsLabel);
    layout->addStretch();
    layout->addWidget(openButton);
}

void ServerCall::setChannelName(const QString &name)
{
    channelLabel->setText(name);
}

void ServerCall::setParticipantCount(int count)
{
    participantsLabel->setText(count == 1 ? "1 user" : QString("%1 users").arg(count));
}
