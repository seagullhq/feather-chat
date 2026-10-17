#include "Statusbar.hpp"
#include "../tools/Utils.hpp"
#include "../window/SettingWindow.hpp"
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
{
    setObjectName("statusbarFrame");

    setStyleSheet("QFrame#statusbarFrame {"
                  "    border-top: 1px solid rgba(127, 127, 127, 100);"
                  "}");

    auto *layout = new QHBoxLayout(this);
    layout->setContentsMargins(8, 4, 8, 4);
    layout->setSpacing(8);

    userSettingsButton = new QPushButton;
    ICONS::setButtonIcon(userSettingsButton, ":/icons/gear-six.svg");
    userSettingsButton->setObjectName("userSettings");
    userSettingsButton->setToolTip("User settings");

    layout->addWidget(userSettingsButton);
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

    QT_MANAGER::connectWithContext(
        this, QT_MANAGER::Conn { userSettingsButton, &QPushButton::clicked, [this]() {
                                    auto *settings = new SettingWindow;
                                    settings->setAttribute(
                                        Qt::WA_DeleteOnClose); // closes and frees on window close
                                    settings->show();
                                } });
}

void Statusbar::setMicrophoneName(const QString &name)
{
    microphoneLabel->setText(name);
}

void Statusbar::setServerName(const QString &name)
{
    serverLabel->setText(name);
}
