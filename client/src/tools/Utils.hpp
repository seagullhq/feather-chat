
#include <QIcon>
#include <QPainter>
#include <QPixmap>
#include <QPushButton>

namespace ICONS {
static QPixmap whiteIcon(const QString &resourcePath, int size)
{
    QPixmap pixmap = QIcon(resourcePath).pixmap(size, size);

    QPainter painter(&pixmap);
    painter.setCompositionMode(QPainter::CompositionMode_SourceIn);
    painter.fillRect(pixmap.rect(), Qt::white);

    return pixmap;
}

static void setButtonIcon(QPushButton *button, const QString &resourcePath)
{
    button->setIcon(QIcon(whiteIcon(resourcePath, 18)));
    button->setIconSize(QSize(18, 18));
}
}

namespace QT_MANAGER {
template<typename Sender, typename Receiver, typename... Pairs>
void connectAll(Sender *sender, Receiver *reciver, Pairs... pair)
{
    (QObject::connect(sender, pair.first, reciver, pair.second), ...);
}

template<typename Sender, typename Signal, typename Slot>
struct Conn {
    Sender *sender;
    Signal signal;
    Slot slot;
};

template<typename Context, typename... Conns>
void connectWithContext(Context *ctx, Conns... c)
{
    (QObject::connect(c.sender, c.signal, ctx, c.slot), ...);
}

};
