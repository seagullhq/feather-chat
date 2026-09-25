#include "Titlebar.hpp"
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QMouseEvent>
#include <QPainter>
#include <QPen>
#include <QPushButton>
#include <QScreen>
#include <QStyleOption>
#include <QWindow>

// Distance (px) from the screen's top edge that triggers maximize while
// dragging (manual path only). Small enough that only a deliberate
// "throw to the top" hits it.
static constexpr int kMaximizeZone = 4;

static int screenTop(const QPoint &globalPos)
{
    QScreen *screen = QGuiApplication::screenAt(globalPos);
    if (!screen)
        screen = QGuiApplication::primaryScreen();
    return screen->geometry().top();
}

// Windows: the OS move loop handles everything natively (following the
// cursor, drag-to-top maximize, drag-down restore, edge snap, Win11 snap
// layouts). Hand-rolled move()-dragging on Windows fights the WM and
// stutters / stops following.
// Wayland: clients cannot position windows at all — compositor move is
// mandatory.
// Only X11 (etc.) falls back to the manual dragging below.
static bool useNativeSystemMove()
{
    const QString p = QGuiApplication::platformName();
    return p == "windows" || p == "wayland";
}

// Accent-colored custom titlebar. The stylesheet is applied only to this
// widget (and its children), so it doesn't leak into the rest of the app.
// Note: only the three buttons are interactive here (HTCLIENT); everything
// else in the bar is consumed by the OS as a native caption, so hover
// states only ever appear over the buttons.
static const char *kTitlebarStyle = R"(
    Titlebar {
        background-color: #5865f2;
        border-bottom: 1px solid #4752cc;
    }
    Titlebar QLabel {
        color: #ffffff;
        font-size: 13px;
        font-weight: 600;
    }
    Titlebar QPushButton {
        background: transparent;
        color: #ffffff;
        border: none;
        border-radius: 4px;
        min-width: 30px;
        min-height: 24px;
    }
    Titlebar QPushButton:hover {
        background-color: rgba(255, 255, 255, 0.16);
    }
    Titlebar QPushButton#closeBtn:hover {
        background-color: #e81123;
    }
)";

// Window-control buttons drawn directly with QPainter instead of font glyphs:
// - Font glyphs (–□✕ / Segoe MDL2 Assets) all have different baselines and
//   metrics, which is what made the buttons look misaligned. (MDL2 also
//   rasterizes as blank in this Qt build — GDI can't load it.)
// - Drawn shapes (bar / square / X) all share the exact same center point,
//   so they are aligned by construction and match native Windows buttons.
class TitlebarButton : public QPushButton {
public:
    enum Kind { Minimize, Maximize, Close };

    explicit TitlebarButton(Kind kind, QWidget *parent = nullptr)
        : QPushButton(parent)
        , m_kind(kind)
    {
        setObjectName(kind == Minimize   ? "minBtn"
                      : kind == Maximize ? "maxBtn"
                                         : "closeBtn");
        setFocusPolicy(Qt::NoFocus);
        setCursor(Qt::ArrowCursor);
    }

protected:
    void paintEvent(QPaintEvent *) override
    {
        // QSS background + hover states (Titlebar QPushButton:hover,
        // #closeBtn:hover) come from the style, like any QPushButton.
        QStyleOption opt;
        opt.initFrom(this);
        QPainter p(this);
        style()->drawPrimitive(QStyle::PE_PanelButtonCommand, &opt, &p, this);

        p.setRenderHint(QPainter::Antialiasing, true);

        // Glyph color follows the palette so it stays in sync with the QSS
        // text color (white on the accent bar).
        const QColor color = palette().color(QPalette::ButtonText);
        QPen pen(color, qMax(1.5, height() / 16.0));
        pen.setCapStyle(Qt::RoundCap);
        p.setPen(pen);

        const QPointF c = QRectF(rect()).center();
        switch (m_kind) {
        case Minimize:
            p.drawLine(QLineF(c.x() - 5.0, c.y(), c.x() + 5.0, c.y()));
            break;

        case Maximize: {
            const QRectF sq(c.x() - 4.5, c.y() - 4.5, 9.0, 9.0);
            if (window()->isMaximized()) {
                // Restore glyph: two overlapping squares.
                p.drawRect(sq.translated(2.0, -2.0));
                p.drawRect(sq.translated(-2.0, 2.0));
            } else {
                p.drawRect(sq);
            }
            break;
        }

        case Close:
            p.drawLine(QLineF(c.x() - 4.0, c.y() - 4.0, c.x() + 4.0, c.y() + 4.0));
            p.drawLine(QLineF(c.x() - 4.0, c.y() + 4.0, c.x() + 4.0, c.y() - 4.0));
            break;
        }
    }

private:
    Kind m_kind;
};

