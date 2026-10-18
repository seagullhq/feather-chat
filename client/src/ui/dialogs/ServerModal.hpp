#pragma once
#include <QFrame>
#include <QMainWindow>
#include <QMouseEvent>
#include <QPainter>
#include <QWidget>

class ServerModal : public QWidget {
    Q_OBJECT
public:
    ServerModal(QWidget *parent);

public slots:
    void open();
    void close();

signals:
    void createServerRequested();
    void joinServerRequested();

protected:
    void paintEvent(QPaintEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    void keyPressEvent(QKeyEvent *event) override;
    bool eventFilter(QObject *watched, QEvent *event) override;

private:
    QFrame *card;
};
