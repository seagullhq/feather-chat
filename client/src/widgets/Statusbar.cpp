#include "Statusbar.hpp"
#include <QButtonGroup>
#include <QIcon>
#include <QLabel>
#include <QPainter>
#include <QPixmap>
#include <QPushButton>

namespace {
QPixmap whiteIcon(const QString &resourcePath, int size)
{
    QPixmap pixmap = QIcon(resourcePath).pixmap(size, size);
    QPainter painter(&pixmap);
    painter.setCompositionMode(QPainter::CompositionMode_SourceIn);
    painter.fillRect(pixmap.rect(), Qt::white);
    painter.end();
    return pixmap;
}

void setButtonIcon(QPushButton *button, const QString &resourcePath)
{
    button->setIcon(QIcon(whiteIcon(resourcePath, 18)));
    button->setIconSize(QSize(18, 18));
}
} // namespace

Statusbar::Statusbar()
    : serverLabel(new QLabel("No server"))
    , microphoneLabel(new QLabel("No microphone"))
    , muteButton(new QPushButton)
    , deafenButton(new QPushButton)
{
    setContentsMargins(8, 4, 8, 4);
    setSpacing(8);

    muteButton->setCheckable(true);
    deafenButton->setCheckable(true);
    muteButton->setToolTip("Mute");
    deafenButton->setToolTip("Deafen");

    setButtonIcon(muteButton, ":/icons/microphone.svg");
    setButtonIcon(deafenButton, ":/icons/ear.svg");

    connect(muteButton, &QPushButton::toggled, this, [this](bool muted) {
        setButtonIcon(muteButton, muted ? ":/icons/microphone-slash.svg"
                                       : ":/icons/microphone.svg");
    });
    connect(deafenButton, &QPushButton::toggled, this, [this](bool deafened) {
        setButtonIcon(deafenButton, deafened ? ":/icons/ear-slash.svg"
                                             : ":/icons/ear.svg");
    });

    muteButton->setStyleSheet("QPushButton:checked { background-color: red; }");
    deafenButton->setStyleSheet("QPushButton:checked { background-color: gray; }");

    addWidget(muteButton);
    addWidget(deafenButton);

    addStretch();

    serverLabel->setStyleSheet("color: white;");
    microphoneLabel->setStyleSheet("color: white;");

    auto *serverIcon = new QLabel;
    serverIcon->setPixmap(whiteIcon(":/icons/server.svg", 18));
    auto *microphoneIcon = new QLabel;
    microphoneIcon->setPixmap(whiteIcon(":/icons/microphone.svg", 18));

    addWidget(serverIcon);
    addWidget(serverLabel);
    addWidget(microphoneIcon);
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
