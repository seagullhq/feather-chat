#include "ServerRail.hpp"
#include <QFrame>
#include <QIcon>
#include <QPainter>
#include <QPixmap>
#include <QPushButton>
#include <qlayoutitem.h>

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
} // namespace

ServerRail::ServerRail()
{
    setContentsMargins(8, 4, 8, 4);

    auto *homeButton = new QPushButton;
    homeButton->setObjectName("homeButton");
    homeButton->setToolTip("Home");
    homeButton->setIcon(QIcon(whiteIcon(":/icons/home.svg", 26)));
    homeButton->setIconSize(QSize(26, 26));
    addWidget(homeButton, 0, Qt::AlignHCenter);
    addSpacing(8);

    auto *separator = new QFrame;
    separator->setFrameShape(QFrame::HLine);
    separator->setFrameShadow(QFrame::Sunken);
    addWidget(separator);

    // TODO: Add user list of server here!

    addStretch();

    auto *addServer = new QPushButton;
    addServer->setObjectName("addServer");
    addServer->setToolTip("Add server");
    addServer->setIcon(QIcon(whiteIcon(":/icons/plus.svg", 26)));
    addServer->setIconSize(QSize(26, 26));
    addWidget(addServer, 0, Qt::AlignHCenter);
}
