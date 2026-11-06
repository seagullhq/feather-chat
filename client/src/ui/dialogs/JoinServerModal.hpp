#pragma once
#include <QWidget>
#include <QFrame>

class JoinServerModal : public QWidget{
    Q_OBJECT
public:
    JoinServerModal(QWidget* parent);

public slots:
    void open();
    void close();

protected:
    void paintEvent(QPaintEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    void keyPressEvent(QKeyEvent *event) override;
    bool eventFilter(QObject *watched, QEvent *event) override;

private:
    QFrame* card;
};
