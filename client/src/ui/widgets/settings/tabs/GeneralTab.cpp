#include "GeneralTab.hpp"

#include <QVBoxLayout>

GeneralTab::GeneralTab(QWidget *parent)
    : QWidget(parent)
{
    auto *layout = new QVBoxLayout(this);
    // Settings rows go here; the stretch keeps them at the top of the page.
    layout->addStretch();
}
