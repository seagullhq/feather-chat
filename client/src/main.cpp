#include <QApplication>
#include <QMainWindow>
#include <QFrame>
#include <QPushButton>
#include <QVBoxLayout>
#include <QStackedWidget>
#include <QLabel>

class MainWindow : public QMainWindow
{
public:
    MainWindow()
    {
        auto *central = new QWidget;
        auto *layout = new QHBoxLayout(central);

        auto *sidebar = new QFrame;
        sidebar->setFixedWidth(200);

        auto palette = QApplication::palette();
        palette.setColor(QPalette::Window, palette.color(QPalette::Base));
        sidebar->setAutoFillBackground(true);
        sidebar->setPalette(palette);


        auto *sidebarLayout = new QVBoxLayout(sidebar);

        auto *homeButton = new QPushButton("Home");
        auto *settingsButton = new QPushButton("Settings");

        sidebarLayout->addWidget(homeButton);
        sidebarLayout->addStretch();
        sidebarLayout->addWidget(settingsButton);

        // Pages
        auto *pages = new QStackedWidget;

        auto *homePage = new QLabel("Home");
        auto *settingsPage = new QLabel("Settings");
        pages->addWidget(homePage);
        pages->addWidget(settingsPage);

        // Sidebar + content
        layout->setContentsMargins({0, 1, 0, 0});
        layout->addWidget(sidebar);
        layout->addWidget(pages);

        // Navigation
        connect(homeButton, &QPushButton::clicked,
                pages, [pages] { pages->setCurrentIndex(0); });

        connect(settingsButton, &QPushButton::clicked,
                pages, [pages] { pages->setCurrentIndex(1); });

        setCentralWidget(central);
        setWindowTitle("Feather Chat");
        resize(800, 600);
    }
};

int main(int argc, char *argv[])
{
    QApplication app(argc, argv);

    MainWindow window;
    window.show();

    return app.exec();
}
