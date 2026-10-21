#pragma once
#include <QFrame>
#include <QWidget>

class CreateServerModal : public QWidget {
    Q_OBJECT
public:
    CreateServerModal(QWidget *parent);

public slots:
    void open();
    void close();

protected:
    void paintEvent(QPaintEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    void keyPressEvent(QKeyEvent *event) override;
    bool eventFilter(QObject *watched, QEvent *event) override;

private:
    QFrame *card;
};
