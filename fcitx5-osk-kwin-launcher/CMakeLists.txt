option(BUILD_KWIN_PLUGIN "Build the kwin plugin" FALSE)

if (BUILD_KWIN_PLUGIN)
  add_subdirectory(kwin-plugin)
endif()
