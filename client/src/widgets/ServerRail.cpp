#include "ServerRail.hpp"

#include <QFrame>
#include <QIcon>
#include <QPainter>
#include <QPixmap>
#include <QPushButton>
#include <QVBoxLayout>
#include <qpushbutton.h>

ServerRail::ServerRail(QWidget *parent)
    : QFrame(parent)
{
    // "border: none" first: Qt only enters the stylesheet box drawing when the
    // top edge has a declared style, so a lone border-right is silently dropped
    // (see QRenderRule::hasNativeBorder in qstylesheetstyle.cpp).
    setObjectName("serverRail");
    setStyleSheet("QFrame#serverRail {"
                  "    border: none;"
                  "    border-right: 1px solid rgba(127, 127, 127, 100);"
                  "}");

    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(8, 4, 8, 4);
    layout->setSpacing(0);

    auto *homeButton = new QPushButton;
    homeButton->setObjectName("homeButton");
    homeButton->setToolTip("Home");
    homeButton->setIcon(QIcon(ICONS::whiteIcon(":/icons/chats-teardrop.svg", 26)));
    homeButton->setIconSize(QSize(26, 26));

    layout->addWidget(homeButton, 0, Qt::AlignHCenter);
    layout->addSpacing(8);

    auto *separator = new QFrame;
    separator->setFrameShape(QFrame::HLine);
    separator->setFrameShadow(QFrame::Sunken);

    layout->addWidget(separator);

    // TODO: Add user list of server here!

    layout->addStretch();

    auto *exploreServer = new QPushButton;
    exploreServer->setObjectName("addServer");
    exploreServer->setToolTip("Explore Server");
    exploreServer->setIcon(QIcon(ICONS::whiteIcon(":/icons/compass.svg", 26)));
    exploreServer->setIconSize(QSize(26, 26));
    QT_MANAGER::connectWithContext(this,
        QT_MANAGER::Conn { exploreServer, &QPushButton::clicked, &ServerRail::addServerRequested });

    layout->addWidget(exploreServer, 0, Qt::AlignHCenter);
}
