#include "pages/MainWindow.hpp"
#include <QApplication>
#include <QIcon>

int main(int argc, char *argv[])
{
    QApplication app(argc, argv);
    app.setWindowIcon(QIcon(":/feather.svg"));

    MainWindow window;
    window.show();

    return app.exec();
}
