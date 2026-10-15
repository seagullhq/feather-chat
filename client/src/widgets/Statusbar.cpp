#include "Statusbar.hpp"
#include "../tools/Utils.hpp"

#include <QHBoxLayout>
#include <QIcon>
#include <QLabel>
#include <QPainter>
#include <QPixmap>
#include <QPushButton>

Statusbar::Statusbar(QWidget *parent)
    : QFrame(parent)
    , serverLabel(new QLabel("No server"))
    , microphoneLabel(new QLabel("No microphone"))
    , muteButton(new QPushButton)
    , deafenButton(new QPushButton)
{
    setObjectName("statusbarFrame");

    setStyleSheet("QFrame#statusbarFrame {"
                  "    border-top: 1px solid rgba(127, 127, 127, 100);"
                  "}");

    auto *layout = new QHBoxLayout(this);
    layout->setContentsMargins(8, 4, 8, 4);
    layout->setSpacing(8);

    muteButton->setCheckable(true);
    deafenButton->setCheckable(true);

    muteButton->setToolTip("Mute");
    deafenButton->setToolTip("Deafen");

    ICONS::setButtonIcon(muteButton, ":/icons/microphone.svg");
    ICONS::setButtonIcon(deafenButton, ":/icons/ear.svg");

    connect(muteButton, &QPushButton::toggled, this, [this](bool muted) {
        ICONS::setButtonIcon(
            muteButton, muted ? ":/icons/microphone-slash.svg" : ":/icons/microphone.svg");
    });

    connect(deafenButton, &QPushButton::toggled, this, [this](bool deafened) {
        ICONS::setButtonIcon(deafenButton, deafened ? ":/icons/ear-slash.svg" : ":/icons/ear.svg");
    });

    muteButton->setStyleSheet("QPushButton:checked { background-color: red; }");

    deafenButton->setStyleSheet("QPushButton:checked { background-color: gray; }");

    layout->addWidget(muteButton);
    layout->addWidget(deafenButton);

    layout->addStretch();

    serverLabel->setStyleSheet("color: white;");
    microphoneLabel->setStyleSheet("color: white;");

    auto *serverIcon = new QLabel;
    serverIcon->setPixmap(ICONS::whiteIcon(":/icons/server.svg", 18));

    auto *microphoneIcon = new QLabel;
    microphoneIcon->setPixmap(ICONS::whiteIcon(":/icons/microphone.svg", 18));

    layout->addWidget(serverIcon);
    layout->addWidget(serverLabel);
    layout->addWidget(microphoneIcon);
    layout->addWidget(microphoneLabel);
}

void Statusbar::setMicrophoneName(const QString &name)
{
    microphoneLabel->setText(name);
}

void Statusbar::setServerName(const QString &name)
{
    serverLabel->setText(name);
}
