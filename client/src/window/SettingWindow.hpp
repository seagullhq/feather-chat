#pragma once

#include <QMainWindow>

// A second top-level window, opened from the user settings button in
// ServerCall. It deliberately takes no parent: a parent would turn it into a
// child widget embedded in the main window instead of a window of its own.
// The caller owns the instance (heap-allocate it and show()).
class SettingWindow final : public QMainWindow {
    Q_OBJECT

public:
    SettingWindow();
};
