#include "Statusbar.hpp"
#include <QButtonGroup>
#include <QLabel>
#include <QPushButton>
// class StatusBar : public QHBoxLayout {
// public:
//     StatusBar();
//     void addWidget(QWidget *widget);
//     void addStretch();
//     void addSpacing(int size);
// };

Statusbar::Statusbar()
    : serverLabel(new QLabel("No server"))
    , microphoneLabel(new QLabel("No microphone"))
    , muteButton(new QPushButton("Mute"))
    , deafenButton(new QPushButton("Deafen"))
{
    setContentsMargins(8, 4, 8, 4);
    setSpacing(8);

    muteButton->setCheckable(true);
    deafenButton->setCheckable(true);

    addWidget(muteButton);
    addWidget(deafenButton);

    addStretch();

    addWidget(serverLabel);
    addWidget(microphoneLabel);
}

void Statusbar::setMicrophoneName(const QString &name)
{
    microphoneLabel->setText(name);
}

void Statusbar::setServerName(const QString &name)
{
    serverLabel->setText(name);
}
