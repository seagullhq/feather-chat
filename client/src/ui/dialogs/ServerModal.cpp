#include "ServerModal.hpp"
#include <QEvent>
#include <QFrame>
#include <QKeyEvent>
#include <QLabel>
#include <QMouseEvent>
#include <QPainter>
#include <QPushButton>
#include <QVBoxLayout>
ServerModal::ServerModal(QWidget *parent)
    : QWidget(parent)
{
    setFocusPolicy(Qt::StrongFocus);
    hide();
    parent->installEventFilter(this);

    // Outer layout
    auto *outer = new QVBoxLayout(this);
    outer->setContentsMargins(0, 0, 0, 0);

    card = new QFrame(this);
    card->setObjectName("exploreCard");
    card->setFixedWidth(360);
    card->setStyleSheet("QFrame#exploreCard {"
                        "    background: palette(window);"
                        "    border: 1px solid rgba(127, 127, 127, 100);"
                        "    border-radius: 12px;"
                        "}");

    auto *cardLayout = new QVBoxLayout(card);
    cardLayout->setContentsMargins(24, 24, 24, 24);
    cardLayout->setSpacing(12);

    auto *title = new QLabel("Explore server", card);
    title->setAlignment(Qt::AlignCenter);
    QFont f = title->font();
    f.setPointSize(f.pointSize() + 4);
    f.setBold(true);
    title->setFont(f);

    auto *subtitle = new QLabel("Create a new server or Join in an existing one.", card);
    subtitle->setAlignment(Qt::AlignCenter);
    subtitle->setWordWrap(true);

    auto *createBtn = new QPushButton("Create a server", card);
    auto *joinBtn = new QPushButton("Join a server", card);
    auto *cancelBtn = new QPushButton("Cancel", card);
    createBtn->setMinimumHeight(36);
    cancelBtn->setMinimumHeight(36);
    joinBtn->setMinimumHeight(36);

    cardLayout->addWidget(title);
    cardLayout->addWidget(subtitle);
    cardLayout->addSpacing(8);
    cardLayout->addWidget(createBtn);
    cardLayout->addWidget(joinBtn);
    cardLayout->addWidget(cancelBtn);

    outer->addStretch();
    outer->addWidget(card, 0, Qt::AlignHCenter);
    outer->addStretch();

    // The two modals are separate widgets owned by the view, so ServerModal
    // only announces the intent and lets the view decide which one to open.
    connect(createBtn, &QPushButton::clicked, this, [this] {
        close();
        emit createServerRequested();
    });
    connect(joinBtn, &QPushButton::clicked, this, [this] {
        close();
        emit joinServerRequested();
    });
    connect(cancelBtn, &QPushButton::clicked, this, &ServerModal::close);
}

void ServerModal::open()
{
    setGeometry(parentWidget()->rect());
    raise();
    show();
    setFocus();
}
void ServerModal::close()
{
    hide();
}
void ServerModal::paintEvent(QPaintEvent *event)
{
    QPainter p(this);
    p.fillRect(rect(), QColor(0, 0, 0, 140));
}
void ServerModal::mousePressEvent(QMouseEvent *event)
{
    if (!card->geometry().contains(event->pos()))
        close();
    event->accept();
}
void ServerModal::keyPressEvent(QKeyEvent *event)
{
    if (event->key() == Qt::Key_Escape)
        close();
    else
        QWidget::keyPressEvent(event);
}
bool ServerModal::eventFilter(QObject *watched, QEvent *event)
{
    if (watched == parentWidget() && event->type() == QEvent::Resize && isVisible())
        setGeometry(parentWidget()->rect());
    return QWidget::eventFilter(watched, event);
}
