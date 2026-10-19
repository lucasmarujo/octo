#[cfg(windows)]
#[test]
fn the_windows_binary_carries_the_orkai_icon() {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ExtractIconExW(file: *const u16, index: i32, large: *mut isize, small: *mut isize, icons: u32) -> u32;
    }

    let path: Vec<u16> = std::ffi::OsStr::new(env!("CARGO_BIN_EXE_orkai")).encode_wide().chain([0]).collect();
    let icons = unsafe { ExtractIconExW(path.as_ptr(), -1, std::ptr::null_mut(), std::ptr::null_mut(), 0) };
    assert_eq!(icons, 1, "orkai.exe should carry exactly one icon group");
}
