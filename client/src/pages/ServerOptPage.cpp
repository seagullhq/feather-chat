#include "ServerOptPage.hpp"
#include "../tools/Utils.hpp"
#include <QFormLayout>
#include <QLineEdit>
#include <QPushButton>

ServerOptPage::ServerOptPage(QWidget *parent)
{
    auto *serverOptWidget = new QWidget(this);
    auto *serverOptLayout = new QFormLayout(serverOptWidget);
    auto *serverNameEdit = new QLineEdit(serverOptWidget);

    auto *backBtn = new QPushButton;
    backBtn->setObjectName("backBtn");
    backBtn->setToolTip("Back");
    backBtn->setIcon(QIcon(ICONS::whiteIcon(":/icons/arrow-left.svg", 26)));
    backBtn->setIconSize(QSize(26, 26));
    connect(backBtn, &QPushButton::clicked, this, &ServerOptPage::returnHomeRequested);

    serverOptLayout->addWidget(backBtn);
    serverOptLayout->addRow("Server Name:", serverNameEdit);

    auto *createOrModifyServerBtn = new QPushButton;
    createOrModifyServerBtn->setText("Create");
    connect(createOrModifyServerBtn, &QPushButton::clicked, this, []() -> void {

    });
    setLayout(serverOptLayout);
}
