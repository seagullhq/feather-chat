#include "CreateServerModal.hpp"
#include <QEvent>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QMouseEvent>
#include <QPainter>
#include <QPushButton>
#include <QVBoxLayout>

CreateServerModal::CreateServerModal(QWidget *parent)
    : QWidget(parent)
{
    setFocusPolicy(Qt::StrongFocus);
    hide();
    parent->installEventFilter(this);

    auto outer = new QVBoxLayout(this);
    outer->setContentsMargins(0, 0, 0, 0);

    card = new QFrame(this);
    card->setObjectName("createCard");
    card->setFixedWidth(360);
    card->setStyleSheet("QFrame#createCard {"
                        "    background: palette(window);"
                        "    border: 1px solid rgba(127, 127, 127, 100);"
                        "    border-radius: 12px;"
                        "}");

    auto *cardLayout = new QVBoxLayout(card);
    cardLayout->setContentsMargins(24, 24, 24, 24);
    cardLayout->setSpacing(12);

    auto *title = new QLabel("Create server", card);
    title->setAlignment(Qt::AlignCenter);
    QFont f = title->font();
    f.setPointSize(f.pointSize() + 4);
    f.setBold(true);
    title->setFont(f);

    auto *subtitle = new QLabel("Create a server");
    subtitle->setAlignment(Qt::AlignCenter);
    subtitle->setWordWrap(true);

    auto *serverName = new QLineEdit(card);
    serverName->setMinimumHeight(36);

    auto *serverNameLabel = new QLabel(card);
    serverNameLabel->setText("Server name:");

    auto *createBtn = new QPushButton("Create server", card);
    createBtn->setMinimumHeight(36);
    auto *cancelBtn = new QPushButton("Cancel", card);
    cancelBtn->setMinimumHeight(36);

    cardLayout->addWidget(title);
    cardLayout->addWidget(subtitle);
    cardLayout->addSpacing(8);
    cardLayout->addWidget(serverNameLabel);
    cardLayout->addWidget(serverName);
    cardLayout->addWidget(createBtn);
    cardLayout->addWidget(cancelBtn);

    outer->addStretch();
    outer->addWidget(card, 0, Qt::AlignHCenter);
    outer->addStretch();

    connect(cancelBtn, &QPushButton::clicked, this, &CreateServerModal::close);
}

void CreateServerModal::open()
{
    setGeometry(parentWidget()->rect());
    raise();
    show();
    setFocus();
}
void CreateServerModal::close()
{
    hide();
}
void CreateServerModal::paintEvent(QPaintEvent *event)
{
    QPainter p(this);
    p.fillRect(rect(), QColor(0, 0, 0, 140));
}
void CreateServerModal::mousePressEvent(QMouseEvent *event)
{
    if (!card->geometry().contains(event->pos()))
        close();
    event->accept();
}
void CreateServerModal::keyPressEvent(QKeyEvent *event)
{
    if (event->key() == Qt::Key_Escape)
        close();
    else
        QWidget::keyPressEvent(event);
}
bool CreateServerModal::eventFilter(QObject *watched, QEvent *event)
{
    if (watched == parentWidget() && event->type() == QEvent::Resize && isVisible())
        setGeometry(parentWidget()->rect());
    return QWidget::eventFilter(watched, event);
}
