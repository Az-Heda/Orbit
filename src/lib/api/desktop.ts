import { call } from "./invoke";

export const toggleWidget = () => call<void>("toggle_widget");
export const setWidgetClickThrough = (enabled: boolean) => call<void>("set_widget_click_through", { enabled });
export const openPalette = () => call<void>("open_palette");
export const openCapture = () => call<void>("open_capture");
export const hideWindow = (label: "widget" | "palette" | "capture") => call<void>("hide_window", { label });
export const openMain = (path?: string) => call<void>("open_main", { path });
export const getAutostart = () => call<boolean>("get_autostart");
export const setAutostart = (enabled: boolean) => call<boolean>("set_autostart", { enabled });
export const shortcutFailures = () => call<string[]>("shortcut_failures");
export const applyShortcuts = () => call<string[]>("apply_shortcuts");
