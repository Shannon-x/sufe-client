// Sufe is a GUI application, including locally packaged review builds.
// Development output remains available through inherited/redirected handles.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    xboard_desktop_lib::run();
}
