find_package(Qt6 REQUIRED COMPONENTS Widgets)

qt_standard_project_setup()

qt_add_executable(feather-chat src/main.cpp)

# Static builds do not need deployment, and the missing qtpaths tool can cause windeployqt to corrupt the executable's manifest when it tries to embed qt.conf
set_target_properties(feather-chat PROPERTIES 
    QT_NO_DEPLOY ON 
    QT_WINDOWS_APP_PROJECT_EXECUTION_LEVEL "asInvoker"
)

target_link_libraries(feather-chat PRIVATE Qt6::Widgets)
