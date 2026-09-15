find_package(Qt6 REQUIRED COMPONENTS Widgets)

qt_standard_project_setup()

qt_add_executable(feather-chat src/main.cpp)

target_link_libraries(feather-chat PRIVATE Qt6::Widgets)
