/// Returns the name of the app the OS opens files with this extension in, like "Photos" for
/// ".png", or None when no app is set.
#[cfg(windows)]
pub fn default_app(extension: &str) -> Option<String> {
    use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_FRIENDLYAPPNAME};
    use windows::core::{HSTRING, PCWSTR, PWSTR};

    let extension = HSTRING::from(format!(".{extension}"));
    let mut buffer = [0u16; 260];
    let mut length = buffer.len() as u32;
    // Without IGNOREUNKNOWN, Windows names its "Open with" picker for extensions no app claims.
    let result = unsafe { AssocQueryStringW(ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_FRIENDLYAPPNAME, &extension, PCWSTR::null(), Some(PWSTR(buffer.as_mut_ptr())), &mut length) };
    if result.is_err() {
        return None;
    }
    let name = String::from_utf16_lossy(&buffer[..length.saturating_sub(1) as usize]);
    (!name.trim().is_empty()).then_some(name)
}

#[cfg(not(windows))]
pub fn default_app(_extension: &str) -> Option<String> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn names_the_app_for_a_common_extension_and_none_for_an_unknown_one() {
        assert!(super::default_app("txt").is_some());
        assert!(super::default_app("packraven-no-such-extension").is_none());
    }
}
