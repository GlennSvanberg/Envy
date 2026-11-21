use gpui_kit::{actions, KeyBinding};

actions!(
    envy,
    [
        MoveDown,
        MoveUp,
        MoveLeft,
        MoveRight,
        SwitchZone,
        SwitchZoneBack,
        Confirm,
        Cancel,
        CopyValue,
        ToggleReveal,
        RevealAll,
        NewKey,
        RenameKey,
        DeleteKey,
        UndoChange,
        RedoChange,
        Save,
        Refresh,
        ToggleDiff,
        CycleDiff,
        NewFromExample,
        AddFolder,
        OpenSettings,
        ReloadExternal,
        KeepMine,
        PasteBlock,
        ShowShortcuts,
        FocusSidebar,
        FocusKeys,
        ToggleFavorite,
        FocusSearch,
    ]
);

pub fn bind(cx: &mut gpui_kit::App) {
    // Letter and editing keys stay out of a text field. The field's context is
    // `Input`, which sits above `Envy` while it is focused.
    let list = Some("Envy && !Input");
    cx.bind_keys([
        KeyBinding::new("j", MoveDown, list),
        KeyBinding::new("down", MoveDown, list),
        KeyBinding::new("k", MoveUp, list),
        KeyBinding::new("up", MoveUp, list),
        KeyBinding::new("h", MoveLeft, list),
        KeyBinding::new("l", MoveRight, list),
        KeyBinding::new("left", FocusSidebar, list),
        KeyBinding::new("right", FocusKeys, list),
        KeyBinding::new("f", ToggleFavorite, list),
        KeyBinding::new("/", FocusSearch, list),
        KeyBinding::new("ctrl-f", FocusSearch, Some("Envy")),
        KeyBinding::new("enter", Confirm, list),
        KeyBinding::new("c", CopyValue, list),
        KeyBinding::new("space", ToggleReveal, list),
        KeyBinding::new("a", RevealAll, list),
        KeyBinding::new("n", NewKey, list),
        KeyBinding::new("f2", RenameKey, list),
        KeyBinding::new("x", DeleteKey, list),
        KeyBinding::new("delete", DeleteKey, list),
        KeyBinding::new("backspace", DeleteKey, list),
        // `ctrl` is the shortcut modifier. In this GPUI, `cmd` is the
        // Windows/Super key, not Control.
        KeyBinding::new("ctrl-z", UndoChange, Some("Envy")),
        KeyBinding::new("ctrl-y", RedoChange, Some("Envy")),
        KeyBinding::new("ctrl-shift-z", RedoChange, Some("Envy")),
        KeyBinding::new("shift-/", ShowShortcuts, list),
        KeyBinding::new("?", ShowShortcuts, list),
        KeyBinding::new("ctrl-s", Save, Some("Envy")),
        KeyBinding::new("ctrl-r", Refresh, Some("Envy")),
        KeyBinding::new("d", ToggleDiff, list),
        KeyBinding::new("]", CycleDiff, list),
        KeyBinding::new("ctrl-shift-n", NewFromExample, Some("Envy")),
        KeyBinding::new("ctrl-o", AddFolder, Some("Envy")),
        KeyBinding::new("ctrl-,", OpenSettings, Some("Envy")),
        KeyBinding::new("r", ReloadExternal, list),
        KeyBinding::new("ctrl-v", PasteBlock, list),
    ]);
}
