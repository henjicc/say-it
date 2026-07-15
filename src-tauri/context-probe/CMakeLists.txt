cmake_minimum_required(VERSION 3.20)
project(say_it_context_probe LANGUAGES CXX)

add_executable(context-probe WIN32
  src/main.cpp
  src/ia2_text.h
)

target_compile_features(context-probe PRIVATE cxx_std_20)
target_compile_definitions(context-probe PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_compile_options(context-probe PRIVATE /W4 /permissive- /EHsc /utf-8)
target_link_libraries(context-probe PRIVATE
  UIAutomationCore
  Oleacc
  OleAut32
  Ole32
  User32
  Advapi32
  Shell32
)

set_target_properties(context-probe PROPERTIES
  OUTPUT_NAME "context-probe"
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
  RUNTIME_OUTPUT_DIRECTORY_RELEASE "${CMAKE_BINARY_DIR}/bin"
)

include(CTest)
if(BUILD_TESTING)
  add_test(NAME context-probe-self-test COMMAND $<TARGET_FILE:context-probe> --self-test)
endif()