Titlebar::Titlebar(QWidget *parent)
    : QWidget(parent)
    , windowName(new QLabel(window()->windowTitle(), this))
{

    minBtn = new TitlebarButton(TitlebarButton::Minimize, this);
    maxBtn = new TitlebarButton(TitlebarButton::Maximize, this);
    closeBtn = new TitlebarButton(TitlebarButton::Close, this);

    setStyleSheet(kTitlebarStyle);

    auto *layout = new QHBoxLayout(this);
    layout->setContentsMargins(12, 0, 12, 0);
    layout->setSpacing(8);
    layout->addWidget(windowName);
    layout->addStretch();
    layout->addWidget(minBtn);
    layout->addWidget(maxBtn);
    layout->addWidget(closeBtn);

    connect(minBtn, &QPushButton::clicked, this, &Titlebar::minimizeRequested);
    connect(maxBtn, &QPushButton::clicked, this, &Titlebar::maximizeRequested);
    connect(closeBtn, &QPushButton::clicked, this, &Titlebar::closeRequested);
}

void Titlebar::setWindowName(const QString &newName)
{
    windowName->setText(newName);
}

void Titlebar::changeEvent(QEvent *ev)
{
    if (ev->type() == QEvent::WindowTitleChange)
        windowName->setText(window()->windowTitle());
    QWidget::changeEvent(ev);
}

void Titlebar::mousePressEvent(QMouseEvent *ev)
{
    if (ev->button() != Qt::LeftButton) {
        QWidget::mousePressEvent(ev);
        return;
    }

    if (useNativeSystemMove()) {
        window()->windowHandle()->startSystemMove();
        return;
    }

    dragging = true;
    grabInTitlebar = ev->position().toPoint(); // where in the bar you grabbed
    ev->accept(); // establish the drag grab so mouseMoveEvent keeps receiving events
}

void Titlebar::mouseMoveEvent(QMouseEvent *ev)
{
    if (!dragging || !(ev->buttons() & Qt::LeftButton)) {
        QWidget::mouseMoveEvent(ev);
        return;
    }

    const QPoint cursor = QCursor::pos();

    // Maximized window: drag back down past the top zone to restore it.
    if (window()->isMaximized()) {
        if (cursor.y() > screenTop(cursor) + kMaximizeZone) {
            window()->showNormal();
            window()->move(cursor - grabInTitlebar); // grab point stays under the cursor
        }
        return;
    }

    // Restored window: reach the top edge of the screen -> maximize.
    if (cursor.y() <= screenTop(cursor) + kMaximizeZone) {
        window()->showMaximized();
        return;
    }

    window()->move(cursor - grabInTitlebar);
}

void Titlebar::mouseReleaseEvent(QMouseEvent *ev)
{
    dragging = false;
    QWidget::mouseReleaseEvent(ev);
}

void Titlebar::mouseDoubleClickEvent(QMouseEvent *ev)
{
    if (ev->button() == Qt::LeftButton) {
        if (window()->isMaximized())
            window()->showNormal();
        else
            window()->showMaximized();
    }
}
