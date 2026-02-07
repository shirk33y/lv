// No windows_subsystem = "windows" here — we need the console for CLI mode.
// For GUI mode on Windows, we call FreeConsole() instead.

fn main() {
    lv_lib::run();
}
