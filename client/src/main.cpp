#include "./widgets/Statusbar.hpp"
#include "widgets/ServerRail.hpp"
#include "widgets/Titlebar.hpp"
#include <QApplication>
#include <QFrame>
#include <QHBoxLayout>
#include <QLabel>
#include <QMainWindow>
#include <QPushButton>
#include <QStackedWidget>
#include <QVBoxLayout>

#ifdef Q_OS_WIN
#include <windows.h>
#include <windowsx.h>
#endif

class MainWindow : public QMainWindow {
public:
    MainWindow()
    {
        // Windows keeps its native window styles (needed for Aero snap) and
        // we hide the non-client area via WM_NCCALCSIZE below. On other
        // platforms a frameless window is required for the custom titlebar.
#ifndef Q_OS_WIN
        setWindowFlags(Qt::FramelessWindowHint);
#endif
        setWindowTitle("Feather Chat");

        Statusbar *statusbar = new Statusbar();
        auto *central = new QWidget;
        auto *layout = new QVBoxLayout(central);
        layout->setContentsMargins(0, 0, 0, 0);
        layout->setSpacing(0);
        auto *content = new QWidget;

        titleBar = new Titlebar(this);

        connect(titleBar, &Titlebar::minimizeRequested, this, &QWidget::showMinimized);
        connect(titleBar, &Titlebar::maximizeRequested, this, &MainWindow::toggleMaximize);
        connect(titleBar, &Titlebar::closeRequested, this, &QWidget::close);
        layout->addWidget(titleBar);

        auto *pageLayout = new QHBoxLayout;
        pageLayout->addLayout(new ServerRail);
        pageLayout->addWidget(content, 1);
        layout->addLayout(pageLayout, 1);
        layout->addLayout(statusbar);
        setCentralWidget(central);

        resize(800, 600);
    }
    void toggleMaximize()
    {
        if (isMaximized())
            showNormal();
        else
            showMaximized();
    }

protected:
    // Windows: the trick is to *keep* the native window styles (WS_CAPTION,
    // WS_THICKFRAME, WS_MAXIMIZEBOX — without them Aero snap refuses to
    // maximize on top-drag) and eat the non-client area instead:
    //
    // - WM_NCCALCSIZE -> client covers the whole window: no native titlebar
    //   is ever painted, and we can hand-pick what area is a caption.
    // - WM_GETMINMAXINFO -> maximize to the monitor work area exactly, so
    //   the zero-size frame doesn't push the window past the taskbar.
    // - WM_NCHITTEST -> resize edges + titlebar as native caption (drag,
    //   drag-to-top maximize, drag-down restore are then OS-provided).
    bool nativeEvent(const QByteArray &eventType, void *message, qintptr *result) override
    {
#ifdef Q_OS_WIN
        MSG *msg = static_cast<MSG *>(message);
        switch (msg->message) {
        case WM_NCCALCSIZE:
            if (msg->wParam == TRUE) {
                // Client area covers the whole window: no native titlebar
                // is painted. When maximized, Windows inflates the window
                // past the screen edges (to hide resize borders we don't
                // have) — shrink the client rect back so the titlebar and
                // edges stay visible.
                if (IsZoomed(msg->hwnd)) {
                    auto *params = reinterpret_cast<NCCALCSIZE_PARAMS *>(msg->lParam);
                    const int fx
                        = GetSystemMetrics(SM_CXSIZEFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER);
                    const int fy
                        = GetSystemMetrics(SM_CYSIZEFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER);
                    RECT &r = params->rgrc[0];
                    r.left += fx;
                    r.top += fy;
                    r.right -= fx;
                }
                *result = 0;
                return true;
            }
            break;

        case WM_GETMINMAXINFO: {
            auto *mmi = reinterpret_cast<MINMAXINFO *>(msg->lParam);
            HMONITOR mon = MonitorFromWindow(msg->hwnd, MONITOR_DEFAULTTONEAREST);
            MONITORINFO mi = { sizeof(mi) };
            if (GetMonitorInfo(mon, &mi)) {
                mmi->ptMaxPosition.x = mi.rcWork.left - mi.rcMonitor.left;
                mmi->ptMaxPosition.y = mi.rcWork.top - mi.rcMonitor.top;
                mmi->ptMaxSize.x = mi.rcWork.right - mi.rcWork.left;
                mmi->ptMaxSize.y = mi.rcWork.bottom - mi.rcWork.top;
            }
            *result = 0;
            return true;
        }

        case WM_NCHITTEST: {
            const POINT pt = { GET_X_LPARAM(msg->lParam), GET_Y_LPARAM(msg->lParam) };

            // Resize borders (only when not maximized; physical pixels here).
            if (!IsZoomed(msg->hwnd)) {
                RECT wr;
                GetWindowRect(msg->hwnd, &wr);
                const int border
                    = GetSystemMetrics(SM_CXSIZEFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER);
                const bool l = pt.x < wr.left + border;
                const bool r = pt.x >= wr.right - border;
                const bool t = pt.y < wr.top + border;
                const bool b = pt.y >= wr.bottom - border;
                if (t && l) {
                    *result = HTTOPLEFT;
                    return true;
                }
                if (t && r) {
                    *result = HTTOPRIGHT;
                    return true;
                }
                if (b && l) {
                    *result = HTBOTTOMLEFT;
                    return true;
                }
                if (b && r) {
                    *result = HTBOTTOMRIGHT;
                    return true;
                }
                if (l) {
                    *result = HTLEFT;
                    return true;
                }
                if (r) {
                    *result = HTRIGHT;
                    return true;
                }
                if (t) {
                    *result = HTTOP;
                    return true;
                }
                if (b) {
                    *result = HTBOTTOM;
                    return true;
                }
            }

            // Titlebar: caption except over the three buttons. lParam is in
            // physical pixels; Qt geometry is logical -> divide by DPR.
            const QPoint pos = (QPointF(pt.x, pt.y) / devicePixelRatioF()).toPoint();
            auto globalRect
                = [](const QWidget *w) { return QRect(w->mapToGlobal(QPoint(0, 0)), w->size()); };
            const QRect bar = globalRect(titleBar);
            if (bar.contains(pos)) {
                if (globalRect(titleBar->minButton()).contains(pos)
                    || globalRect(titleBar->maxButton()).contains(pos)
                    || globalRect(titleBar->closeButton()).contains(pos)) {
                    *result = HTCLIENT;
                }
                else {
                    *result = HTCAPTION;
                }
                return true;
            }
            break;
        }

        default:
            break;
        }
#endif
        return QMainWindow::nativeEvent(eventType, message, result);
    }

private:
    Titlebar *titleBar = nullptr;
};

int main(int argc, char *argv[])
{
    QApplication app(argc, argv);

    MainWindow window;
    window.show();

    return app.exec();
}
