cmake_minimum_required(VERSION 3.28)

file(STRINGS "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml" CARGO_TOML REGEX "^version =")
list(GET CARGO_TOML 0 VERSION_LINE)
string(REGEX REPLACE "^version = \"(.+)\"" "\\1" RHESIS_VERSION "${VERSION_LINE}")
project(rhesis VERSION ${RHESIS_VERSION})

# --- Options ---
set(RHESIS_BINARY_NAME "rhesis" CACHE STRING "Name of the executable")

# --- Dependencies ---
find_package(ECM 6.0 REQUIRED NO_MODULE)
set(CMAKE_MODULE_PATH ${ECM_MODULE_PATH})
include(KDEInstallDirs)
include(ECMUninstallTarget)

include(ECMFindQmlModule)
find_package(Qt6 REQUIRED COMPONENTS Gui)
find_package(Qt6 COMPONENTS QmlTools QUIET)
find_package(Qt6 COMPONENTS LinguistTools QUIET)

# --- Options ---
set(CARGO_BUILD_PROFILE "release" CACHE STRING "Cargo build profile (release, release-fast, debug)")

# --- Build the Rust binary ---
add_custom_target(rhesis_build ALL
    COMMAND cargo build --profile ${CARGO_BUILD_PROFILE}
              --target-dir ${CMAKE_CURRENT_BINARY_DIR}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    COMMENT "Building rhesis with cargo (profile=${CARGO_BUILD_PROFILE})"
)

# Determine binary output location
if(CARGO_BUILD_PROFILE STREQUAL "debug")
    set(RHESIS_BINARY_PATH "${CMAKE_CURRENT_BINARY_DIR}/debug/${RHESIS_BINARY_NAME}${CMAKE_EXECUTABLE_SUFFIX}")
else()
    set(RHESIS_BINARY_PATH "${CMAKE_CURRENT_BINARY_DIR}/${CARGO_BUILD_PROFILE}/${RHESIS_BINARY_NAME}${CMAKE_EXECUTABLE_SUFFIX}")
endif()

# --- Translations ---
set(RHESIS_TRANSLATION_SOURCES
    ${CMAKE_CURRENT_SOURCE_DIR}/src/interop/qml/Root.qml
    ${CMAKE_CURRENT_SOURCE_DIR}/src/interop/qml/MainPage.qml
    ${CMAKE_CURRENT_SOURCE_DIR}/src/interop/qml/SettingsPage.qml
    ${CMAKE_CURRENT_SOURCE_DIR}/src/interop/qml/components/RuleListItem.qml
)

file(GLOB RHESIS_TRANSLATION_FILES
    ${CMAKE_CURRENT_SOURCE_DIR}/translations/*.ts
)

if(Qt6LinguistTools_FOUND)
    qt6_add_translation(RHESIS_QM_FILES ${RHESIS_TRANSLATION_FILES})
    add_custom_target(translations ALL DEPENDS ${RHESIS_QM_FILES})

    install(
        FILES ${RHESIS_QM_FILES}
        DESTINATION ${KDE_INSTALL_BINDIR}/translations
        COMPONENT Runtime
    )
endif()

# --- Install rules ---
install(
    PROGRAMS ${RHESIS_BINARY_PATH}
    DESTINATION ${KDE_INSTALL_BINDIR}
    COMPONENT Runtime
)

install(
    FILES ${CMAKE_CURRENT_SOURCE_DIR}/io.github.dimkar3000.rhesis.desktop
    DESTINATION ${KDE_INSTALL_APPDIR}
    COMPONENT Runtime
)
