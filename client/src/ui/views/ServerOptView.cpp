#include "ServerOptView.hpp"
#include "../../core/Utils.hpp"
#include <QFormLayout>
#include <QLineEdit>
#include <QPushButton>
#include <qpushbutton.h>

ServerOptView::ServerOptView(QWidget *parent)
{
    auto *serverOptWidget = new QWidget(this);
    auto *serverOptLayout = new QFormLayout(serverOptWidget);
    auto *serverNameEdit = new QLineEdit(serverOptWidget);

    auto *backBtn = new QPushButton;
    backBtn->setObjectName("backBtn");
    backBtn->setToolTip("Back");
    backBtn->setIcon(QIcon(ICONS::whiteIcon(":/icons/arrow-left.svg", 26)));
    backBtn->setIconSize(QSize(26, 26));

    serverOptLayout->addWidget(backBtn);
    serverOptLayout->addRow("Server Name:", serverNameEdit);

    auto *createOrModifyServerBtn = new QPushButton;
    createOrModifyServerBtn->setText("Create");

    QT_MANAGER::connectWithContext(this,
        QT_MANAGER::Conn { backBtn, &QPushButton::clicked, &ServerOptView::returnHomeRequested },
        QT_MANAGER::Conn { createOrModifyServerBtn, &QPushButton::clicked, []() -> void {

                          } });

    setLayout(serverOptLayout);
}
