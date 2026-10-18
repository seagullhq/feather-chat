#include "MainPage.hpp"
#include "../widgets/ServerCall.hpp"
#include "../widgets/ServerContent.hpp"
#include "../widgets/ServerRail.hpp"
#include "../widgets/Statusbar.hpp"
#include "../window/ServerModal.hpp"
#include <QHBoxLayout>
#include <QVBoxLayout>

MainPage::MainPage(QWidget *parent)
    : QWidget(parent)
{
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);

    auto *pageLayout = new QHBoxLayout;
    pageLayout->setContentsMargins(0, 0, 0, 0);
    pageLayout->setSpacing(0);

    auto *serverRail = new ServerRail(this);

    auto *contentArea = new QWidget(this);
    auto *serverLayout = new QVBoxLayout(contentArea);
    serverLayout->setContentsMargins(0, 0, 0, 0);
    serverLayout->setSpacing(0);

    auto *serverContent = new ServerContent(contentArea);
    auto *serverCall = new ServerCall(contentArea);

    serverLayout->addWidget(serverContent, 1);
    serverLayout->addWidget(serverCall);

    statusbar = new Statusbar(this);

    pageLayout->addWidget(serverRail);
    pageLayout->addWidget(contentArea, 1);

    layout->addLayout(pageLayout, 1);
    layout->addWidget(statusbar);

    auto *serverModal = new ServerModal(this);
    connect(serverRail, &ServerRail::addServerRequested, serverModal, &ServerModal::open);
}

void MainPage::setServerName(const QString &name)
{
    statusbar->setServerName(name);
}
