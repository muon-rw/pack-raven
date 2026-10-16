use std::path::PathBuf;

use tauri::Window;

/// Returns whether you held the file manager's move or copy modifier, if you held either.
/// Explorer moves with Shift and copies with Ctrl.
#[cfg(windows)]
pub fn requested_move() -> Option<bool> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_SHIFT};
    let held = |key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| unsafe { GetAsyncKeyState(key.0 as i32) } < 0;
    if held(VK_SHIFT) {
        Some(true)
    } else if held(VK_CONTROL) {
        Some(false)
    } else {
        None
    }
}

#[cfg(not(windows))]
pub fn requested_move() -> Option<bool> {
    None
}

// The shell only parses backslash paths without the \\?\ prefix, and settings can store the
// pack folder with forward slashes.
#[cfg(windows)]
fn shell_path(path: &std::path::Path) -> windows::core::HSTRING {
    let path = path.to_string_lossy().replace('/', "\\");
    windows::core::HSTRING::from(path.strip_prefix(r"\\?\").unwrap_or(&path))
}

// We hand Explorer a shell data object for the files, like the one it builds for its own
// drags, so it draws the usual file icons and applies its own rules: a move on the same drive,
// a copy across drives, and Shift or Ctrl to choose.
#[cfg(windows)]
pub fn drag_out(window: &Window, paths: &[PathBuf]) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::IDataObject;
    use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, IDropSource};
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{BHID_DataObject, ILCreateFromPathW, ILFree, IShellItemArray, SHCreateShellItemArrayFromIDLists, SHDoDragDrop};

    let hwnd = HWND(window.hwnd().map_err(|e| e.to_string())?.0 as _);
    let pidls: Vec<*const ITEMIDLIST> = paths.iter().map(|p| unsafe { ILCreateFromPathW(&shell_path(p)) } as *const _).collect();
    let result = (|| unsafe {
        if pidls.iter().any(|p| p.is_null()) {
            return Err("couldn't find one of the files to drag".to_string());
        }
        let items: IShellItemArray = SHCreateShellItemArrayFromIDLists(&pidls).map_err(|e| e.to_string())?;
        let data: IDataObject = items.BindToHandler(None, &BHID_DataObject).map_err(|e| e.to_string())?;
        SHDoDragDrop(Some(hwnd), &data, None::<&IDropSource>, DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK).map_err(|e| e.to_string())?;
        Ok(())
    })();
    for pidl in pidls {
        unsafe { ILFree(Some(pidl)) };
    }
    result
}

#[cfg(not(windows))]
pub fn drag_out(window: &Window, paths: &[PathBuf]) -> Result<(), String> {
    const DRAG_IMAGE: &[u8] = include_bytes!("../icons/32x32.png");
    let item = drag::DragItem::Files(paths.to_vec());
    // On Linux, the drag crate wants the GTK window rather than the Tauri one.
    #[cfg(target_os = "linux")]
    let window = &window.gtk_window().map_err(|e| e.to_string())?;
    drag::start_drag(window, item, drag::Image::Raw(DRAG_IMAGE.to_vec()), |_, _| {}, drag::Options::default()).map_err(|e| e.to_string())
}

#[cfg(all(test, windows))]
mod tests {
    use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree};

    #[test]
    fn shell_parses_forward_slash_pack_paths() {
        let file = std::env::temp_dir().join("packraven-shell-path.json");
        std::fs::write(&file, "{}").unwrap();
        let forward = std::path::PathBuf::from(file.to_string_lossy().replace('\\', "/"));
        let verbatim = file.canonicalize().unwrap();
        unsafe {
            assert!(ILCreateFromPathW(&windows::core::HSTRING::from(forward.as_os_str())).is_null(), "the shell rejects forward slashes");
            for path in [&forward, &verbatim] {
                let pidl = ILCreateFromPathW(&super::shell_path(path));
                assert!(!pidl.is_null(), "{}", path.display());
                ILFree(Some(pidl));
            }
        }
    }
}
