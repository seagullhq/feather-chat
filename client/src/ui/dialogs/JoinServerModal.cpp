#include "JoinServerModal.hpp"
#include <QEvent>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QMouseEvent>
#include <QPainter>
#include <QPushButton>
#include <QVBoxLayout>


JoinServerModal::JoinServerModal(QWidget *parent)
    : QWidget(parent)
{
    setFocusPolicy(Qt::StrongFocus);
    hide();
    parent->installEventFilter(this);

    auto outer = new QVBoxLayout(this);
    outer->setContentsMargins(0, 0, 0, 0);

    card = new QFrame(this);
    card->setObjectName("joinCard");
    card->setFixedWidth(360);
    card->setStyleSheet("QFrame#joinCard {"
                        "    background: palette(window);"
                        "    border: 1px solid rgba(127, 127, 127, 100);"
                        "    border-radius: 12px;"
                        "}");
    auto *cardLayout = new QVBoxLayout(card);
    cardLayout->setContentsMargins(24, 24, 24, 24);
    cardLayout->setSpacing(12);

    auto *title = new QLabel("Join server", card);
    title->setAlignment(Qt::AlignCenter);
    QFont f = title->font();
    f.setPointSize(f.pointSize() + 4);
    f.setBold(true);
    title->setFont(f);

    auto *subtitle = new QLabel("Join a server with the invite code");
    subtitle->setAlignment(Qt::AlignCenter);
    subtitle->setWordWrap(true);

    auto *joinBtn = new QPushButton("Join server", card);
    joinBtn->setMinimumHeight(36);

    auto* inviteLabel = new QLabel(card);
    inviteLabel->setText("Invite code:");

    auto *cancelBtn = new QPushButton("Cancel", card);
    cancelBtn->setMinimumHeight(36);
    auto *invite = new QLineEdit("", card);
    invite->setMinimumHeight(36);

    cardLayout->addWidget(title);
    cardLayout->addWidget(subtitle);
    cardLayout->addSpacing(8);
    cardLayout->addWidget(inviteLabel);
    cardLayout->addWidget(invite);
    cardLayout->addWidget(joinBtn);
    cardLayout->addWidget(cancelBtn);

    outer->addStretch();
    outer->addWidget(card, 0, Qt::AlignHCenter);
    outer->addStretch();

    connect(cancelBtn, &QPushButton::clicked, this, &JoinServerModal::close);
}

void JoinServerModal::open()
{
    setGeometry(parentWidget()->rect());
    raise();
    show();
    setFocus();
}
void JoinServerModal::close()
{
    hide();
}
void JoinServerModal::paintEvent(QPaintEvent *event)
{
    QPainter p(this);
    p.fillRect(rect(), QColor(0, 0, 0, 140));
}
void JoinServerModal::mousePressEvent(QMouseEvent *event)
{
    if (!card->geometry().contains(event->pos()))
        close();
    event->accept();
}
void JoinServerModal::keyPressEvent(QKeyEvent *event)
{
    if (event->key() == Qt::Key_Escape)
        close();
    else
        QWidget::keyPressEvent(event);
}
bool JoinServerModal::eventFilter(QObject *watched, QEvent *event)
{
    if (watched == parentWidget() && event->type() == QEvent::Resize && isVisible())
        setGeometry(parentWidget()->rect());
    return QWidget::eventFilter(watched, event);
}
