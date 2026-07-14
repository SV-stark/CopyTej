use windows::Win32::Foundation::HINSTANCE;
use windows::core::HRESULT;

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(
    _hmodule: HINSTANCE,
    _ul_reason_for_call: u32,
    _lp_reserved: *mut std::ffi::c_void,
) -> bool {
    true
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn DllGetClassObject(
    _rclsid: *const windows::core::GUID,
    _riid: *const windows::core::GUID,
    _ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    HRESULT(-2147467262) // E_NOINTERFACE
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    HRESULT(1) // S_FALSE
}
