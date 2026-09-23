cmake_minimum_required(VERSION 3.16)
project(dahook VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

# Dependencias resolvidas na hora do build: verifica (e tenta instalar) antes
# de procurar o Qt. Desligue com -DAHOOK_AUTO_DEPS=OFF se gerencia na mão.
option(DAHOOK_AUTO_DEPS "verificar/instalar dependencias no configure" ON)
if(DAHOOK_AUTO_DEPS)
    execute_process(
        COMMAND bash "${CMAKE_SOURCE_DIR}/tools/deps.sh" check
        RESULT_VARIABLE DAHOOK_DEPS_RES
        OUTPUT_VARIABLE DAHOOK_DEPS_OUT
        ERROR_VARIABLE DAHOOK_DEPS_OUT
    )
    message(STATUS "dahook deps:\n${DAHOOK_DEPS_OUT}")
    if(NOT DAHOOK_DEPS_RES EQUAL 0)
        message(STATUS "tentando instalar dependencias faltantes...")
        execute_process(
            COMMAND bash "${CMAKE_SOURCE_DIR}/tools/deps.sh" install
            RESULT_VARIABLE DAHOOK_INS_RES
            OUTPUT_VARIABLE DAHOOK_INS_OUT
            ERROR_VARIABLE DAHOOK_INS_OUT
        )
        message(STATUS "${DAHOOK_INS_OUT}")
        if(NOT DAHOOK_INS_RES EQUAL 0)
            message(FATAL_ERROR "Dependencias faltando. Instale e reconfigure (ou -DAHOOK_AUTO_DEPS=OFF).")
        endif()
    endif()
endif()

find_package(Qt6 REQUIRED COMPONENTS Widgets WebEngineWidgets)

add_executable(dahook main.cpp)
target_link_libraries(dahook PRIVATE Qt6::Widgets Qt6::WebEngineWidgets)
