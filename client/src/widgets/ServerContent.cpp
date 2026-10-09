#include "ServerContent.hpp"

#include <QHBoxLayout>
#include <QLabel>
#include <QListWidget>
#include <QVBoxLayout>

ServerContent::ServerContent(QWidget *parent)
    : QWidget(parent)
{
    auto *layout = new QHBoxLayout(this);

    layout->setContentsMargins(12, 12, 12, 12);
    layout->setSpacing(12);

    // Lista principale
    auto *contentList = new QListWidget(this);

    contentList->addItem("Benvenuto nel server");
    contentList->addItem("Canale generale");
    contentList->addItem("Discussioni");
    contentList->addItem("Annunci");

    // Pannello utenti
    auto *usersPanel = new QWidget(this);
    usersPanel->setFixedWidth(220);

    auto *usersLayout = new QVBoxLayout(usersPanel);
    usersLayout->setContentsMargins(0, 0, 0, 0);
    usersLayout->setSpacing(8);

    auto *usersTitle = new QLabel("UTENTI", usersPanel);

    auto *usersList = new QListWidget(usersPanel);

    usersList->addItem("● Francesco");
    usersList->addItem("● Marco");
    usersList->addItem("● Luca");
    usersList->addItem("● Andrea");
    usersList->addItem("● Matteo");

    usersLayout->addWidget(usersTitle);
    usersLayout->addWidget(usersList, 1);

    layout->addWidget(contentList, 1);
    layout->addWidget(usersPanel);
}
