#pragma once

#include <QHBoxLayout>
#include <QLabel>
#include <QPoint>
#include <QWidget>

class QPushButton;

class Titlebar : public QWidget {
    Q_OBJECT
public:
    explicit Titlebar(QWidget *parent = nullptr);
    void setWindowName(const QString &newName);

    QPushButton *minButton() const { return minBtn; }
    QPushButton *maxButton() const { return maxBtn; }
    QPushButton *closeButton() const { return closeBtn; }

protected:
    void mousePressEvent(QMouseEvent *ev) override;
    void mouseMoveEvent(QMouseEvent *ev) override;
    void mouseReleaseEvent(QMouseEvent *ev) override;
    void mouseDoubleClickEvent(QMouseEvent *ev) override;
    void changeEvent(QEvent *ev) override;

signals:
    void minimizeRequested();
    void maximizeRequested();
    void closeRequested();

private:
    QLabel *windowName;
    QPushButton *minBtn = nullptr;
    QPushButton *maxBtn = nullptr;
    QPushButton *closeBtn = nullptr;
    bool dragging = false;
    QPoint grabInTitlebar;
};