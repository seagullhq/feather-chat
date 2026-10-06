#include "MainPage.hpp"
#include "../widgets/ServerCall.hpp"
#include "../widgets/ServerContent.hpp"
#include <QVBoxLayout>

MainPage::MainPage(QWidget *parent)
{
    auto *serverLayout = new QVBoxLayout(this);

    serverLayout->setContentsMargins(0, 0, 0, 0);
    serverLayout->setSpacing(0);

    auto *serverContent = new ServerContent(this);
    auto *serverCall = new ServerCall(this);

    serverLayout->addWidget(serverContent, 1);
    serverLayout->addWidget(serverCall);
}
