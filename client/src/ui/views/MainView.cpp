#include "MainView.hpp"
#include "../dialogs/CreateServerModal.hpp"
#include "../dialogs/JoinServerModal.hpp"
#include "../dialogs/ServerModal.hpp"
#include "../widgets/ServerCall.hpp"
#include "../widgets/ServerContent.hpp"
#include "../widgets/ServerRail.hpp"
#include "../widgets/Statusbar.hpp"
#include <QHBoxLayout>
#include <QVBoxLayout>

MainView::MainView(QWidget *parent)
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

    // The rail opens the "explore" dialog; that dialog only announces the
    // user's choice, and the view opens the matching form dialog.
    serverModal = new ServerModal(this);
    createServerModal = new CreateServerModal(this);
    joinServerModal = new JoinServerModal(this);

    connect(serverRail, &ServerRail::addServerRequested, serverModal, &ServerModal::open);
    connect(serverModal, &ServerModal::createServerRequested, createServerModal, &CreateServerModal::open);
    connect(serverModal, &ServerModal::joinServerRequested, joinServerModal, &JoinServerModal::open);
}

void MainView::setServerName(const QString &name)
{
    statusbar->setServerName(name);
}
