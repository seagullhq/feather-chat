#include <QApplication>
#include <QLabel>

int main(int argc, char *argv[])
{
    QApplication app(argc, argv);

    QLabel label("hello from feather-chat");
    label.setWindowTitle("Feather");
    label.show();

    return app.exec();
}
