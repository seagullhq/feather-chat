
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
